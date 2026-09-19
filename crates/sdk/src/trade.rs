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
    constants::{PROGRAM_ID, SYSTEM_PROGRAM, TOKEN_PROGRAM, WSOL_MINT},
    legs::{
        cpmm_swap_leg, launchlab_buy_leg, launchlab_sell_leg, meteora_damm_v2_swap_leg,
        meteora_dlmm_swap_leg, pumpfun_buy_leg, pumpfun_buy_v2_leg, pumpfun_sell_leg,
        pumpfun_sell_v2_leg, pumpswap_buy_leg, pumpswap_sell_leg, raydium_amm_v4_swap_leg,
        raydium_clmm_swap_leg, whirlpool_swap_leg, Leg,
    },
    market::{
        CpmmPool, LaunchLabPool, Market, MeteoraDammV2Pool, MeteoraDlmmPool, PumpSwapPool,
        RaydiumAmmV4Pool, RaydiumClmmPool, RoutedMarket, WhirlpoolPool,
    },
    pool_guard::{assert_routed_market_ok, PoolGuardPolicy},
    quote::{
        apply_slippage_min_out, cpmm_in_for_out, cpmm_out, fee_amount, launchlab_buy_quote,
        launchlab_sell_quote_out, meteora_damm_v2_out, pumpfun_buy_token_out, pumpfun_sell_sol_out,
        pumpswap_buy_base_out, pumpswap_sell_quote_out, raydium_amm_v4_out,
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
    /// Explicit DEX minimum output, primarily for complex Meteora DAMM V2 curves.
    pub min_out: Option<u64>,
    /// Exact-out target. When set, `amount_in` on buy/sell is the **max input budget**
    /// and capable venues use exact-out legs (CPMM / AmmV4 / PumpSwap / DAMM V2).
    pub fixed_output: Option<u64>,
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
            min_out: None,
            fixed_output: None,
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
    pub fn with_min_out(mut self, min_out: u64) -> Self {
        self.min_out = Some(min_out);
        self
    }
    pub fn with_fixed_output(mut self, amount_out: u64) -> Self {
        self.fixed_output = Some(amount_out);
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
    /// Mismatch with `cfg.fee_bps` causes `FeeSourceMismatch` on-chain.
    /// Must match on-chain `RouterConfig.fee_bps`. Client computes spend /
    /// `route_amount_in` with this value; the program charges `cfg.fee_bps`.
    pub fee_bps: u16,
    /// Reject wild / non-canonical pools before building legs.
    pub pool_guard: PoolGuardPolicy,
}

impl RouterClient {
    pub fn new(payer: Pubkey, fee_recipient: Pubkey, fee_bps: u16) -> Self {
        Self {
            payer,
            program_id: PROGRAM_ID,
            fee_recipient,
            fee_bps,
            pool_guard: PoolGuardPolicy::default(),
        }
    }

    pub fn with_program_id(mut self, program_id: Pubkey) -> Self {
        self.program_id = program_id;
        self
    }

    pub fn with_pool_guard(mut self, pool_guard: PoolGuardPolicy) -> Self {
        self.pool_guard = pool_guard;
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
        self.create_ata(
            market.market.quote_mint(),
            market.market.quote_token_program(),
        )
    }

    pub fn close_quote_ata(&self, market: &RoutedMarket) -> Instruction {
        self.close_ata(
            market.market.quote_mint(),
            market.market.quote_token_program(),
        )
    }

    /// Cold-path: prepare **reusable** ATAs for buys (WSOL + stock/quote + fee recipient).
    /// Does **not** create meme ATA — that belongs in the same buy tx (`create_meme`).
    pub fn prepare_buy_atas(&self, market: &RoutedMarket, buy_with: BuyWith) -> Vec<Instruction> {
        let mut ixs = Vec::new();
        match buy_with {
            BuyWith::Sol | BuyWith::Wsol => {
                // PumpFun WSOL-quote pools settle in native SOL (V1 and V2).
                if !matches!(&market.market, Market::PumpFunInner(p) if p.is_native_sol_quote()) {
                    ixs.push(self.create_wsol_ata());
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
                if !matches!(&market.market, Market::PumpFunInner(p) if p.is_native_sol_quote()) {
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
        assert_routed_market_ok(market, &self.pool_guard)?;
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

        let is_pump_native_sol =
            matches!(&market.market, Market::PumpFunInner(pool) if pool.is_native_sol_quote());
        let pump_wsol_ata = matches!(
            &market.market,
            Market::PumpFunInner(pool) if pool.uses_wsol_ata_settlement()
        );
        if is_pump_native_sol {
            match opts.buy_with {
                BuyWith::Sol => {}
                BuyWith::Wsol if pump_wsol_ata => {}
                BuyWith::Wsol => {
                    return Err(anyhow!(
                        "PumpFun WSOL-quote native settlement uses BuyWith::Sol; set use_v2 for WSOL ATA"
                    ));
                }
                BuyWith::Token(_) => {
                    return Err(anyhow!(
                        "PumpFun WSOL-quote buy does not accept stock/quote token"
                    ));
                }
            }
        }

        // Non-PumpFun SOL/WSOL: prepare WSOL ATA; wrap after legs so LaunchLab
        // graduation clamps can shrink route amount_in without leaving stranded WSOL.
        // PumpFun WSOL-ATA path (use_v2) also needs the quote ATA.
        let will_use_wsol = match (&opts.buy_with, is_pump_native_sol) {
            (BuyWith::Sol, false) | (BuyWith::Wsol, _) => true,
            _ => false,
        };
        if will_use_wsol {
            touched.touch_wsol();
            push_create_ata(
                &mut setup,
                &opts.ata,
                AtaKind::Wsol,
                &payer,
                &WSOL_MINT,
                &TOKEN_PROGRAM,
            );
        }

        let (legs, min_out, swap_spent) =
            self.build_buy_legs(spend, market, &opts, &mut setup, &mut touched)?;
        let route_amount_in = if swap_spent == spend {
            amount_in
        } else {
            route_amount_in_for_spend(swap_spent, self.fee_bps)
        };

        if matches!(opts.buy_with, BuyWith::Sol) && !is_pump_native_sol {
            setup.extend(wrap_sol_with_options(
                &payer,
                route_amount_in,
                false, // ATA already created above when policy allows
            ));
        }

        // Fee asset: PumpFun native-SOL (BuyWith::Sol) stays lamports; WSOL ATA
        // settlement and other SOL/WSOL fees come from WSOL.
        let (fee_asset, fee_destination, fee_source, fee_program, fee_mint) = match opts.buy_with {
            BuyWith::Sol if is_pump_native_sol => (
                FEE_ASSET_SOL,
                self.fee_recipient,
                payer,
                sol_fee_program(),
                sol_fee_program(), // placeholder mint account (System Program)
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
                (FEE_ASSET_TOKEN, fee_dst, fee_src, TOKEN_PROGRAM, WSOL_MINT)
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
                    setup.push(create_ata_idempotent(
                        &payer,
                        &self.fee_recipient,
                        &mint,
                        &tp,
                    ));
                }
                push_create_ata(&mut setup, &opts.ata, kind, &payer, &mint, &tp);
                (
                    FEE_ASSET_TOKEN,
                    fee_dst,
                    fee_src,
                    token_fee_program(&tp),
                    mint,
                )
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
                fee_mint,
            },
            route_amount_in,
            min_out,
            fee_asset,
            &meme,
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
        self.sell_with_opts(amount_in, market, TradeOpts::default().sell_to_token(mint))
    }

    /// Generic sell with explicit [`TradeOpts`].
    pub fn sell_with_opts(
        &self,
        amount_in: u64,
        market: &RoutedMarket,
        opts: TradeOpts,
    ) -> Result<BuiltTrade> {
        assert_routed_market_ok(market, &self.pool_guard)?;
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

        let is_pump_native_sol =
            matches!(&market.market, Market::PumpFunInner(pool) if pool.is_native_sol_quote());
        let pump_wsol_ata = matches!(
            &market.market,
            Market::PumpFunInner(pool) if pool.uses_wsol_ata_settlement()
        );
        if is_pump_native_sol {
            match opts.sell_to {
                SellTo::Sol => {}
                SellTo::Wsol if pump_wsol_ata => {}
                SellTo::Wsol => {
                    return Err(anyhow!(
                        "PumpFun WSOL-quote native settlement uses SellTo::Sol; set use_v2 for WSOL ATA"
                    ));
                }
                SellTo::Token(_) => {
                    return Err(anyhow!("PumpFun WSOL-quote sell only returns SOL/WSOL"));
                }
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
        if (opts.sell_to.is_sol_family() && !is_pump_native_sol)
            || matches!(opts.sell_to, SellTo::Wsol if pump_wsol_ata)
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

        let (expected_output_mint, output_token_account) = match (&opts.sell_to, &market.market) {
            // Native SOL credit — router checks payer lamport Δ.
            (SellTo::Sol, Market::PumpFunInner(pool)) if pool.is_native_sol_quote() => {
                (SYSTEM_PROGRAM, payer)
            }
            (SellTo::Wsol, Market::PumpFunInner(pool))
                if pool.uses_wsol_ata_settlement() =>
            {
                (WSOL_MINT, output_ata)
            }
            (SellTo::Sol | SellTo::Wsol, _) => (WSOL_MINT, output_ata),
            (SellTo::Token(m), _) => (*m, output_ata),
        };

        let route = build_route_instruction(
            &self.program_id,
            RouteAccounts {
                payer,
                fee_destination: fee_ata,
                fee_source: meme_ata,
                output_token_account,
                fee_program: token_fee_program(&meme_tp),
                fee_mint: meme,
            },
            amount_in,
            min_out,
            FEE_ASSET_TOKEN,
            &expected_output_mint,
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
    ) -> Result<(Vec<Leg>, u64, u64)> {
        let payer = self.payer;
        let meme = market.meme_mint();
        let meme_tp = market.meme_token_program();
        let meme_ata = ata(&payer, &meme, &meme_tp);
        let slip = opts.slippage_bps;

        match (&opts.buy_with, &market.market) {
            // —— PumpFun WSOL-quote: native SOL always uses V1 layout (sol-trade-sdk) ——
            (BuyWith::Sol, Market::PumpFunInner(pool)) if pool.is_native_sol_quote() => {
                let expected = pumpfun_buy_token_out(pool, spend);
                let min_out = opts
                    .min_out
                    .unwrap_or_else(|| apply_slippage_min_out(expected, slip));
                Ok((
                    vec![pumpfun_buy_leg(&payer, pool, spend, min_out, meme_ata)],
                    min_out,
                    spend,
                ))
            }
            // —— PumpFun WSOL-quote + use_v2: settle via existing WSOL ATA ——
            (BuyWith::Wsol, Market::PumpFunInner(pool)) if pool.uses_wsol_ata_settlement() => {
                let expected = pumpfun_buy_token_out(pool, spend);
                let min_out = opts
                    .min_out
                    .unwrap_or_else(|| apply_slippage_min_out(expected, slip));
                Ok((
                    vec![pumpfun_buy_v2_leg(&payer, pool, spend, min_out)],
                    min_out,
                    spend,
                ))
            }
            (BuyWith::Token(pay_mint), Market::PumpFunInner(pool)) if pool.uses_v2() => {
                if *pay_mint != pool.quote_mint {
                    return Err(anyhow!("PumpFun V2 pay mint must be pool quote"));
                }
                if pool.is_native_sol_quote() {
                    return Err(anyhow!(
                        "PumpFun WSOL-quote: use BuyWith::Sol (native) or BuyWith::Wsol (use_v2)"
                    ));
                }
                let expected = pumpfun_buy_token_out(pool, spend);
                let min_out = opts
                    .min_out
                    .unwrap_or_else(|| apply_slippage_min_out(expected, slip));
                Ok((
                    vec![pumpfun_buy_v2_leg(&payer, pool, spend, min_out)],
                    min_out,
                    spend,
                ))
            }

            // —— Pay stock/quote directly (single hop) ——
            (BuyWith::Token(pay_mint), Market::LaunchLabInner(pool)) => self
                .buy_launchlab_with_quote(
                    spend, pool, *pay_mint, slip, opts.min_out, meme_ata, setup, touched,
                    &opts.ata,
                ),
            (BuyWith::Token(pay_mint), Market::CpmmOuter(pool)) => self
                .buy_cpmm_with_mint(
                    spend, pool, *pay_mint, meme, slip, opts.min_out, setup, touched,
                    &opts.ata,
                )
                .map(|(legs, min)| (legs, min, spend)),
            (BuyWith::Token(pay_mint), Market::PumpSwapOuter(pool)) => {
                let tp = pool.quote_token_program;
                let kind = if *pay_mint == WSOL_MINT {
                    touched.touch_wsol();
                    AtaKind::Wsol
                } else {
                    touched.touch_quote(*pay_mint, tp);
                    AtaKind::Quote
                };
                push_create_ata(setup, &opts.ata, kind, &payer, pay_mint, &tp);
                self.buy_pumpswap_with_quote(spend, pool, *pay_mint, slip, opts.min_out)
                    .map(|(legs, min)| (legs, min, spend))
            }
            (BuyWith::Token(pay_mint), Market::RaydiumAmmV4(pool)) => {
                let kind = if *pay_mint == WSOL_MINT {
                    touched.touch_wsol();
                    AtaKind::Wsol
                } else {
                    touched.touch_quote(*pay_mint, TOKEN_PROGRAM);
                    AtaKind::Quote
                };
                push_create_ata(setup, &opts.ata, kind, &payer, pay_mint, &TOKEN_PROGRAM);
                self.buy_raydium_v4_with_mint(spend, pool, *pay_mint, slip, opts.min_out)
                    .map(|(legs, min)| (legs, min, spend))
            }
            (BuyWith::Token(pay_mint), Market::MeteoraDammV2(pool)) => {
                let tp = if *pay_mint == pool.token_a_mint {
                    pool.token_a_program
                } else {
                    pool.token_b_program
                };
                let kind = if *pay_mint == WSOL_MINT {
                    touched.touch_wsol();
                    AtaKind::Wsol
                } else {
                    touched.touch_quote(*pay_mint, tp);
                    AtaKind::Quote
                };
                push_create_ata(setup, &opts.ata, kind, &payer, pay_mint, &tp);
                self.buy_meteora_v2_with_mint(spend, pool, *pay_mint, slip, opts.min_out)
                    .map(|(legs, min)| (legs, min, spend))
            }
            (BuyWith::Token(pay_mint), Market::RaydiumClmm(pool)) => {
                let tp = if *pay_mint == pool.token_0_mint {
                    pool.token_0_program
                } else {
                    pool.token_1_program
                };
                let kind = if *pay_mint == WSOL_MINT {
                    touched.touch_wsol();
                    AtaKind::Wsol
                } else {
                    touched.touch_quote(*pay_mint, tp);
                    AtaKind::Quote
                };
                push_create_ata(setup, &opts.ata, kind, &payer, pay_mint, &tp);
                self.buy_raydium_clmm_with_mint(spend, pool, *pay_mint, slip, opts.min_out)
                    .map(|(legs, min)| (legs, min, spend))
            }
            (BuyWith::Token(pay_mint), Market::Whirlpool(pool)) => {
                let tp = if *pay_mint == pool.mint_a {
                    pool.token_program_a
                } else {
                    pool.token_program_b
                };
                let kind = if *pay_mint == WSOL_MINT {
                    touched.touch_wsol();
                    AtaKind::Wsol
                } else {
                    touched.touch_quote(*pay_mint, tp);
                    AtaKind::Quote
                };
                push_create_ata(setup, &opts.ata, kind, &payer, pay_mint, &tp);
                self.buy_whirlpool_with_mint(spend, pool, *pay_mint, slip, opts.min_out)
                    .map(|(legs, min)| (legs, min, spend))
            }
            (BuyWith::Token(pay_mint), Market::MeteoraDlmm(pool)) => {
                let tp = if *pay_mint == pool.token_x_mint {
                    pool.token_x_program
                } else {
                    pool.token_y_program
                };
                let kind = if *pay_mint == WSOL_MINT {
                    touched.touch_wsol();
                    AtaKind::Wsol
                } else {
                    touched.touch_quote(*pay_mint, tp);
                    AtaKind::Quote
                };
                push_create_ata(setup, &opts.ata, kind, &payer, pay_mint, &tp);
                self.buy_meteora_dlmm_with_mint(spend, pool, *pay_mint, slip, opts.min_out)
                    .map(|(legs, min)| (legs, min, spend))
            }

            // —— Pay SOL/WSOL, LaunchLab SOL-quoted ——
            (BuyWith::Sol | BuyWith::Wsol, Market::LaunchLabInner(pool)) if pool.is_sol_quote() => {
                let quote_ata = ata(&payer, &WSOL_MINT, &pool.quote_token_program);
                let q = launchlab_buy_quote(pool, spend, 0)?;
                let min_out = opts
                    .min_out
                    .unwrap_or_else(|| apply_slippage_min_out(q.amount_out, slip));
                Ok((
                    vec![launchlab_buy_leg(
                        &payer,
                        pool,
                        q.amount_in,
                        min_out,
                        meme_ata,
                        quote_ata,
                    )],
                    min_out,
                    q.amount_in,
                ))
            }
            // PumpFun V2 WSOL-ATA settlement (explicit use_v2 + BuyWith::Wsol) handled above.
            // Do NOT send V2 CPI with BuyWith::Sol — that mismatches fee_source (lamports vs WSOL).

            // PumpFun V2 non-WSOL quote (e.g. USDC): SOL → quote bridge → V2 buy.
            (BuyWith::Sol | BuyWith::Wsol, Market::PumpFunInner(pool))
                if pool.uses_v2() && !pool.is_native_sol_quote() =>
            {
                let bridge = market
                    .bridge
                    .as_ref()
                    .ok_or_else(|| anyhow!("PumpFun V2 non-WSOL quote needs SOL↔quote bridge"))?;
                self.buy_via_bridge_mainstream(
                    spend,
                    &market.market,
                    bridge,
                    slip,
                    opts.min_out,
                    setup,
                    touched,
                    &opts.ata,
                )
                .map(|(legs, min)| (legs, min, spend))
            }

            // —— Pay SOL/WSOL, LaunchLab stock-quoted → bridge ——
            (BuyWith::Sol | BuyWith::Wsol, Market::LaunchLabInner(pool)) => {
                let bridge = market
                    .bridge
                    .as_ref()
                    .ok_or_else(|| anyhow!("missing SOL↔stock bridge"))?;
                self.buy_via_bridge_launchlab(
                    spend, pool, bridge, slip, opts.min_out, setup, touched, &opts.ata,
                )
            }

            // —— Pay SOL/WSOL, CPMM with WSOL side ——
            (BuyWith::Sol | BuyWith::Wsol, Market::CpmmOuter(pool))
                if !market.market.needs_sol_bridge() =>
            {
                let input_mint = WSOL_MINT;
                let output_mint = pool.meme_mint();
                self.buy_cpmm_with_mint(
                    spend,
                    pool,
                    input_mint,
                    output_mint,
                    slip,
                    opts.min_out,
                    setup,
                    touched,
                    &opts.ata,
                )
                .map(|(legs, min)| (legs, min, spend))
            }
            (BuyWith::Sol | BuyWith::Wsol, Market::PumpSwapOuter(pool))
                if pool.quote_mint == WSOL_MINT =>
            {
                self.buy_pumpswap_with_quote(spend, pool, WSOL_MINT, slip, opts.min_out)
                    .map(|(legs, min)| (legs, min, spend))
            }
            (BuyWith::Sol | BuyWith::Wsol, Market::RaydiumAmmV4(pool))
                if !market.market.needs_sol_bridge() =>
            {
                self.buy_raydium_v4_with_mint(spend, pool, WSOL_MINT, slip, opts.min_out)
                    .map(|(legs, min)| (legs, min, spend))
            }
            (BuyWith::Sol | BuyWith::Wsol, Market::MeteoraDammV2(pool))
                if !market.market.needs_sol_bridge() =>
            {
                self.buy_meteora_v2_with_mint(spend, pool, WSOL_MINT, slip, opts.min_out)
                    .map(|(legs, min)| (legs, min, spend))
            }
            (BuyWith::Sol | BuyWith::Wsol, Market::RaydiumClmm(pool))
                if !market.market.needs_sol_bridge() =>
            {
                self.buy_raydium_clmm_with_mint(spend, pool, WSOL_MINT, slip, opts.min_out)
                    .map(|(legs, min)| (legs, min, spend))
            }
            (BuyWith::Sol | BuyWith::Wsol, Market::Whirlpool(pool))
                if !market.market.needs_sol_bridge() =>
            {
                self.buy_whirlpool_with_mint(spend, pool, WSOL_MINT, slip, opts.min_out)
                    .map(|(legs, min)| (legs, min, spend))
            }
            (BuyWith::Sol | BuyWith::Wsol, Market::MeteoraDlmm(pool))
                if !market.market.needs_sol_bridge() =>
            {
                self.buy_meteora_dlmm_with_mint(spend, pool, WSOL_MINT, slip, opts.min_out)
                    .map(|(legs, min)| (legs, min, spend))
            }
            (BuyWith::Sol | BuyWith::Wsol, target @ Market::PumpSwapOuter(_))
            | (BuyWith::Sol | BuyWith::Wsol, target @ Market::RaydiumAmmV4(_))
            | (BuyWith::Sol | BuyWith::Wsol, target @ Market::MeteoraDammV2(_))
            | (BuyWith::Sol | BuyWith::Wsol, target @ Market::RaydiumClmm(_))
            | (BuyWith::Sol | BuyWith::Wsol, target @ Market::Whirlpool(_))
            | (BuyWith::Sol | BuyWith::Wsol, target @ Market::MeteoraDlmm(_)) => {
                let bridge = market
                    .bridge
                    .as_ref()
                    .ok_or_else(|| anyhow!("missing SOL↔quote bridge"))?;
                self.buy_via_bridge_mainstream(
                    spend,
                    target,
                    bridge,
                    slip,
                    opts.min_out,
                    setup,
                    touched,
                    &opts.ata,
                )
                .map(|(legs, min)| (legs, min, spend))
            }

            // —— Pay SOL/WSOL, CPMM stock/meme → bridge ——
            (BuyWith::Sol | BuyWith::Wsol, Market::CpmmOuter(pool)) => {
                let bridge = market
                    .bridge
                    .as_ref()
                    .ok_or_else(|| anyhow!("missing SOL↔stock bridge"))?;
                self.buy_via_bridge_cpmm(
                    spend, pool, bridge, slip, opts.min_out, setup, touched, &opts.ata,
                )
                    .map(|(legs, min)| (legs, min, spend))
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
            // —— PumpFun WSOL-quote: native SOL always V1 ——
            (SellTo::Sol, Market::PumpFunInner(pool)) if pool.is_native_sol_quote() => {
                let expected = pumpfun_sell_sol_out(pool, sell_amt);
                let min_out = opts
                    .min_out
                    .unwrap_or_else(|| apply_slippage_min_out(expected, slip));
                Ok((
                    vec![pumpfun_sell_leg(&payer, pool, sell_amt, min_out, meme_ata)],
                    min_out,
                    meme_ata, // unused — sell_with_opts swaps to payer for native SOL
                ))
            }
            // —— PumpFun WSOL-quote + use_v2: credit WSOL ATA ——
            (SellTo::Wsol, Market::PumpFunInner(pool)) if pool.uses_wsol_ata_settlement() => {
                let expected = pumpfun_sell_sol_out(pool, sell_amt);
                let min_out = opts
                    .min_out
                    .unwrap_or_else(|| apply_slippage_min_out(expected, slip));
                let wsol_ata = ata(&payer, &WSOL_MINT, &TOKEN_PROGRAM);
                Ok((
                    vec![pumpfun_sell_v2_leg(&payer, pool, sell_amt, min_out)],
                    min_out,
                    wsol_ata,
                ))
            }
            (SellTo::Token(out_mint), Market::PumpFunInner(pool)) if pool.uses_v2() => {
                if pool.is_native_sol_quote() {
                    return Err(anyhow!(
                        "PumpFun WSOL-quote: use SellTo::Sol (native) or SellTo::Wsol (use_v2)"
                    ));
                }
                if *out_mint != pool.quote_mint {
                    return Err(anyhow!("PumpFun V2 receive mint must be pool quote"));
                }
                let expected = pumpfun_sell_sol_out(pool, sell_amt);
                let min_out = opts
                    .min_out
                    .unwrap_or_else(|| apply_slippage_min_out(expected, slip));
                let quote_ata = ata(&payer, out_mint, &pool.quote_token_program);
                Ok((
                    vec![pumpfun_sell_v2_leg(&payer, pool, sell_amt, min_out)],
                    min_out,
                    quote_ata,
                ))
            }

            // —— Receive stock/quote only ——
            (SellTo::Token(out_mint), Market::LaunchLabInner(pool)) => self
                .sell_launchlab_to_quote(
                    sell_amt, pool, *out_mint, slip, opts.min_out, meme_ata, setup, touched,
                    &opts.ata,
                ),
            (SellTo::Token(out_mint), Market::CpmmOuter(pool)) => self.sell_cpmm_to_mint(
                sell_amt, pool, meme, *out_mint, slip, opts.min_out, setup, touched, &opts.ata,
            ),
            (SellTo::Token(out_mint), Market::PumpSwapOuter(pool)) => {
                let tp = pool.quote_token_program;
                let kind = if *out_mint == WSOL_MINT {
                    touched.touch_wsol();
                    AtaKind::Wsol
                } else {
                    touched.touch_quote(*out_mint, tp);
                    AtaKind::Quote
                };
                push_create_ata(setup, &opts.ata, kind, &payer, out_mint, &tp);
                self.sell_pumpswap_to_quote(sell_amt, pool, *out_mint, slip, opts.min_out)
            }
            (SellTo::Token(out_mint), Market::RaydiumAmmV4(pool)) => {
                let kind = if *out_mint == WSOL_MINT {
                    touched.touch_wsol();
                    AtaKind::Wsol
                } else {
                    touched.touch_quote(*out_mint, TOKEN_PROGRAM);
                    AtaKind::Quote
                };
                push_create_ata(setup, &opts.ata, kind, &payer, out_mint, &TOKEN_PROGRAM);
                self.sell_raydium_v4_to_mint(sell_amt, pool, *out_mint, slip, opts.min_out)
            }
            (SellTo::Token(out_mint), Market::MeteoraDammV2(pool)) => {
                let tp = if *out_mint == pool.token_a_mint {
                    pool.token_a_program
                } else {
                    pool.token_b_program
                };
                let kind = if *out_mint == WSOL_MINT {
                    touched.touch_wsol();
                    AtaKind::Wsol
                } else {
                    touched.touch_quote(*out_mint, tp);
                    AtaKind::Quote
                };
                push_create_ata(setup, &opts.ata, kind, &payer, out_mint, &tp);
                self.sell_meteora_v2_to_mint(sell_amt, pool, *out_mint, slip, opts.min_out)
            }
            (SellTo::Token(out_mint), Market::RaydiumClmm(pool)) => {
                let tp = if *out_mint == pool.token_0_mint {
                    pool.token_0_program
                } else {
                    pool.token_1_program
                };
                let kind = if *out_mint == WSOL_MINT {
                    touched.touch_wsol();
                    AtaKind::Wsol
                } else {
                    touched.touch_quote(*out_mint, tp);
                    AtaKind::Quote
                };
                push_create_ata(setup, &opts.ata, kind, &payer, out_mint, &tp);
                self.sell_raydium_clmm_to_mint(sell_amt, pool, *out_mint, slip, opts.min_out)
            }
            (SellTo::Token(out_mint), Market::Whirlpool(pool)) => {
                let tp = if *out_mint == pool.mint_a {
                    pool.token_program_a
                } else {
                    pool.token_program_b
                };
                let kind = if *out_mint == WSOL_MINT {
                    touched.touch_wsol();
                    AtaKind::Wsol
                } else {
                    touched.touch_quote(*out_mint, tp);
                    AtaKind::Quote
                };
                push_create_ata(setup, &opts.ata, kind, &payer, out_mint, &tp);
                self.sell_whirlpool_to_mint(sell_amt, pool, *out_mint, slip, opts.min_out)
            }
            (SellTo::Token(out_mint), Market::MeteoraDlmm(pool)) => {
                let tp = if *out_mint == pool.token_x_mint {
                    pool.token_x_program
                } else {
                    pool.token_y_program
                };
                let kind = if *out_mint == WSOL_MINT {
                    touched.touch_wsol();
                    AtaKind::Wsol
                } else {
                    touched.touch_quote(*out_mint, tp);
                    AtaKind::Quote
                };
                push_create_ata(setup, &opts.ata, kind, &payer, out_mint, &tp);
                self.sell_meteora_dlmm_to_mint(sell_amt, pool, *out_mint, slip, opts.min_out)
            }

            // —— Receive SOL/WSOL, LaunchLab SOL-quoted ——
            (SellTo::Sol | SellTo::Wsol, Market::LaunchLabInner(pool)) if pool.is_sol_quote() => {
                let quote_ata = ata(&payer, &WSOL_MINT, &pool.quote_token_program);
                let expected = launchlab_sell_quote_out(pool, sell_amt)?;
                let min_out = opts
                    .min_out
                    .unwrap_or_else(|| apply_slippage_min_out(expected, slip));
                Ok((
                    vec![launchlab_sell_leg(
                        &payer, pool, sell_amt, min_out, meme_ata, quote_ata,
                    )],
                    min_out,
                    quote_ata,
                ))
            }
            // PumpFun V2 non-WSOL quote → sell to quote then bridge to WSOL.
            (SellTo::Sol | SellTo::Wsol, Market::PumpFunInner(pool))
                if pool.uses_v2() && !pool.is_native_sol_quote() =>
            {
                let bridge = market
                    .bridge
                    .as_ref()
                    .ok_or_else(|| anyhow!("PumpFun V2 non-WSOL quote needs quote↔SOL bridge"))?;
                self.sell_via_bridge_mainstream(
                    sell_amt,
                    &market.market,
                    bridge,
                    slip,
                    opts.min_out,
                    setup,
                    touched,
                    &opts.ata,
                )
            }

            // —— Receive SOL/WSOL via bridge ——
            (SellTo::Sol | SellTo::Wsol, Market::LaunchLabInner(pool)) => {
                let bridge = market
                    .bridge
                    .as_ref()
                    .ok_or_else(|| anyhow!("missing SOL↔stock bridge"))?;
                self.sell_via_bridge_launchlab(
                    sell_amt, pool, bridge, slip, opts.min_out, setup, touched, &opts.ata,
                )
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
                    opts.min_out,
                    setup,
                    touched,
                    &opts.ata,
                )
            }
            (SellTo::Sol | SellTo::Wsol, Market::PumpSwapOuter(pool))
                if pool.quote_mint == WSOL_MINT =>
            {
                self.sell_pumpswap_to_quote(sell_amt, pool, WSOL_MINT, slip, opts.min_out)
            }
            (SellTo::Sol | SellTo::Wsol, Market::RaydiumAmmV4(pool))
                if !market.market.needs_sol_bridge() =>
            {
                self.sell_raydium_v4_to_mint(sell_amt, pool, WSOL_MINT, slip, opts.min_out)
            }
            (SellTo::Sol | SellTo::Wsol, Market::MeteoraDammV2(pool))
                if !market.market.needs_sol_bridge() =>
            {
                self.sell_meteora_v2_to_mint(sell_amt, pool, WSOL_MINT, slip, opts.min_out)
            }
            (SellTo::Sol | SellTo::Wsol, Market::RaydiumClmm(pool))
                if !market.market.needs_sol_bridge() =>
            {
                self.sell_raydium_clmm_to_mint(sell_amt, pool, WSOL_MINT, slip, opts.min_out)
            }
            (SellTo::Sol | SellTo::Wsol, Market::Whirlpool(pool))
                if !market.market.needs_sol_bridge() =>
            {
                self.sell_whirlpool_to_mint(sell_amt, pool, WSOL_MINT, slip, opts.min_out)
            }
            (SellTo::Sol | SellTo::Wsol, Market::MeteoraDlmm(pool))
                if !market.market.needs_sol_bridge() =>
            {
                self.sell_meteora_dlmm_to_mint(sell_amt, pool, WSOL_MINT, slip, opts.min_out)
            }
            (SellTo::Sol | SellTo::Wsol, target @ Market::PumpSwapOuter(_))
            | (SellTo::Sol | SellTo::Wsol, target @ Market::RaydiumAmmV4(_))
            | (SellTo::Sol | SellTo::Wsol, target @ Market::MeteoraDammV2(_))
            | (SellTo::Sol | SellTo::Wsol, target @ Market::RaydiumClmm(_))
            | (SellTo::Sol | SellTo::Wsol, target @ Market::Whirlpool(_))
            | (SellTo::Sol | SellTo::Wsol, target @ Market::MeteoraDlmm(_)) => {
                let bridge = market
                    .bridge
                    .as_ref()
                    .ok_or_else(|| anyhow!("missing SOL↔quote bridge"))?;
                self.sell_via_bridge_mainstream(
                    sell_amt,
                    target,
                    bridge,
                    slip,
                    opts.min_out,
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
                self.sell_via_bridge_cpmm(
                    sell_amt, pool, bridge, slip, opts.min_out, setup, touched, &opts.ata,
                )
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
        explicit_min_out: Option<u64>,
        meme_ata: Pubkey,
        setup: &mut Vec<Instruction>,
        touched: &mut TouchedAtas,
        policy: &AtaPolicy,
    ) -> Result<(Vec<Leg>, u64, u64)> {
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
        push_create_ata(
            setup,
            policy,
            kind,
            &payer,
            &pay_mint,
            &pool.quote_token_program,
        );
        let q = launchlab_buy_quote(pool, spend, 0)?;
        let min_out = explicit_min_out
            .unwrap_or_else(|| apply_slippage_min_out(q.amount_out, slip));
        Ok((
            vec![launchlab_buy_leg(
                &payer,
                pool,
                q.amount_in,
                min_out,
                meme_ata,
                quote_ata,
            )],
            min_out,
            q.amount_in,
        ))
    }

    fn buy_pumpswap_with_quote(
        &self,
        spend: u64,
        pool: &PumpSwapPool,
        pay_mint: Pubkey,
        slip: u64,
        explicit_min_out: Option<u64>,
    ) -> Result<(Vec<Leg>, u64)> {
        if pay_mint != pool.quote_mint {
            return Err(anyhow!("PumpSwap pay mint must be pool quote"));
        }
        let expected = pumpswap_buy_base_out(pool, spend)?;
        let min_out = explicit_min_out.unwrap_or_else(|| apply_slippage_min_out(expected, slip));
        Ok((
            vec![pumpswap_buy_leg(&self.payer, pool, spend, min_out)?],
            min_out,
        ))
    }

    fn sell_pumpswap_to_quote(
        &self,
        sell_amt: u64,
        pool: &PumpSwapPool,
        out_mint: Pubkey,
        slip: u64,
        explicit_min_out: Option<u64>,
    ) -> Result<(Vec<Leg>, u64, Pubkey)> {
        if out_mint != pool.quote_mint {
            return Err(anyhow!("PumpSwap receive mint must be pool quote"));
        }
        let expected = pumpswap_sell_quote_out(pool, sell_amt)?;
        let min_out = explicit_min_out.unwrap_or_else(|| apply_slippage_min_out(expected, slip));
        let output_ata = ata(&self.payer, &out_mint, &pool.quote_token_program);
        Ok((
            vec![pumpswap_sell_leg(&self.payer, pool, sell_amt, min_out)?],
            min_out,
            output_ata,
        ))
    }

    fn buy_raydium_v4_with_mint(
        &self,
        spend: u64,
        pool: &RaydiumAmmV4Pool,
        input_mint: Pubkey,
        slip: u64,
        explicit_min_out: Option<u64>,
    ) -> Result<(Vec<Leg>, u64)> {
        let expected = raydium_amm_v4_out(pool, spend, input_mint == pool.coin_mint)?;
        let min_out = explicit_min_out.unwrap_or_else(|| apply_slippage_min_out(expected, slip));
        Ok((
            vec![raydium_amm_v4_swap_leg(
                &self.payer,
                pool,
                spend,
                min_out,
                input_mint,
            )?],
            min_out,
        ))
    }

    fn sell_raydium_v4_to_mint(
        &self,
        sell_amt: u64,
        pool: &RaydiumAmmV4Pool,
        output_mint: Pubkey,
        slip: u64,
        explicit_min_out: Option<u64>,
    ) -> Result<(Vec<Leg>, u64, Pubkey)> {
        let input_mint = if output_mint == pool.coin_mint {
            pool.pc_mint
        } else if output_mint == pool.pc_mint {
            pool.coin_mint
        } else {
            return Err(anyhow!("Raydium AMM V4 output mint does not match pool"));
        };
        let expected = raydium_amm_v4_out(pool, sell_amt, input_mint == pool.coin_mint)?;
        let min_out = explicit_min_out.unwrap_or_else(|| apply_slippage_min_out(expected, slip));
        Ok((
            vec![raydium_amm_v4_swap_leg(
                &self.payer,
                pool,
                sell_amt,
                min_out,
                input_mint,
            )?],
            min_out,
            ata(&self.payer, &output_mint, &TOKEN_PROGRAM),
        ))
    }

    fn buy_meteora_v2_with_mint(
        &self,
        spend: u64,
        pool: &MeteoraDammV2Pool,
        input_mint: Pubkey,
        slip: u64,
        explicit_min_out: Option<u64>,
    ) -> Result<(Vec<Leg>, u64)> {
        let expected = meteora_damm_v2_out(pool, spend, input_mint == pool.token_a_mint)?;
        let min_out = explicit_min_out.unwrap_or_else(|| apply_slippage_min_out(expected, slip));
        Ok((
            vec![meteora_damm_v2_swap_leg(
                &self.payer,
                pool,
                spend,
                min_out,
                input_mint,
            )?],
            min_out,
        ))
    }

    fn sell_meteora_v2_to_mint(
        &self,
        sell_amt: u64,
        pool: &MeteoraDammV2Pool,
        output_mint: Pubkey,
        slip: u64,
        explicit_min_out: Option<u64>,
    ) -> Result<(Vec<Leg>, u64, Pubkey)> {
        let (input_mint, output_program) = if output_mint == pool.token_a_mint {
            (pool.token_b_mint, pool.token_a_program)
        } else if output_mint == pool.token_b_mint {
            (pool.token_a_mint, pool.token_b_program)
        } else {
            return Err(anyhow!("Meteora DAMM V2 output mint does not match pool"));
        };
        let expected = meteora_damm_v2_out(pool, sell_amt, input_mint == pool.token_a_mint)?;
        let min_out = explicit_min_out.unwrap_or_else(|| apply_slippage_min_out(expected, slip));
        Ok((
            vec![meteora_damm_v2_swap_leg(
                &self.payer,
                pool,
                sell_amt,
                min_out,
                input_mint,
            )?],
            min_out,
            ata(&self.payer, &output_mint, &output_program),
        ))
    }

    fn buy_raydium_clmm_with_mint(
        &self,
        amount: u64,
        pool: &RaydiumClmmPool,
        input: Pubkey,
        slip: u64,
        explicit: Option<u64>,
    ) -> Result<(Vec<Leg>, u64)> {
        let min = concentrated_min_out(
            pool.expected_out,
            pool.quoted_amount_in,
            amount,
            explicit,
            slip,
            "Raydium CLMM",
        )?;
        Ok((
            vec![raydium_clmm_swap_leg(
                &self.payer,
                pool,
                amount,
                min,
                input,
            )?],
            min,
        ))
    }

    fn sell_raydium_clmm_to_mint(
        &self,
        amount: u64,
        pool: &RaydiumClmmPool,
        output: Pubkey,
        slip: u64,
        explicit: Option<u64>,
    ) -> Result<(Vec<Leg>, u64, Pubkey)> {
        let (input, output_program) = pair_input_and_output_program(
            output,
            pool.token_0_mint,
            pool.token_1_mint,
            pool.token_0_program,
            pool.token_1_program,
            "Raydium CLMM",
        )?;
        let min = concentrated_min_out(
            pool.expected_out,
            pool.quoted_amount_in,
            amount,
            explicit,
            slip,
            "Raydium CLMM",
        )?;
        Ok((
            vec![raydium_clmm_swap_leg(
                &self.payer,
                pool,
                amount,
                min,
                input,
            )?],
            min,
            ata(&self.payer, &output, &output_program),
        ))
    }

    fn buy_whirlpool_with_mint(
        &self,
        amount: u64,
        pool: &WhirlpoolPool,
        input: Pubkey,
        slip: u64,
        explicit: Option<u64>,
    ) -> Result<(Vec<Leg>, u64)> {
        let min = concentrated_min_out(
            pool.expected_out,
            pool.quoted_amount_in,
            amount,
            explicit,
            slip,
            "Orca Whirlpool",
        )?;
        Ok((
            vec![whirlpool_swap_leg(&self.payer, pool, amount, min, input)?],
            min,
        ))
    }

    fn sell_whirlpool_to_mint(
        &self,
        amount: u64,
        pool: &WhirlpoolPool,
        output: Pubkey,
        slip: u64,
        explicit: Option<u64>,
    ) -> Result<(Vec<Leg>, u64, Pubkey)> {
        let (input, output_program) = pair_input_and_output_program(
            output,
            pool.mint_a,
            pool.mint_b,
            pool.token_program_a,
            pool.token_program_b,
            "Orca Whirlpool",
        )?;
        let min = concentrated_min_out(
            pool.expected_out,
            pool.quoted_amount_in,
            amount,
            explicit,
            slip,
            "Orca Whirlpool",
        )?;
        Ok((
            vec![whirlpool_swap_leg(&self.payer, pool, amount, min, input)?],
            min,
            ata(&self.payer, &output, &output_program),
        ))
    }

    fn buy_meteora_dlmm_with_mint(
        &self,
        amount: u64,
        pool: &MeteoraDlmmPool,
        input: Pubkey,
        slip: u64,
        explicit: Option<u64>,
    ) -> Result<(Vec<Leg>, u64)> {
        let min = concentrated_min_out(
            pool.expected_out,
            pool.quoted_amount_in,
            amount,
            explicit,
            slip,
            "Meteora DLMM",
        )?;
        Ok((
            vec![meteora_dlmm_swap_leg(
                &self.payer,
                pool,
                amount,
                min,
                input,
            )?],
            min,
        ))
    }

    fn sell_meteora_dlmm_to_mint(
        &self,
        amount: u64,
        pool: &MeteoraDlmmPool,
        output: Pubkey,
        slip: u64,
        explicit: Option<u64>,
    ) -> Result<(Vec<Leg>, u64, Pubkey)> {
        let (input, output_program) = pair_input_and_output_program(
            output,
            pool.token_x_mint,
            pool.token_y_mint,
            pool.token_x_program,
            pool.token_y_program,
            "Meteora DLMM",
        )?;
        let min = concentrated_min_out(
            pool.expected_out,
            pool.quoted_amount_in,
            amount,
            explicit,
            slip,
            "Meteora DLMM",
        )?;
        Ok((
            vec![meteora_dlmm_swap_leg(
                &self.payer,
                pool,
                amount,
                min,
                input,
            )?],
            min,
            ata(&self.payer, &output, &output_program),
        ))
    }

    fn buy_cpmm_with_mint(
        &self,
        spend: u64,
        pool: &CpmmPool,
        input_mint: Pubkey,
        output_mint: Pubkey,
        slip: u64,
        explicit_min_out: Option<u64>,
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
            push_create_ata(
                setup,
                policy,
                AtaKind::Wsol,
                &payer,
                &output_mint,
                &output_tp,
            );
        } else {
            touched.touch_meme(output_mint, output_tp);
            push_create_ata(
                setup,
                policy,
                AtaKind::Meme,
                &payer,
                &output_mint,
                &output_tp,
            );
        }
        let expected = cpmm_out(pool, spend, input_is_base)?;
        let min_out = explicit_min_out
            .unwrap_or_else(|| apply_slippage_min_out(expected, slip));
        let leg = cpmm_swap_leg(
            &payer,
            pool,
            spend,
            min_out,
            input_mint,
            output_mint,
            user_in,
            user_out,
        )?;
        Ok((vec![leg], min_out))
    }

    fn sell_launchlab_to_quote(
        &self,
        sell_amt: u64,
        pool: &LaunchLabPool,
        out_mint: Pubkey,
        slip: u64,
        explicit_min_out: Option<u64>,
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
        push_create_ata(
            setup,
            policy,
            kind,
            &payer,
            &out_mint,
            &pool.quote_token_program,
        );
        let expected = launchlab_sell_quote_out(pool, sell_amt)?;
        let min_out = explicit_min_out
            .unwrap_or_else(|| apply_slippage_min_out(expected, slip));
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
        explicit_min_out: Option<u64>,
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
        let min_out = explicit_min_out
            .unwrap_or_else(|| apply_slippage_min_out(expected, slip));
        let leg = cpmm_swap_leg(
            &payer,
            pool,
            sell_amt,
            min_out,
            input_mint,
            output_mint,
            user_in,
            user_out,
        )?;
        Ok((vec![leg], min_out, user_out))
    }

    fn buy_via_bridge_launchlab(
        &self,
        spend: u64,
        pool: &LaunchLabPool,
        bridge: &CpmmPool,
        slippage_bps: u64,
        explicit_min_out: Option<u64>,
        setup: &mut Vec<Instruction>,
        touched: &mut TouchedAtas,
        policy: &AtaPolicy,
    ) -> Result<(Vec<Leg>, u64, u64)> {
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
        let provisional_stock = cpmm_out(bridge, spend, input_is_base)?;
        let q0 = launchlab_buy_quote(pool, provisional_stock, 0)?;
        // Near graduation LaunchLab clamps amount_in < provisional — shrink hop1
        // so stock_ata is not left with residual quote.
        let (wsol_spent, q) = if q0.amount_in < provisional_stock {
            let wsol = cpmm_in_for_out(bridge, q0.amount_in, input_is_base)?;
            let stock_out = cpmm_out(bridge, wsol, input_is_base)?;
            let q = launchlab_buy_quote(pool, stock_out.min(q0.amount_in), 0)?;
            (wsol, q)
        } else {
            (spend, q0)
        };
        let hop1_min = bridge_intermediate_min_out(q.amount_in);
        let leg1 = cpmm_swap_leg(
            &payer, bridge, wsol_spent, hop1_min, WSOL_MINT, stock, wsol_ata, stock_ata,
        )?;
        let hop2_in = hop1_min.min(q.amount_in);
        let q2 = launchlab_buy_quote(pool, hop2_in, 0)?;
        let min_meme = explicit_min_out
            .unwrap_or_else(|| apply_slippage_min_out(q2.amount_out, slippage_bps));
        let leg2 = launchlab_buy_leg(&payer, pool, hop2_in, min_meme, meme_ata, stock_ata);
        Ok((vec![leg1, leg2], min_meme, wsol_spent))
    }

    #[allow(clippy::too_many_arguments)]
    fn buy_via_bridge_mainstream(
        &self,
        spend: u64,
        market: &Market,
        bridge: &CpmmPool,
        slippage_bps: u64,
        explicit_min_out: Option<u64>,
        setup: &mut Vec<Instruction>,
        touched: &mut TouchedAtas,
        policy: &AtaPolicy,
    ) -> Result<(Vec<Leg>, u64)> {
        let payer = self.payer;
        let quote = market.quote_mint();
        let quote_tp = market.quote_token_program();
        let quote_ata = ata(&payer, &quote, &quote_tp);
        let wsol_ata = ata(&payer, &WSOL_MINT, &TOKEN_PROGRAM);
        touched.touch_quote(quote, quote_tp);
        push_create_ata(setup, policy, AtaKind::Quote, &payer, &quote, &quote_tp);

        let bridge_input_is_base = bridge.base_mint == WSOL_MINT;
        if bridge.other_mint(&WSOL_MINT) != Some(quote) {
            return Err(anyhow!("bridge does not connect WSOL to market quote"));
        }
        let quote_out = cpmm_out(bridge, spend, bridge_input_is_base)?;
        // Intermediate hop pins quoted stock/quote out; slippage applies only on final hop.
        let hop1_min = bridge_intermediate_min_out(quote_out);
        let leg1 = cpmm_swap_leg(
            &payer, bridge, spend, hop1_min, WSOL_MINT, quote, wsol_ata, quote_ata,
        )?;
        // Hop2 amount_in = hop1 guaranteed min_out (= quoted).
        let (leg2, expected) = match market {
            Market::PumpFunInner(pool) if pool.uses_v2() => {
                if pool.quote_mint != quote {
                    return Err(anyhow!("PumpFun V2 bridge quote mismatch"));
                }
                let expected = pumpfun_buy_token_out(pool, hop1_min);
                let min = explicit_min_out
                    .unwrap_or_else(|| apply_slippage_min_out(expected, slippage_bps));
                (pumpfun_buy_v2_leg(&payer, pool, hop1_min, min), expected)
            }
            Market::PumpSwapOuter(pool) => {
                let expected = pumpswap_buy_base_out(pool, hop1_min)?;
                let min = explicit_min_out
                    .unwrap_or_else(|| apply_slippage_min_out(expected, slippage_bps));
                (pumpswap_buy_leg(&payer, pool, hop1_min, min)?, expected)
            }
            Market::RaydiumAmmV4(pool) => {
                let expected = raydium_amm_v4_out(pool, hop1_min, quote == pool.coin_mint)?;
                let min = explicit_min_out
                    .unwrap_or_else(|| apply_slippage_min_out(expected, slippage_bps));
                (
                    raydium_amm_v4_swap_leg(&payer, pool, hop1_min, min, quote)?,
                    expected,
                )
            }
            Market::MeteoraDammV2(pool) => {
                let expected = meteora_damm_v2_out(pool, hop1_min, quote == pool.token_a_mint)?;
                let min = explicit_min_out
                    .unwrap_or_else(|| apply_slippage_min_out(expected, slippage_bps));
                (
                    meteora_damm_v2_swap_leg(&payer, pool, hop1_min, min, quote)?,
                    expected,
                )
            }
            // Bridged CL hop2 input size ≠ streamer single-hop size → require explicit min_out.
            Market::RaydiumClmm(pool) => {
                let min = bridged_concentrated_min_out(explicit_min_out, "Raydium CLMM")?;
                (
                    raydium_clmm_swap_leg(&payer, pool, hop1_min, min, quote)?,
                    min,
                )
            }
            Market::Whirlpool(pool) => {
                let min = bridged_concentrated_min_out(explicit_min_out, "Orca Whirlpool")?;
                (whirlpool_swap_leg(&payer, pool, hop1_min, min, quote)?, min)
            }
            Market::MeteoraDlmm(pool) => {
                let min = bridged_concentrated_min_out(explicit_min_out, "Meteora DLMM")?;
                (
                    meteora_dlmm_swap_leg(&payer, pool, hop1_min, min, quote)?,
                    min,
                )
            }
            _ => return Err(anyhow!("unsupported mainstream bridge market")),
        };
        let min_out =
            explicit_min_out.unwrap_or_else(|| apply_slippage_min_out(expected, slippage_bps));
        Ok((vec![leg1, leg2], min_out))
    }

    fn buy_via_bridge_cpmm(
        &self,
        spend: u64,
        pool: &CpmmPool,
        bridge: &CpmmPool,
        slippage_bps: u64,
        explicit_min_out: Option<u64>,
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
        let hop1_min = bridge_intermediate_min_out(stock_out);
        let leg1 = cpmm_swap_leg(
            &payer, bridge, spend, hop1_min, WSOL_MINT, stock, wsol_ata, stock_ata,
        )?;
        let input_is_base2 = stock == pool.base_mint;
        let meme_out = cpmm_out(pool, hop1_min, input_is_base2)?;
        let min_meme = explicit_min_out
            .unwrap_or_else(|| apply_slippage_min_out(meme_out, slippage_bps));
        let leg2 = cpmm_swap_leg(
            &payer, pool, hop1_min, min_meme, stock, meme, stock_ata, meme_ata,
        )?;
        Ok((vec![leg1, leg2], min_meme))
    }

    fn sell_via_bridge_launchlab(
        &self,
        sell_amt: u64,
        pool: &LaunchLabPool,
        bridge: &CpmmPool,
        slippage_bps: u64,
        explicit_min_out: Option<u64>,
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
        // Intermediate hop pins quoted stock out; slippage only on final SOL min_out.
        let hop1_min = bridge_intermediate_min_out(stock_out);
        let leg1 = launchlab_sell_leg(&payer, pool, sell_amt, hop1_min, meme_ata, stock_ata);
        let input_is_base = stock == bridge.base_mint;
        let sol_out = cpmm_out(bridge, hop1_min, input_is_base)?;
        let min_sol = explicit_min_out
            .unwrap_or_else(|| apply_slippage_min_out(sol_out, slippage_bps));
        let leg2 = cpmm_swap_leg(
            &payer, bridge, hop1_min, min_sol, stock, WSOL_MINT, stock_ata, wsol_ata,
        )?;
        Ok((vec![leg1, leg2], min_sol, wsol_ata))
    }

    #[allow(clippy::too_many_arguments)]
    fn sell_via_bridge_mainstream(
        &self,
        sell_amt: u64,
        market: &Market,
        bridge: &CpmmPool,
        slippage_bps: u64,
        explicit_min_out: Option<u64>,
        setup: &mut Vec<Instruction>,
        touched: &mut TouchedAtas,
        policy: &AtaPolicy,
    ) -> Result<(Vec<Leg>, u64, Pubkey)> {
        let payer = self.payer;
        let quote = market.quote_mint();
        let quote_tp = market.quote_token_program();
        let quote_ata = ata(&payer, &quote, &quote_tp);
        let wsol_ata = ata(&payer, &WSOL_MINT, &TOKEN_PROGRAM);
        touched.touch_quote(quote, quote_tp);
        push_create_ata(setup, policy, AtaKind::Quote, &payer, &quote, &quote_tp);
        if bridge.other_mint(&WSOL_MINT) != Some(quote) {
            return Err(anyhow!("bridge does not connect market quote to WSOL"));
        }

        let (leg1, quote_out) = match market {
            Market::PumpFunInner(pool) if pool.uses_v2() => {
                if pool.quote_mint != quote {
                    return Err(anyhow!("PumpFun V2 bridge quote mismatch"));
                }
                let expected = pumpfun_sell_sol_out(pool, sell_amt);
                let hop1_min = bridge_intermediate_min_out(expected);
                (pumpfun_sell_v2_leg(&payer, pool, sell_amt, hop1_min), hop1_min)
            }
            Market::PumpSwapOuter(pool) => {
                let expected = pumpswap_sell_quote_out(pool, sell_amt)?;
                let hop1_min = bridge_intermediate_min_out(expected);
                (pumpswap_sell_leg(&payer, pool, sell_amt, hop1_min)?, hop1_min)
            }
            Market::RaydiumAmmV4(pool) => {
                let input = market.base_mint();
                let expected = raydium_amm_v4_out(pool, sell_amt, input == pool.coin_mint)?;
                let hop1_min = bridge_intermediate_min_out(expected);
                (
                    raydium_amm_v4_swap_leg(&payer, pool, sell_amt, hop1_min, input)?,
                    hop1_min,
                )
            }
            Market::MeteoraDammV2(pool) => {
                let input = market.base_mint();
                let expected = meteora_damm_v2_out(pool, sell_amt, input == pool.token_a_mint)?;
                let hop1_min = bridge_intermediate_min_out(expected);
                (
                    meteora_damm_v2_swap_leg(&payer, pool, sell_amt, hop1_min, input)?,
                    hop1_min,
                )
            }
            // Intermediate CL hop pins expected_out (slippage_bps=0); final hop applies slip.
            Market::RaydiumClmm(pool) => {
                let input = market.base_mint();
                let quote_est = concentrated_min_out(
                    pool.expected_out,
                    pool.quoted_amount_in,
                    sell_amt,
                    None,
                    0,
                    "Raydium CLMM",
                )?;
                (
                    raydium_clmm_swap_leg(&payer, pool, sell_amt, quote_est, input)?,
                    quote_est,
                )
            }
            Market::Whirlpool(pool) => {
                let input = market.base_mint();
                let quote_est = concentrated_min_out(
                    pool.expected_out,
                    pool.quoted_amount_in,
                    sell_amt,
                    None,
                    0,
                    "Orca Whirlpool",
                )?;
                (
                    whirlpool_swap_leg(&payer, pool, sell_amt, quote_est, input)?,
                    quote_est,
                )
            }
            Market::MeteoraDlmm(pool) => {
                let input = market.base_mint();
                let quote_est = concentrated_min_out(
                    pool.expected_out,
                    pool.quoted_amount_in,
                    sell_amt,
                    None,
                    0,
                    "Meteora DLMM",
                )?;
                (
                    meteora_dlmm_swap_leg(&payer, pool, sell_amt, quote_est, input)?,
                    quote_est,
                )
            }
            _ => return Err(anyhow!("unsupported mainstream bridge market")),
        };
        let sol_out = cpmm_out(bridge, quote_out, quote == bridge.base_mint)?;
        let min_sol =
            explicit_min_out.unwrap_or_else(|| apply_slippage_min_out(sol_out, slippage_bps));
        let leg2 = cpmm_swap_leg(
            &payer, bridge, quote_out, min_sol, quote, WSOL_MINT, quote_ata, wsol_ata,
        )?;
        Ok((vec![leg1, leg2], min_sol, wsol_ata))
    }

    fn sell_via_bridge_cpmm(
        &self,
        sell_amt: u64,
        pool: &CpmmPool,
        bridge: &CpmmPool,
        slippage_bps: u64,
        explicit_min_out: Option<u64>,
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
        let hop1_min = bridge_intermediate_min_out(stock_out);
        let leg1 = cpmm_swap_leg(
            &payer, pool, sell_amt, hop1_min, meme, stock, meme_ata, stock_ata,
        )?;
        let input_is_base2 = stock == bridge.base_mint;
        let sol_out = cpmm_out(bridge, hop1_min, input_is_base2)?;
        let min_sol = explicit_min_out
            .unwrap_or_else(|| apply_slippage_min_out(sol_out, slippage_bps));
        let leg2 = cpmm_swap_leg(
            &payer, bridge, hop1_min, min_sol, stock, WSOL_MINT, stock_ata, wsol_ata,
        )?;
        Ok((vec![leg1, leg2], min_sol, wsol_ata))
    }
}

fn concentrated_min_out(
    expected: Option<u64>,
    quoted_amount_in: Option<u64>,
    amount_in: u64,
    explicit: Option<u64>,
    slippage_bps: u64,
    dex: &str,
) -> Result<u64> {
    match quoted_amount_in {
        Some(qin) if qin == amount_in => {}
        Some(qin) => {
            return Err(anyhow!(
                "{dex} expected_out was quoted for amount_in={qin}, got {amount_in}; refresh snapshot"
            ));
        }
        None => {
            return Err(anyhow!(
                "{dex} snapshot missing quoted_amount_in; refresh snapshot (required even with with_min_out)"
            ));
        }
    }
    if let Some(min) = explicit {
        return Ok(min);
    }
    let expected = expected.ok_or_else(|| {
        anyhow!("{dex} snapshot has no expected_out; provide TradeOpts::with_min_out")
    })?;
    Ok(apply_slippage_min_out(expected, slippage_bps))
}

/// Intermediate bridge hop: pin the local quote (no slippage haircut).
/// Final-hop `min_out` applies `slippage_bps` once.
#[inline]
fn bridge_intermediate_min_out(quoted: u64) -> u64 {
    quoted
}

/// Bridged CL hop2 amount ≠ streamer single-hop size — never reuse `expected_out`.
#[inline]
fn bridged_concentrated_min_out(explicit: Option<u64>, dex: &str) -> Result<u64> {
    explicit.ok_or_else(|| {
        anyhow!("bridged {dex} requires TradeOpts::with_min_out (do not reuse single-hop expected_out)")
    })
}

fn pair_input_and_output_program(
    output: Pubkey,
    a: Pubkey,
    b: Pubkey,
    program_a: Pubkey,
    program_b: Pubkey,
    dex: &str,
) -> Result<(Pubkey, Pubkey)> {
    if output == a {
        Ok((b, program_a))
    } else if output == b {
        Ok((a, program_b))
    } else {
        Err(anyhow!("{dex} output mint does not match pool"))
    }
}

/// Smallest `amount_in` such that `amount_in - fee_amount(amount_in, fee_bps) >= spend`.
#[inline]
fn route_amount_in_for_spend(spend: u64, fee_bps: u16) -> u64 {
    if fee_bps == 0 || spend == 0 {
        return spend;
    }
    let bps = fee_bps as u128;
    let denom = 10_000u128.saturating_sub(bps);
    if denom == 0 {
        return u64::MAX;
    }
    let mut amount = (spend as u128).saturating_mul(10_000).div_ceil(denom);
    while amount.saturating_sub(amount.saturating_mul(bps) / 10_000) < spend as u128 {
        amount = amount.saturating_add(1);
    }
    amount.min(u64::MAX as u128) as u64
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_amount_in_covers_spend_after_fee() {
        for bps in [0u16, 1, 25, 100, 1000] {
            for spend in [1u64, 999, 1_000_000, 12_345_678] {
                let amount = route_amount_in_for_spend(spend, bps);
                let net = amount.saturating_sub(fee_amount(amount, bps));
                assert!(
                    net >= spend,
                    "bps={bps} spend={spend} amount={amount} net={net}"
                );
            }
        }
    }
}
