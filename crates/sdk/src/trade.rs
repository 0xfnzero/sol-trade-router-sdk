//! High-level buy/sell — hot path with **zero RPC**.
//!
//! Supports:
//! - Pay with native SOL / existing WSOL / held stock(quote) token
//! - Receive native SOL / WSOL / stock(quote) token
//! - Inner (LaunchLab / PumpFun) + outer (CPMM), 1-hop or 2-hop as needed

use anyhow::{anyhow, Result};
use solana_sdk::{instruction::Instruction, pubkey::Pubkey};

use crate::{
    asset::{BuyWith, SellTo},
    ata::{
        ata, create_ata, create_ata_idempotent, wrap_sol_with_options, AtaKind, AtaPolicy,
        TouchedAtas,
    },
    config_pda,
    constants::{PROGRAM_ID, TOKEN_PROGRAM, WSOL_MINT},
    legs::{
        cpmm_swap_leg, launchlab_buy_leg, launchlab_sell_leg, pumpfun_buy_leg, pumpfun_sell_leg,
        Leg,
    },
    market::{CpmmPool, LaunchLabPool, Market, RoutedMarket},
    quote::{
        apply_slippage_min_out, cpmm_out, fee_amount, launchlab_buy_quote, launchlab_sell_quote_out,
        pumpfun_buy_token_out, pumpfun_sell_sol_out,
    },
    route_ix::{
        build_route_instruction, sol_fee_program, token_fee_program, RouteAccounts, FEE_ASSET_SOL,
        FEE_ASSET_TOKEN,
    },
};

#[derive(Clone, Debug)]
pub struct TradeOpts {
    /// Slippage in basis points (default 100 = 1%).
    pub slippage_bps: u64,
    /// Buy input asset (`buy_with_*`).
    pub buy_with: BuyWith,
    /// Sell output asset (`sell_to_*`).
    pub sell_to: SellTo,
    /// ATA create / close in the same trade tx.
    /// Buy builders use [`AtaPolicy::for_buy`] (create meme in-tx only).
    pub ata: AtaPolicy,
}

impl Default for TradeOpts {
    fn default() -> Self {
        Self {
            slippage_bps: 100,
            buy_with: BuyWith::Sol,
            sell_to: SellTo::Sol,
            ata: AtaPolicy::default(),
        }
    }
}

impl TradeOpts {
    pub fn with_slippage_bps(mut self, slippage_bps: u64) -> Self {
        self.slippage_bps = slippage_bps;
        self
    }
    pub fn buy_with_sol(mut self) -> Self {
        self.buy_with = BuyWith::Sol;
        self.ata.create_meme = true;
        self
    }
    pub fn buy_with_wsol(mut self) -> Self {
        self.buy_with = BuyWith::Wsol;
        self.ata.create_meme = true;
        self
    }
    pub fn buy_with_token(mut self, mint: Pubkey) -> Self {
        self.buy_with = BuyWith::Token(mint);
        self.ata.create_meme = true;
        self
    }
    pub fn sell_to_sol(mut self) -> Self {
        self.sell_to = SellTo::Sol;
        self.ata.create_meme = false;
        // Native SOL settlement unwraps WSOL after the route.
        self.ata.close_wsol = true;
        self
    }
    pub fn sell_to_wsol(mut self) -> Self {
        self.sell_to = SellTo::Wsol;
        self.ata.create_meme = false;
        self
    }
    pub fn sell_to_token(mut self, mint: Pubkey) -> Self {
        self.sell_to = SellTo::Token(mint);
        self.ata.create_meme = false;
        self
    }
    pub fn with_ata(mut self, ata: AtaPolicy) -> Self {
        self.ata = ata;
        self
    }
    /// Create meme ATA inside this buy tx (default on for buy_* builders).
    pub fn create_meme(mut self, create: bool) -> Self {
        self.ata.create_meme = create;
        self
    }
    /// Create WSOL inside this trade tx (default off — prepare ahead).
    pub fn create_wsol(mut self, create: bool) -> Self {
        self.ata.create_wsol = create;
        self
    }
    /// Create stock/quote inside this trade tx (default off — prepare ahead).
    pub fn create_quote(mut self, create: bool) -> Self {
        self.ata.create_quote = create;
        self
    }
    /// Opt-in: create WSOL + quote (+ meme if already enabled) in-trade.
    pub fn create_shared_atas(mut self, create: bool) -> Self {
        self.ata.create_wsol = create;
        self.ata.create_quote = create;
        self
    }
    pub fn close_wsol(mut self, close: bool) -> Self {
        self.ata.close_wsol = close;
        self
    }
    pub fn close_meme(mut self, close: bool) -> Self {
        self.ata.close_meme = close;
        self
    }
    pub fn close_quote(mut self, close: bool) -> Self {
        self.ata.close_quote = close;
        self
    }
}

#[inline]
fn push_create_ata(
    setup: &mut Vec<Instruction>,
    policy: &AtaPolicy,
    kind: AtaKind,
    payer: &Pubkey,
    mint: &Pubkey,
    token_program: &Pubkey,
) {
    if policy.allows(kind) {
        setup.push(create_ata_idempotent(payer, payer, mint, token_program));
    }
}

/// Ready-to-sign instruction bundle.
#[derive(Clone, Debug, Default)]
pub struct BuiltTrade {
    pub setup: Vec<Instruction>,
    pub route: Option<Instruction>,
    pub cleanup: Vec<Instruction>,
}

impl BuiltTrade {
    pub fn into_instructions(self) -> Vec<Instruction> {
        let mut out = self.setup;
        if let Some(route) = self.route {
            out.push(route);
        }
        out.extend(self.cleanup);
        out
    }

    #[inline]
    pub fn into_vec(self) -> Vec<Instruction> {
        self.into_instructions()
    }
}

/// Client for the Pinocchio router program.
#[derive(Clone, Debug)]
pub struct RouterClient {
    /// Transaction fee payer / trade authority.
    pub payer: Pubkey,
    pub program_id: Pubkey,
    pub fee_recipient: Pubkey,
    /// Must match on-chain config (used for local fee netting).
    pub fee_bps: u16,
}

impl RouterClient {
    pub fn new(payer: Pubkey, fee_recipient: Pubkey, fee_bps: u16) -> Self {
        Self {
            payer,
            program_id: PROGRAM_ID,
            fee_recipient,
            fee_bps,
        }
    }

    pub fn with_program_id(mut self, program_id: Pubkey) -> Self {
        self.program_id = program_id;
        self
    }

    pub fn config_address(&self) -> Pubkey {
        config_pda(&self.program_id).0
    }

    // ── Standalone ATA ops (prefer cold path / prepare tx — saves hot-path CU) ──

    /// Create any ATA for `payer` as owner.
    pub fn create_ata(&self, mint: Pubkey, token_program: Pubkey) -> Instruction {
        create_ata(&self.payer, &self.payer, &mint, &token_program)
    }

    /// Close any ATA owned by `payer` (must be empty).
    pub fn close_ata(&self, mint: Pubkey, token_program: Pubkey) -> Instruction {
        crate::ata::close_ata(&self.payer, &mint, &token_program, &self.payer)
    }

    pub fn create_wsol_ata(&self) -> Instruction {
        crate::ata::create_wsol_ata(&self.payer)
    }

    pub fn close_wsol_ata(&self) -> Instruction {
        crate::ata::close_wsol_ata(&self.payer)
    }

    pub fn create_meme_ata(&self, market: &RoutedMarket) -> Instruction {
        self.create_ata(market.meme_mint(), market.meme_token_program())
    }

    pub fn close_meme_ata(&self, market: &RoutedMarket) -> Instruction {
        self.close_ata(market.meme_mint(), market.meme_token_program())
    }

    pub fn create_quote_ata(&self, market: &RoutedMarket) -> Instruction {
        self.create_ata(market.market.quote_mint(), market.market.quote_token_program())
    }

    pub fn close_quote_ata(&self, market: &RoutedMarket) -> Instruction {
        self.close_ata(market.market.quote_mint(), market.market.quote_token_program())
    }

    /// Cold-path: prepare **reusable** ATAs for buys (WSOL + stock/quote + fee recipient).
    /// Does **not** create meme ATA — that belongs in the same buy tx (`create_meme`).
    pub fn prepare_buy_atas(&self, market: &RoutedMarket, buy_with: BuyWith) -> Vec<Instruction> {
        let mut ixs = Vec::new();
        match buy_with {
            BuyWith::Sol | BuyWith::Wsol => {
                if !matches!(&market.market, Market::PumpFunInner(_)) {
                    ixs.push(self.create_wsol_ata());
                    // Fee from WSOL (non-PumpFun SOL/WSOL buys).
                    ixs.push(create_ata(
                        &self.payer,
                        &self.fee_recipient,
                        &WSOL_MINT,
                        &TOKEN_PROGRAM,
                    ));
                }
                if market.market.needs_sol_bridge() {
                    ixs.push(self.create_quote_ata(market));
                }
            }
            BuyWith::Token(mint) => {
                let tp = if mint == WSOL_MINT {
                    TOKEN_PROGRAM
                } else {
                    market.market.quote_token_program()
                };
                ixs.push(self.create_ata(mint, tp));
                ixs.push(create_ata(&self.payer, &self.fee_recipient, &mint, &tp));
            }
        }
        ixs
    }

    /// Cold-path: prepare sell ATAs (output side + fee recipient meme ATA).
    pub fn prepare_sell_atas(&self, market: &RoutedMarket, sell_to: SellTo) -> Vec<Instruction> {
        let mut ixs = Vec::new();
        let meme = market.meme_mint();
        let meme_tp = market.meme_token_program();
        ixs.push(create_ata(
            &self.payer,
            &self.fee_recipient,
            &meme,
            &meme_tp,
        ));
        match sell_to {
            SellTo::Sol | SellTo::Wsol => {
                if !matches!(&market.market, Market::PumpFunInner(_)) {
                    ixs.push(self.create_wsol_ata());
                }
                if market.market.needs_sol_bridge() {
                    ixs.push(self.create_quote_ata(market));
                }
            }
            SellTo::Token(mint) => {
                let tp = if mint == WSOL_MINT {
                    TOKEN_PROGRAM
                } else {
                    market.market.quote_token_program()
                };
                ixs.push(self.create_ata(mint, tp));
            }
        }
        ixs
    }

    /// Buy meme paying with native SOL (may 1-hop or 2-hop).
    /// Creates meme ATA in-tx; WSOL/stock should be prepared via [`Self::prepare_buy_atas`].
    pub fn buy_with_sol(&self, amount_in: u64, market: &RoutedMarket) -> Result<BuiltTrade> {
        self.buy_with_opts(amount_in, market, TradeOpts::default().buy_with_sol())
    }

    /// Buy meme paying with existing WSOL (no wrap).
    pub fn buy_with_wsol(&self, amount_in: u64, market: &RoutedMarket) -> Result<BuiltTrade> {
        self.buy_with_opts(amount_in, market, TradeOpts::default().buy_with_wsol())
    }

    /// Buy meme paying with held stock/quote token (single hop, no SOL bridge).
    pub fn buy_with_token(
        &self,
        amount_in: u64,
        market: &RoutedMarket,
        mint: Pubkey,
    ) -> Result<BuiltTrade> {
        self.buy_with_opts(amount_in, market, TradeOpts::default().buy_with_token(mint))
    }

    /// Generic buy with explicit [`TradeOpts`].
    pub fn buy_with_opts(
        &self,
        amount_in: u64,
        market: &RoutedMarket,
        opts: TradeOpts,
    ) -> Result<BuiltTrade> {
        if amount_in == 0 {
            return Err(anyhow!("amount_in is zero"));
        }

        let fee = fee_amount(amount_in, self.fee_bps);
        let spend = amount_in.saturating_sub(fee);
        if spend == 0 {
            return Err(anyhow!("amount after fee is zero"));
        }

        let payer = self.payer;
        let meme = market.meme_mint();
        let meme_tp = market.meme_token_program();
        let meme_ata = ata(&payer, &meme, &meme_tp);
        let mut setup = Vec::with_capacity(8);
        let mut touched = TouchedAtas::default();
        touched.touch_meme(meme, meme_tp);
        push_create_ata(
            &mut setup,
            &opts.ata,
            AtaKind::Meme,
            &payer,
            &meme,
            &meme_tp,
        );

        // Resolve path from buy_with.
        let needs_bridge = match opts.buy_with {
            BuyWith::Token(mint) => {
                if mint == meme {
                    return Err(anyhow!("buy_with token cannot be the meme mint"));
                }
                if mint != market.market.quote_mint() {
                    return Err(anyhow!(
                        "buy_with token {} is not this market's quote {}",
                        mint,
                        market.market.quote_mint()
                    ));
                }
                false
            }
            BuyWith::Sol | BuyWith::Wsol => market.market.needs_sol_bridge(),
        };
        if needs_bridge && market.bridge.is_none() {
            return Err(anyhow!(
                "SOL/WSOL path needs stock bridge; or use buy_with_token(mint)"
            ));
        }

        // PumpFun only accepts native SOL (not WSOL ATA / stock).
        let is_pump = matches!(&market.market, Market::PumpFunInner(_));
        if is_pump {
            match opts.buy_with {
                BuyWith::Sol => {}
                BuyWith::Wsol => {
                    return Err(anyhow!(
                        "PumpFun buy expects native SOL; unwrap WSOL first or use BuyWith::Sol"
                    ));
                }
                BuyWith::Token(_) => {
                    return Err(anyhow!("PumpFun buy does not accept stock/quote token"));
                }
            }
        }

        // Non-PumpFun SOL buys: wrap full amount_in into WSOL so on-chain can
        // verify fee_source (WSOL) spent >= amount_in (fee + swap).
        let will_use_wsol = match (&opts.buy_with, is_pump) {
            (BuyWith::Sol, false) | (BuyWith::Wsol, _) => true,
            _ => false,
        };
        if will_use_wsol {
            touched.touch_wsol();
        }
        if matches!(opts.buy_with, BuyWith::Sol) && !is_pump {
            setup.extend(wrap_sol_with_options(
                &payer,
                amount_in,
                opts.ata.create_wsol,
            ));
        } else if matches!(opts.buy_with, BuyWith::Wsol) {
            push_create_ata(
                &mut setup,
                &opts.ata,
                AtaKind::Wsol,
                &payer,
                &WSOL_MINT,
                &TOKEN_PROGRAM,
            );
        }

        let (legs, min_out) =
            self.build_buy_legs(spend, market, &opts, &mut setup, &mut touched)?;

        // Fee asset: PumpFun SOL stays native; other SOL/WSOL paths fee from WSOL.
        let (fee_asset, fee_destination, fee_source, fee_program) = match opts.buy_with {
            BuyWith::Sol if is_pump => (
                FEE_ASSET_SOL,
                self.fee_recipient,
                payer,
                sol_fee_program(),
            ),
            BuyWith::Sol | BuyWith::Wsol => {
                let fee_src = ata(&payer, &WSOL_MINT, &TOKEN_PROGRAM);
                let fee_dst = ata(&self.fee_recipient, &WSOL_MINT, &TOKEN_PROGRAM);
                // Ensure fee recipient WSOL ATA exists when charging a fee.
                if fee > 0 || opts.ata.create_wsol {
                    setup.push(create_ata_idempotent(
                        &payer,
                        &self.fee_recipient,
                        &WSOL_MINT,
                        &TOKEN_PROGRAM,
                    ));
                }
                (FEE_ASSET_TOKEN, fee_dst, fee_src, TOKEN_PROGRAM)
            }
            BuyWith::Token(mint) => {
                let tp = if mint == WSOL_MINT {
                    TOKEN_PROGRAM
                } else {
                    market.market.quote_token_program()
                };
                let kind = if mint == WSOL_MINT {
                    touched.touch_wsol();
                    AtaKind::Wsol
                } else {
                    touched.touch_quote(mint, tp);
                    AtaKind::Quote
                };
                let fee_src = ata(&payer, &mint, &tp);
                let fee_dst = ata(&self.fee_recipient, &mint, &tp);
                if fee > 0 || opts.ata.allows(kind) {
                    setup.push(create_ata_idempotent(&payer, &self.fee_recipient, &mint, &tp));
                }
                push_create_ata(&mut setup, &opts.ata, kind, &payer, &mint, &tp);
                (FEE_ASSET_TOKEN, fee_dst, fee_src, token_fee_program(&tp))
            }
        };

        let route = build_route_instruction(
            &self.program_id,
            RouteAccounts {
                payer,
                fee_destination,
                fee_source,
                output_token_account: meme_ata,
                fee_program,
            },
            amount_in,
            min_out,
            fee_asset,
            &legs,
        );

        let cleanup = opts.ata.build_cleanup(&payer, &touched);

        Ok(BuiltTrade {
            setup,
            route: Some(route),
            cleanup,
        })
    }

    /// Sell meme receiving native SOL.
    pub fn sell_to_sol(&self, amount_in: u64, market: &RoutedMarket) -> Result<BuiltTrade> {
        self.sell_with_opts(amount_in, market, TradeOpts::default().sell_to_sol())
    }

    /// Sell meme → WSOL (keep wrapped).
    pub fn sell_to_wsol(&self, amount_in: u64, market: &RoutedMarket) -> Result<BuiltTrade> {
        self.sell_with_opts(amount_in, market, TradeOpts::default().sell_to_wsol())
    }

    /// Sell meme → stock/quote only (no bridge back to SOL).
    pub fn sell_to_token(
        &self,
        amount_in: u64,
        market: &RoutedMarket,
        mint: Pubkey,
    ) -> Result<BuiltTrade> {
        self.sell_with_opts(
            amount_in,
            market,
            TradeOpts::default().sell_to_token(mint),
        )
    }

    /// Generic sell with explicit [`TradeOpts`].
    pub fn sell_with_opts(
        &self,
        amount_in: u64,
        market: &RoutedMarket,
        opts: TradeOpts,
    ) -> Result<BuiltTrade> {
        if amount_in == 0 {
            return Err(anyhow!("amount_in is zero"));
        }

        let fee = fee_amount(amount_in, self.fee_bps);
        let sell_amt = amount_in.saturating_sub(fee);
        if sell_amt == 0 {
            return Err(anyhow!("amount after fee is zero"));
        }

        let payer = self.payer;
        let meme = market.meme_mint();
        let meme_tp = market.meme_token_program();
        let meme_ata = ata(&payer, &meme, &meme_tp);
        let fee_ata = ata(&self.fee_recipient, &meme, &meme_tp);

        let needs_bridge = match opts.sell_to {
            SellTo::Token(mint) => {
                if mint == meme {
                    return Err(anyhow!("sell_to token cannot be the meme mint"));
                }
                if mint != market.market.quote_mint() {
                    return Err(anyhow!(
                        "sell_to token {} is not this market's quote {}",
                        mint,
                        market.market.quote_mint()
                    ));
                }
                false
            }
            SellTo::Sol | SellTo::Wsol => market.market.needs_sol_bridge(),
        };
        if needs_bridge && market.bridge.is_none() {
            return Err(anyhow!(
                "SOL/WSOL receive needs stock bridge; or use sell_to_token(mint)"
            ));
        }

        if matches!(&market.market, Market::PumpFunInner(_)) {
            if matches!(opts.sell_to, SellTo::Token(_)) {
                return Err(anyhow!("PumpFun sell only returns SOL"));
            }
            // PumpFun credits native SOL; WSOL receive not supported without wrap path.
            if matches!(opts.sell_to, SellTo::Wsol) {
                return Err(anyhow!(
                    "PumpFun sell pays native SOL; wrap afterward if you need WSOL"
                ));
            }
        }

        let mut setup = Vec::with_capacity(6);
        let mut touched = TouchedAtas::default();
        touched.touch_meme(meme, meme_tp);
        // Fee recipient meme ATA required when fee_bps > 0.
        if fee > 0 || opts.ata.create_meme {
            setup.push(create_ata_idempotent(
                &payer,
                &self.fee_recipient,
                &meme,
                &meme_tp,
            ));
        }
        if opts.sell_to.is_sol_family()
            && !matches!(&market.market, Market::PumpFunInner(_))
        {
            touched.touch_wsol();
            push_create_ata(
                &mut setup,
                &opts.ata,
                AtaKind::Wsol,
                &payer,
                &WSOL_MINT,
                &TOKEN_PROGRAM,
            );
        } else if let SellTo::Token(mint) = opts.sell_to {
            let tp = market.market.quote_token_program();
            let kind = if mint == WSOL_MINT {
                touched.touch_wsol();
                AtaKind::Wsol
            } else {
                touched.touch_quote(mint, tp);
                AtaKind::Quote
            };
            push_create_ata(&mut setup, &opts.ata, kind, &payer, &mint, &tp);
        }

        let (legs, min_out, output_ata) =
            self.build_sell_legs(sell_amt, market, &opts, &mut setup, &mut touched)?;

        let route = build_route_instruction(
            &self.program_id,
            RouteAccounts {
                payer,
                fee_destination: fee_ata,
                fee_source: meme_ata,
                output_token_account: output_ata,
                fee_program: token_fee_program(&meme_tp),
            },
            amount_in,
            min_out,
            FEE_ASSET_TOKEN,
            &legs,
        );

        let cleanup = opts.ata.build_cleanup(&payer, &touched);

        Ok(BuiltTrade {
            setup,
            route: Some(route),
            cleanup,
        })
    }

    fn build_buy_legs(
        &self,
        spend: u64,
        market: &RoutedMarket,
        opts: &TradeOpts,
        setup: &mut Vec<Instruction>,
        touched: &mut TouchedAtas,
    ) -> Result<(Vec<Leg>, u64)> {
        let payer = self.payer;
        let meme = market.meme_mint();
        let meme_tp = market.meme_token_program();
        let meme_ata = ata(&payer, &meme, &meme_tp);
        let slip = opts.slippage_bps;

        match (&opts.buy_with, &market.market) {
            // —— PumpFun: native SOL only ——
            (BuyWith::Sol, Market::PumpFunInner(pool)) => {
                let expected = pumpfun_buy_token_out(pool, spend);
                let min_out = apply_slippage_min_out(expected, slip);
                Ok((
                    vec![pumpfun_buy_leg(&payer, pool, spend, min_out, meme_ata)],
                    min_out,
                ))
            }

            // —— Pay stock/quote directly (single hop) ——
            (BuyWith::Token(pay_mint), Market::LaunchLabInner(pool)) => {
                self.buy_launchlab_with_quote(spend, pool, *pay_mint, slip, meme_ata, setup, touched, &opts.ata)
            }
            (BuyWith::Token(pay_mint), Market::CpmmOuter(pool)) => {
                self.buy_cpmm_with_mint(spend, pool, *pay_mint, meme, slip, setup, touched, &opts.ata)
            }

            // —— Pay SOL/WSOL, LaunchLab SOL-quoted ——
            (BuyWith::Sol | BuyWith::Wsol, Market::LaunchLabInner(pool)) if pool.is_sol_quote() => {
                let quote_ata = ata(&payer, &WSOL_MINT, &pool.quote_token_program);
                let q = launchlab_buy_quote(pool, spend, 0)?;
                let min_out = apply_slippage_min_out(q.amount_out, slip);
                Ok((
                    vec![launchlab_buy_leg(
                        &payer, pool, q.amount_in, min_out, meme_ata, quote_ata,
                    )],
                    min_out,
                ))
            }

            // —— Pay SOL/WSOL, LaunchLab stock-quoted → bridge ——
            (BuyWith::Sol | BuyWith::Wsol, Market::LaunchLabInner(pool)) => {
                let bridge = market
                    .bridge
                    .as_ref()
                    .ok_or_else(|| anyhow!("missing SOL↔stock bridge"))?;
                self.buy_via_bridge_launchlab(spend, pool, bridge, slip, setup, touched, &opts.ata)
            }

            // —— Pay SOL/WSOL, CPMM with WSOL side ——
            (BuyWith::Sol | BuyWith::Wsol, Market::CpmmOuter(pool))
                if !market.market.needs_sol_bridge() =>
            {
                let input_mint = WSOL_MINT;
                let output_mint = pool.meme_mint();
                self.buy_cpmm_with_mint(
                    spend, pool, input_mint, output_mint, slip, setup, touched, &opts.ata,
                )
            }

            // —— Pay SOL/WSOL, CPMM stock/meme → bridge ——
            (BuyWith::Sol | BuyWith::Wsol, Market::CpmmOuter(pool)) => {
                let bridge = market
                    .bridge
                    .as_ref()
                    .ok_or_else(|| anyhow!("missing SOL↔stock bridge"))?;
                self.buy_via_bridge_cpmm(spend, pool, bridge, slip, setup, touched, &opts.ata)
            }

            _ => Err(anyhow!("unsupported buy path for this buy_with / market")),
        }
    }

    fn build_sell_legs(
        &self,
        sell_amt: u64,
        market: &RoutedMarket,
        opts: &TradeOpts,
        setup: &mut Vec<Instruction>,
        touched: &mut TouchedAtas,
    ) -> Result<(Vec<Leg>, u64, Pubkey)> {
        let payer = self.payer;
        let meme = market.meme_mint();
        let meme_tp = market.meme_token_program();
        let meme_ata = ata(&payer, &meme, &meme_tp);
        let slip = opts.slippage_bps;

        match (&opts.sell_to, &market.market) {
            (SellTo::Sol, Market::PumpFunInner(pool)) => {
                let expected = pumpfun_sell_sol_out(pool, sell_amt);
                let min_out = apply_slippage_min_out(expected, slip);
                // Native SOL credit — router min_out on token ATA not applicable.
                Ok((
                    vec![pumpfun_sell_leg(&payer, pool, sell_amt, min_out, meme_ata)],
                    0,
                    meme_ata,
                ))
            }

            // —— Receive stock/quote only ——
            (SellTo::Token(out_mint), Market::LaunchLabInner(pool)) => {
                self.sell_launchlab_to_quote(sell_amt, pool, *out_mint, slip, meme_ata, setup, touched, &opts.ata)
            }
            (SellTo::Token(out_mint), Market::CpmmOuter(pool)) => {
                self.sell_cpmm_to_mint(sell_amt, pool, meme, *out_mint, slip, setup, touched, &opts.ata)
            }

            // —— Receive SOL/WSOL, LaunchLab SOL-quoted ——
            (SellTo::Sol | SellTo::Wsol, Market::LaunchLabInner(pool))
                if pool.is_sol_quote() =>
            {
                let quote_ata = ata(&payer, &WSOL_MINT, &pool.quote_token_program);
                let expected = launchlab_sell_quote_out(pool, sell_amt)?;
                let min_out = apply_slippage_min_out(expected, slip);
                Ok((
                    vec![launchlab_sell_leg(
                        &payer, pool, sell_amt, min_out, meme_ata, quote_ata,
                    )],
                    min_out,
                    quote_ata,
                ))
            }

            // —— Receive SOL/WSOL via bridge ——
            (SellTo::Sol | SellTo::Wsol, Market::LaunchLabInner(pool)) => {
                let bridge = market
                    .bridge
                    .as_ref()
                    .ok_or_else(|| anyhow!("missing SOL↔stock bridge"))?;
                self.sell_via_bridge_launchlab(sell_amt, pool, bridge, slip, setup, touched, &opts.ata)
            }

            (SellTo::Sol | SellTo::Wsol, Market::CpmmOuter(pool))
                if !market.market.needs_sol_bridge() =>
            {
                self.sell_cpmm_to_mint(
                    sell_amt,
                    pool,
                    pool.meme_mint(),
                    WSOL_MINT,
                    slip,
                    setup,
                    touched,
                    &opts.ata,
                )
            }

            (SellTo::Sol | SellTo::Wsol, Market::CpmmOuter(pool)) => {
                let bridge = market
                    .bridge
                    .as_ref()
                    .ok_or_else(|| anyhow!("missing SOL↔stock bridge"))?;
                self.sell_via_bridge_cpmm(sell_amt, pool, bridge, slip, setup, touched, &opts.ata)
            }

            _ => Err(anyhow!("unsupported sell path for this sell_to / market")),
        }
    }

    fn buy_launchlab_with_quote(
        &self,
        spend: u64,
        pool: &LaunchLabPool,
        pay_mint: Pubkey,
        slip: u64,
        meme_ata: Pubkey,
        setup: &mut Vec<Instruction>,
        touched: &mut TouchedAtas,
        policy: &AtaPolicy,
    ) -> Result<(Vec<Leg>, u64)> {
        if pay_mint != pool.quote_mint {
            return Err(anyhow!("LaunchLab pay mint must be pool quote"));
        }
        let payer = self.payer;
        let quote_ata = ata(&payer, &pay_mint, &pool.quote_token_program);
        let kind = if pay_mint == WSOL_MINT {
            touched.touch_wsol();
            AtaKind::Wsol
        } else {
            touched.touch_quote(pay_mint, pool.quote_token_program);
            AtaKind::Quote
        };
        push_create_ata(setup, policy, kind, &payer, &pay_mint, &pool.quote_token_program);
        let q = launchlab_buy_quote(pool, spend, 0)?;
        let min_out = apply_slippage_min_out(q.amount_out, slip);
        Ok((
            vec![launchlab_buy_leg(
                &payer, pool, q.amount_in, min_out, meme_ata, quote_ata,
            )],
            min_out,
        ))
    }

    fn buy_cpmm_with_mint(
        &self,
        spend: u64,
        pool: &CpmmPool,
        input_mint: Pubkey,
        output_mint: Pubkey,
        slip: u64,
        setup: &mut Vec<Instruction>,
        touched: &mut TouchedAtas,
        policy: &AtaPolicy,
    ) -> Result<(Vec<Leg>, u64)> {
        let payer = self.payer;
        let input_is_base = input_mint == pool.base_mint;
        let input_tp = if input_is_base {
            pool.base_token_program
        } else {
            pool.quote_token_program
        };
        let output_tp = if output_mint == pool.base_mint {
            pool.base_token_program
        } else {
            pool.quote_token_program
        };
        let user_in = ata(&payer, &input_mint, &input_tp);
        let user_out = ata(&payer, &output_mint, &output_tp);
        // Buy path: output is meme (already touched in buy_with_opts).
        if output_mint == WSOL_MINT {
            touched.touch_wsol();
            push_create_ata(setup, policy, AtaKind::Wsol, &payer, &output_mint, &output_tp);
        } else {
            touched.touch_meme(output_mint, output_tp);
            push_create_ata(setup, policy, AtaKind::Meme, &payer, &output_mint, &output_tp);
        }
        let expected = cpmm_out(pool, spend, input_is_base)?;
        let min_out = apply_slippage_min_out(expected, slip);
        let leg = cpmm_swap_leg(
            &payer, pool, spend, min_out, input_mint, output_mint, user_in, user_out,
        )?;
        Ok((vec![leg], min_out))
    }

    fn sell_launchlab_to_quote(
        &self,
        sell_amt: u64,
        pool: &LaunchLabPool,
        out_mint: Pubkey,
        slip: u64,
        meme_ata: Pubkey,
        setup: &mut Vec<Instruction>,
        touched: &mut TouchedAtas,
        policy: &AtaPolicy,
    ) -> Result<(Vec<Leg>, u64, Pubkey)> {
        if out_mint != pool.quote_mint {
            return Err(anyhow!("LaunchLab receive mint must be pool quote"));
        }
        let payer = self.payer;
        let quote_ata = ata(&payer, &out_mint, &pool.quote_token_program);
        let kind = if out_mint == WSOL_MINT {
            touched.touch_wsol();
            AtaKind::Wsol
        } else {
            touched.touch_quote(out_mint, pool.quote_token_program);
            AtaKind::Quote
        };
        push_create_ata(setup, policy, kind, &payer, &out_mint, &pool.quote_token_program);
        let expected = launchlab_sell_quote_out(pool, sell_amt)?;
        let min_out = apply_slippage_min_out(expected, slip);
        Ok((
            vec![launchlab_sell_leg(
                &payer, pool, sell_amt, min_out, meme_ata, quote_ata,
            )],
            min_out,
            quote_ata,
        ))
    }

    fn sell_cpmm_to_mint(
        &self,
        sell_amt: u64,
        pool: &CpmmPool,
        input_mint: Pubkey,
        output_mint: Pubkey,
        slip: u64,
        setup: &mut Vec<Instruction>,
        touched: &mut TouchedAtas,
        policy: &AtaPolicy,
    ) -> Result<(Vec<Leg>, u64, Pubkey)> {
        let payer = self.payer;
        let input_is_base = input_mint == pool.base_mint;
        let input_tp = if input_is_base {
            pool.base_token_program
        } else {
            pool.quote_token_program
        };
        let output_tp = if output_mint == pool.base_mint {
            pool.base_token_program
        } else {
            pool.quote_token_program
        };
        let user_in = ata(&payer, &input_mint, &input_tp);
        let user_out = ata(&payer, &output_mint, &output_tp);
        let kind = if output_mint == WSOL_MINT {
            touched.touch_wsol();
            AtaKind::Wsol
        } else {
            touched.touch_quote(output_mint, output_tp);
            AtaKind::Quote
        };
        push_create_ata(setup, policy, kind, &payer, &output_mint, &output_tp);
        let expected = cpmm_out(pool, sell_amt, input_is_base)?;
        let min_out = apply_slippage_min_out(expected, slip);
        let leg = cpmm_swap_leg(
            &payer, pool, sell_amt, min_out, input_mint, output_mint, user_in, user_out,
        )?;
        Ok((vec![leg], min_out, user_out))
    }

    fn buy_via_bridge_launchlab(
        &self,
        spend: u64,
        pool: &LaunchLabPool,
        bridge: &CpmmPool,
        slippage_bps: u64,
        setup: &mut Vec<Instruction>,
        touched: &mut TouchedAtas,
        policy: &AtaPolicy,
    ) -> Result<(Vec<Leg>, u64)> {
        let payer = self.payer;
        let stock = pool.quote_mint;
        let stock_tp = pool.quote_token_program;
        let stock_ata = ata(&payer, &stock, &stock_tp);
        let wsol_ata = ata(&payer, &WSOL_MINT, &TOKEN_PROGRAM);
        let meme_ata = ata(&payer, &pool.base_mint, &pool.base_token_program);
        if stock == WSOL_MINT {
            touched.touch_wsol();
            push_create_ata(setup, policy, AtaKind::Wsol, &payer, &stock, &stock_tp);
        } else {
            touched.touch_quote(stock, stock_tp);
            push_create_ata(setup, policy, AtaKind::Quote, &payer, &stock, &stock_tp);
        }

        let input_is_base = bridge.base_mint == WSOL_MINT;
        let stock_out = cpmm_out(bridge, spend, input_is_base)?;
        let min_stock = apply_slippage_min_out(stock_out, slippage_bps / 2);
        let leg1 = cpmm_swap_leg(
            &payer,
            bridge,
            spend,
            min_stock,
            WSOL_MINT,
            stock,
            wsol_ata,
            stock_ata,
        )?;
        let meme_out = launchlab_buy_quote(pool, min_stock, 0)?.amount_out;
        let min_meme = apply_slippage_min_out(meme_out, slippage_bps / 2);
        let leg2 = launchlab_buy_leg(&payer, pool, min_stock, min_meme, meme_ata, stock_ata);
        Ok((vec![leg1, leg2], min_meme))
    }

    fn buy_via_bridge_cpmm(
        &self,
        spend: u64,
        pool: &CpmmPool,
        bridge: &CpmmPool,
        slippage_bps: u64,
        setup: &mut Vec<Instruction>,
        touched: &mut TouchedAtas,
        policy: &AtaPolicy,
    ) -> Result<(Vec<Leg>, u64)> {
        let payer = self.payer;
        let stock = resolve_shared_mint(pool, bridge)?;
        let meme = other_mint(pool, stock);
        let stock_tp = token_program_for(pool, stock);
        let meme_tp = token_program_for(pool, meme);
        let stock_ata = ata(&payer, &stock, &stock_tp);
        let meme_ata = ata(&payer, &meme, &meme_tp);
        let wsol_ata = ata(&payer, &WSOL_MINT, &TOKEN_PROGRAM);
        if stock == WSOL_MINT {
            touched.touch_wsol();
            push_create_ata(setup, policy, AtaKind::Wsol, &payer, &stock, &stock_tp);
        } else {
            touched.touch_quote(stock, stock_tp);
            push_create_ata(setup, policy, AtaKind::Quote, &payer, &stock, &stock_tp);
        }
        // Meme create is handled in buy_with_opts (AtaKind::Meme).
        touched.touch_meme(meme, meme_tp);

        let input_is_base = bridge.base_mint == WSOL_MINT;
        let stock_out = cpmm_out(bridge, spend, input_is_base)?;
        let min_stock = apply_slippage_min_out(stock_out, slippage_bps / 2);
        let leg1 = cpmm_swap_leg(
            &payer,
            bridge,
            spend,
            min_stock,
            WSOL_MINT,
            stock,
            wsol_ata,
            stock_ata,
        )?;
        let input_is_base2 = stock == pool.base_mint;
        let meme_out = cpmm_out(pool, min_stock, input_is_base2)?;
        let min_meme = apply_slippage_min_out(meme_out, slippage_bps / 2);
        let leg2 = cpmm_swap_leg(
            &payer, pool, min_stock, min_meme, stock, meme, stock_ata, meme_ata,
        )?;
        Ok((vec![leg1, leg2], min_meme))
    }

    fn sell_via_bridge_launchlab(
        &self,
        sell_amt: u64,
        pool: &LaunchLabPool,
        bridge: &CpmmPool,
        slippage_bps: u64,
        setup: &mut Vec<Instruction>,
        touched: &mut TouchedAtas,
        policy: &AtaPolicy,
    ) -> Result<(Vec<Leg>, u64, Pubkey)> {
        let payer = self.payer;
        let stock = pool.quote_mint;
        let stock_ata = ata(&payer, &stock, &pool.quote_token_program);
        let meme_ata = ata(&payer, &pool.base_mint, &pool.base_token_program);
        let wsol_ata = ata(&payer, &WSOL_MINT, &TOKEN_PROGRAM);
        if stock == WSOL_MINT {
            touched.touch_wsol();
            push_create_ata(
                setup,
                policy,
                AtaKind::Wsol,
                &payer,
                &stock,
                &pool.quote_token_program,
            );
        } else {
            touched.touch_quote(stock, pool.quote_token_program);
            push_create_ata(
                setup,
                policy,
                AtaKind::Quote,
                &payer,
                &stock,
                &pool.quote_token_program,
            );
        }

        let stock_out = launchlab_sell_quote_out(pool, sell_amt)?;
        let min_stock = apply_slippage_min_out(stock_out, slippage_bps / 2);
        let leg1 = launchlab_sell_leg(&payer, pool, sell_amt, min_stock, meme_ata, stock_ata);
        let input_is_base = stock == bridge.base_mint;
        let sol_out = cpmm_out(bridge, min_stock, input_is_base)?;
        let min_sol = apply_slippage_min_out(sol_out, slippage_bps / 2);
        let leg2 = cpmm_swap_leg(
            &payer,
            bridge,
            min_stock,
            min_sol,
            stock,
            WSOL_MINT,
            stock_ata,
            wsol_ata,
        )?;
        Ok((vec![leg1, leg2], min_sol, wsol_ata))
    }

    fn sell_via_bridge_cpmm(
        &self,
        sell_amt: u64,
        pool: &CpmmPool,
        bridge: &CpmmPool,
        slippage_bps: u64,
        setup: &mut Vec<Instruction>,
        touched: &mut TouchedAtas,
        policy: &AtaPolicy,
    ) -> Result<(Vec<Leg>, u64, Pubkey)> {
        let payer = self.payer;
        let stock = resolve_shared_mint(pool, bridge)?;
        let meme = other_mint(pool, stock);
        let stock_tp = token_program_for(pool, stock);
        let meme_tp = token_program_for(pool, meme);
        let stock_ata = ata(&payer, &stock, &stock_tp);
        let meme_ata = ata(&payer, &meme, &meme_tp);
        let wsol_ata = ata(&payer, &WSOL_MINT, &TOKEN_PROGRAM);
        if stock == WSOL_MINT {
            touched.touch_wsol();
            push_create_ata(setup, policy, AtaKind::Wsol, &payer, &stock, &stock_tp);
        } else {
            touched.touch_quote(stock, stock_tp);
            push_create_ata(setup, policy, AtaKind::Quote, &payer, &stock, &stock_tp);
        }

        let input_is_base = meme == pool.base_mint;
        let stock_out = cpmm_out(pool, sell_amt, input_is_base)?;
        let min_stock = apply_slippage_min_out(stock_out, slippage_bps / 2);
        let leg1 = cpmm_swap_leg(
            &payer, pool, sell_amt, min_stock, meme, stock, meme_ata, stock_ata,
        )?;
        let input_is_base2 = stock == bridge.base_mint;
        let sol_out = cpmm_out(bridge, min_stock, input_is_base2)?;
        let min_sol = apply_slippage_min_out(sol_out, slippage_bps / 2);
        let leg2 = cpmm_swap_leg(
            &payer,
            bridge,
            min_stock,
            min_sol,
            stock,
            WSOL_MINT,
            stock_ata,
            wsol_ata,
        )?;
        Ok((vec![leg1, leg2], min_sol, wsol_ata))
    }
}

fn resolve_shared_mint(pool: &CpmmPool, bridge: &CpmmPool) -> Result<Pubkey> {
    let b = [bridge.base_mint, bridge.quote_mint];
    if b.contains(&pool.quote_mint) {
        Ok(pool.quote_mint)
    } else if b.contains(&pool.base_mint) {
        Ok(pool.base_mint)
    } else {
        Err(anyhow!("bridge and market pool share no mint"))
    }
}

fn other_mint(pool: &CpmmPool, mint: Pubkey) -> Pubkey {
    if mint == pool.base_mint {
        pool.quote_mint
    } else {
        pool.base_mint
    }
}

fn token_program_for(pool: &CpmmPool, mint: Pubkey) -> Pubkey {
    if mint == pool.base_mint {
        pool.base_token_program
    } else {
        pool.quote_token_program
    }
}
