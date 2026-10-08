//! Pump 3.2.0 V3 trades, including the pool-to-be leg at graduation.
//! Quote amounts are in the curve's quote units; token quotes must be funded
//! before the route. No signing or broadcasting occurs in these helpers.
use crate::{
    ata::{ata, create_ata},
    constants::*,
    legs::Leg,
    market::PumpFunPool,
    quote::{fee_amount, pumpfun_sell_sol_out},
    route_ix::{build_route_instruction_ex, RouteAccounts},
};
use anyhow::{anyhow, bail, Result};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
};

pub const PUMPFUN_BUY_EXACT_QUOTE_IN_V3: [u8; 8] = [225, 247, 80, 30, 213, 179, 132, 136];
pub const PUMPFUN_BUY_V3: [u8; 8] = [7, 5, 29, 196, 245, 23, 101, 80];
pub const PUMPFUN_SELL_V3: [u8; 8] = [28, 146, 222, 119, 38, 196, 105, 213];

/// Current curve/config/mint snapshot. Use `load_pumpfun_v3_by_rpc` or an
/// authoritative cache; do not substitute an old trade output for current state.
#[derive(Clone, Debug)]
pub struct PumpFunV3Pool {
    pub curve: PumpFunPool,
    pub complete: bool,
    /// Enable only after verifying the target deployment supports the official
    /// post-completion leg. Cold RPC loading defaults to false: the captured
    /// mainnet ELF returns 6021 for our non-Mayhem crossing regression.
    pub supports_graduation: bool,
    pub real_quote_reserves: u64,
    pub curve_base_token_balance: u64,
    pub pool_migration_fee: u64,
    pub mayhem_mode: bool,
    /// A nonzero depth means the quote is itself a Pump coin. These helpers
    /// settle directly in that token; recursive SOL funding is caller-supplied.
    pub depth: u8,
    pub needs_curve_extension: bool,
    pub needs_volume_initialization: bool,
}

pub struct PumpFunV3Trade {
    /// Rent/ATA initialization runs outside the router's input budget check.
    pub setup: Vec<Instruction>,
    pub route: Instruction,
}

impl PumpFunV3Trade {
    pub fn into_instructions(mut self) -> Vec<Instruction> {
        self.setup.push(self.route);
        self.setup
    }
}

impl PumpFunV3Pool {
    fn validate(&self) -> Result<()> {
        let p = &self.curve;
        if self.complete {
            bail!("completed Pump curves must migrate before further trades");
        }
        if p.is_cashback_coin {
            bail!("Pump V3 rejects cashback coins; use V1/V2");
        }
        if !p.fee_rates_known {
            bail!("Pump V3 requires current FeeConfig rates");
        }
        for program in [p.mint_token_program, p.quote_token_program] {
            if program != TOKEN_PROGRAM && program != TOKEN_2022_PROGRAM {
                bail!("Pump V3 mint owner missing or unsupported");
            }
        }
        if p.quote_mint == WSOL_MINT && p.quote_token_program != TOKEN_PROGRAM {
            bail!("Pump V3 native quote requires classic WSOL mint");
        }
        if p.mint == Pubkey::default() || p.quote_mint == p.mint {
            bail!("invalid Pump V3 mints");
        }
        let curve =
            Pubkey::find_program_address(&[b"bonding-curve", p.mint.as_ref()], &PUMPFUN_PROGRAM).0;
        if p.bonding_curve != curve
            || p.associated_bonding_curve != ata(&curve, &p.mint, &p.mint_token_program)
        {
            bail!("Pump V3 curve/base vault is not canonical");
        }
        if p.buyback_fee_recipient == Pubkey::default() {
            bail!("Pump V3 buyback recipient missing");
        }
        Ok(())
    }

    fn rates(&self) -> Result<(u128, u128)> {
        self.validate()?;
        let protocol = self.curve.protocol_fee_bps as u128;
        let creator = if self.curve.has_creator {
            self.curve.creator_fee_bps as u128
        } else {
            0
        };
        if protocol + creator >= 10_000 {
            bail!("invalid Pump V3 fee schedule");
        }
        Ok((protocol, creator))
    }

    /// Official `getBuyV3TokenAmountFromQuoteAmount`, including graduation.
    /// Mayhem graduation with partial_fill=false is rejected before building.
    pub fn buy_quote(&self, budget: u64) -> Result<u64> {
        let (protocol, creator) = self.rates()?;
        let net = net_quote(budget as u128, protocol, creator)?;
        let base = self.curve.virtual_token_reserves as u128;
        let quote = self.curve.virtual_sol_reserves as u128;
        if base == 0 || quote == 0 {
            bail!("empty Pump V3 reserves");
        }
        let input = net
            .checked_sub(1)
            .filter(|v| *v > 0)
            .ok_or_else(|| anyhow!("Pump V3 budget too small"))?;
        let tokens = input * base / (quote + input);
        let remaining = self.curve.real_token_reserves as u128;
        if tokens <= remaining {
            return positive_u64(tokens);
        }
        if !self.supports_graduation {
            bail!("Pump V3 graduation is not verified for this deployment");
        }
        if self.mayhem_mode {
            bail!("Pump V3 Mayhem graduation requires partial fill, unsupported by this exact-budget route");
        }
        let after = base
            .checked_sub(remaining)
            .filter(|v| *v > 0)
            .ok_or_else(|| anyhow!("Pump V3 graduation exhausts virtual reserve"))?;
        // Official curve exact-out uses floor + 1, including divisible cases.
        let curve_net = remaining * quote / after + 1;
        let cost = curve_net + fees(curve_net, protocol, creator);
        let left = (budget as u128).checked_sub(cost).unwrap_or(0);
        if left * 10_000 / (10_000 + protocol + creator) < 2 {
            return positive_u64(remaining);
        }
        let pool_base = (self.curve_base_token_balance as u128)
            .checked_sub(remaining)
            .filter(|v| *v > 0)
            .ok_or_else(|| anyhow!("empty Pump V3 pool-to-be base reserve"))?;
        let raised = (self.real_quote_reserves as u128)
            .checked_add(curve_net)
            .filter(|v| *v <= u64::MAX as u128)
            .ok_or_else(|| anyhow!("Pump V3 pool-to-be quote overflow"))?;
        let pool_quote = if self.curve.quote_mint == WSOL_MINT {
            raised.checked_sub(self.pool_migration_fee as u128)
        } else {
            Some(raised)
        }
        .filter(|v| *v > 0)
        .ok_or_else(|| anyhow!("empty Pump V3 pool-to-be quote reserve"))?;
        let leg_in = net_quote(left, protocol, creator)?
            .checked_sub(1)
            .ok_or_else(|| anyhow!("Pump V3 post-completion budget too small"))?;
        positive_u64(remaining + leg_in * pool_base / (pool_quote + leg_in))
    }

    /// Official `getBuyV3QuoteAmountFromTokenAmount`, fees included.
    pub fn buy_quote_for_tokens(&self, amount: u64) -> Result<u64> {
        let (protocol, creator) = self.rates()?;
        if amount == 0 {
            bail!("Pump V3 output must be positive");
        }
        let remaining = self.curve.real_token_reserves as u128;
        if self.mayhem_mode && amount as u128 > remaining {
            bail!("Pump V3 Mayhem cannot fulfill output beyond the curve");
        }
        if self.curve.virtual_sol_reserves == 0 {
            bail!("empty Pump V3 quote reserve");
        }
        let curve_tokens = (amount as u128).min(remaining);
        let after = (self.curve.virtual_token_reserves as u128)
            .checked_sub(curve_tokens)
            .filter(|v| *v > 0)
            .ok_or_else(|| anyhow!("Pump V3 curve reserve exhausted"))?;
        let net = curve_tokens * self.curve.virtual_sol_reserves as u128 / after + 1;
        let curve_total = net + fees(net, protocol, creator);
        if amount as u128 <= remaining {
            return positive_u64(curve_total);
        }
        if !self.supports_graduation {
            bail!("Pump V3 graduation is not verified for this deployment");
        }
        let past = amount as u128 - remaining;
        let base = (self.curve_base_token_balance as u128)
            .checked_sub(remaining)
            .filter(|v| *v > past)
            .ok_or_else(|| anyhow!("Pump V3 output exhausts pool-to-be"))?;
        let raised = self.real_quote_reserves as u128 + net;
        let quote = if self.curve.quote_mint == WSOL_MINT {
            raised.checked_sub(self.pool_migration_fee as u128)
        } else {
            Some(raised)
        }
        .filter(|v| *v > 0 && *v <= u64::MAX as u128)
        .ok_or_else(|| anyhow!("invalid Pump V3 pool-to-be quote reserve"))?;
        let leg_net = (quote * past).div_ceil(base - past);
        positive_u64(curve_total + leg_net + fees(leg_net, protocol, creator))
    }

    /// Exact output V3 buy, including a graduation leg when available.
    pub fn buy_exact_out_leg(&self, user: &Pubkey, amount: u64, maximum: u64) -> Result<Leg> {
        if amount == 0 || maximum == 0 {
            bail!("Pump V3 amounts must be positive");
        }
        self.buy_quote_for_tokens(amount)?;
        let mut data = PUMPFUN_BUY_V3.to_vec();
        data.extend_from_slice(&amount.to_le_bytes());
        data.extend_from_slice(&maximum.to_le_bytes());
        data.push(0);
        Ok(Leg {
            program_id: PUMPFUN_PROGRAM,
            accounts: self.accounts(user)?,
            data,
        })
    }

    pub fn sell_quote(&self, amount: u64) -> Result<u64> {
        self.validate()?;
        pumpfun_sell_sol_out(&self.curve, amount)
    }

    fn accounts(&self, user: &Pubkey) -> Result<Vec<AccountMeta>> {
        self.validate()?;
        let p = &self.curve;
        let accumulator = Pubkey::find_program_address(
            &[b"user_volume_accumulator", user.as_ref()],
            &PUMPFUN_PROGRAM,
        )
        .0;
        let buyback = if p.quote_mint == WSOL_MINT {
            p.buyback_fee_recipient
        } else {
            ata(
                &p.buyback_fee_recipient,
                &p.quote_mint,
                &p.quote_token_program,
            )
        };
        Ok(vec![
            AccountMeta::new_readonly(PUMPFUN_GLOBAL, false),
            AccountMeta::new_readonly(p.mint, false),
            AccountMeta::new_readonly(p.quote_mint, false),
            AccountMeta::new_readonly(p.mint_token_program, false),
            AccountMeta::new_readonly(p.quote_token_program, false),
            AccountMeta::new(p.bonding_curve, false),
            AccountMeta::new(p.associated_bonding_curve, false),
            AccountMeta::new(
                ata(&p.bonding_curve, &p.quote_mint, &p.quote_token_program),
                false,
            ),
            AccountMeta::new(*user, true),
            AccountMeta::new(ata(user, &p.mint, &p.mint_token_program), false),
            AccountMeta::new(ata(user, &p.quote_mint, &p.quote_token_program), false),
            AccountMeta::new(accumulator, false),
            AccountMeta::new_readonly(PUMPFUN_FEE_CONFIG, false),
            AccountMeta::new(buyback, false),
            AccountMeta::new_readonly(SYSTEM_PROGRAM, false),
            AccountMeta::new_readonly(PUMPFUN_EVENT_AUTHORITY, false),
            AccountMeta::new_readonly(PUMPFUN_PROGRAM, false),
        ])
    }

    pub fn buy_leg(&self, user: &Pubkey, budget: u64, minimum: u64) -> Result<Leg> {
        if budget == 0 || minimum == 0 {
            bail!("Pump V3 amounts must be positive");
        }
        self.buy_quote(budget)?;
        let mut data = PUMPFUN_BUY_EXACT_QUOTE_IN_V3.to_vec();
        data.extend_from_slice(&budget.to_le_bytes());
        data.extend_from_slice(&minimum.to_le_bytes());
        data.push(0); // OptionBool(false), no partial fills.
        Ok(Leg {
            program_id: PUMPFUN_PROGRAM,
            accounts: self.accounts(user)?,
            data,
        })
    }

    pub fn sell_leg(&self, user: &Pubkey, amount: u64, minimum: u64) -> Result<Leg> {
        if amount == 0 || minimum == 0 {
            bail!("Pump V3 amounts must be positive");
        }
        let mut data = PUMPFUN_SELL_V3.to_vec();
        data.extend_from_slice(&amount.to_le_bytes());
        data.extend_from_slice(&minimum.to_le_bytes());
        Ok(Leg {
            program_id: PUMPFUN_PROGRAM,
            accounts: self.accounts(user)?,
            data,
        })
    }

    fn setup(&self, user: &Pubkey, buy: bool) -> Vec<Instruction> {
        let p = &self.curve;
        let mut setup = Vec::new();
        if self.needs_curve_extension {
            setup.push(Instruction {
                program_id: PUMPFUN_PROGRAM,
                accounts: vec![
                    AccountMeta::new(p.bonding_curve, false),
                    AccountMeta::new(*user, true),
                    AccountMeta::new_readonly(SYSTEM_PROGRAM, false),
                    AccountMeta::new_readonly(PUMPFUN_EVENT_AUTHORITY, false),
                    AccountMeta::new_readonly(PUMPFUN_PROGRAM, false),
                ],
                data: vec![234, 102, 194, 203, 150, 72, 62, 229],
            });
        }
        if self.needs_volume_initialization {
            let volume = Pubkey::find_program_address(
                &[b"user_volume_accumulator", user.as_ref()],
                &PUMPFUN_PROGRAM,
            )
            .0;
            setup.push(Instruction {
                program_id: PUMPFUN_PROGRAM,
                accounts: vec![
                    AccountMeta::new(*user, true),
                    AccountMeta::new_readonly(*user, false),
                    AccountMeta::new(volume, false),
                    AccountMeta::new_readonly(SYSTEM_PROGRAM, false),
                    AccountMeta::new_readonly(PUMPFUN_EVENT_AUTHORITY, false),
                    AccountMeta::new_readonly(PUMPFUN_PROGRAM, false),
                ],
                data: vec![94, 6, 202, 115, 255, 96, 232, 183],
            });
        }
        if buy {
            setup.push(create_ata(user, user, &p.mint, &p.mint_token_program));
        }
        if p.quote_mint != WSOL_MINT {
            if !buy {
                setup.push(create_ata(
                    user,
                    user,
                    &p.quote_mint,
                    &p.quote_token_program,
                ));
            }
            setup.push(create_ata(
                user,
                &p.buyback_fee_recipient,
                &p.quote_mint,
                &p.quote_token_program,
            ));
        }
        setup
    }

    /// Protected single V3 buy. The router charges its fee on the declared max
    /// budget and enforces debit <= budget; V3 may leave rounding dust unspent.
    /// `fee_bps` must match the current router config. Minimum is caller-supplied.
    pub fn build_buy_route(
        &self,
        router: &Pubkey,
        user: &Pubkey,
        fee_recipient: &Pubkey,
        budget: u64,
        fee_bps: u16,
        minimum: u64,
    ) -> Result<PumpFunV3Trade> {
        self.build_buy_route_inner(router, user, fee_recipient, budget, fee_bps, minimum, false)
    }

    fn build_buy_route_inner(
        &self,
        router: &Pubkey,
        user: &Pubkey,
        fee_recipient: &Pubkey,
        budget: u64,
        fee_bps: u16,
        minimum: u64,
        exact_out: bool,
    ) -> Result<PumpFunV3Trade> {
        if fee_bps > 10_000 {
            bail!("invalid router fee rate");
        }
        let spend = budget
            .checked_sub(fee_amount(budget, fee_bps))
            .filter(|v| *v > 0)
            .ok_or_else(|| anyhow!("router fee consumes V3 budget"))?;
        let p = &self.curve;
        let native = p.quote_mint == WSOL_MINT;
        let input = if native {
            *user
        } else {
            ata(user, &p.quote_mint, &p.quote_token_program)
        };
        let destination = if native {
            *fee_recipient
        } else {
            ata(fee_recipient, &p.quote_mint, &p.quote_token_program)
        };
        let fee_program = if native {
            SYSTEM_PROGRAM
        } else {
            p.quote_token_program
        };
        let output = ata(user, &p.mint, &p.mint_token_program);
        let leg = if exact_out {
            self.buy_exact_out_leg(user, minimum, spend)?
        } else {
            self.buy_leg(user, spend, minimum)?
        };
        let mut setup = self.setup(user, true);
        if !native && fee_bps > 0 {
            setup.push(create_ata(
                user,
                fee_recipient,
                &p.quote_mint,
                &p.quote_token_program,
            ));
        }
        let route = build_route_instruction_ex(
            router,
            RouteAccounts {
                payer: *user,
                fee_destination: destination,
                fee_source: input,
                output_token_account: output,
                fee_program,
                fee_mint: p.quote_mint,
            },
            budget,
            minimum,
            u8::from(!native),
            true,
            &p.mint,
            &[leg],
        );
        Ok(PumpFunV3Trade { setup, route })
    }

    /// Exact output protected by the declared maximum quote budget. Router fees
    /// are calculated on that maximum, as for all router exact-output trades.
    pub fn build_buy_exact_out_route(
        &self,
        router: &Pubkey,
        user: &Pubkey,
        fee_recipient: &Pubkey,
        maximum: u64,
        fee_bps: u16,
        amount: u64,
    ) -> Result<PumpFunV3Trade> {
        self.build_buy_route_inner(router, user, fee_recipient, maximum, fee_bps, amount, true)
    }

    pub fn build_sell_route(
        &self,
        router: &Pubkey,
        user: &Pubkey,
        fee_recipient: &Pubkey,
        amount: u64,
        fee_bps: u16,
        minimum: u64,
    ) -> Result<PumpFunV3Trade> {
        if fee_bps > 10_000 {
            bail!("invalid router fee rate");
        }
        let spend = amount
            .checked_sub(fee_amount(amount, fee_bps))
            .filter(|v| *v > 0)
            .ok_or_else(|| anyhow!("router fee consumes V3 input"))?;
        let p = &self.curve;
        let output = if p.quote_mint == WSOL_MINT {
            *user
        } else {
            ata(user, &p.quote_mint, &p.quote_token_program)
        };
        let expected = if p.quote_mint == WSOL_MINT {
            SYSTEM_PROGRAM
        } else {
            p.quote_mint
        };
        let leg = self.sell_leg(user, spend, minimum)?;
        let mut setup = self.setup(user, false);
        if fee_bps > 0 {
            setup.push(create_ata(
                user,
                fee_recipient,
                &p.mint,
                &p.mint_token_program,
            ));
        }
        let route = build_route_instruction_ex(
            router,
            RouteAccounts {
                payer: *user,
                fee_destination: ata(fee_recipient, &p.mint, &p.mint_token_program),
                fee_source: ata(user, &p.mint, &p.mint_token_program),
                output_token_account: output,
                fee_program: p.mint_token_program,
                fee_mint: p.mint,
            },
            amount,
            minimum,
            1,
            false,
            &expected,
            &[leg],
        );
        Ok(PumpFunV3Trade { setup, route })
    }
}

fn fees(amount: u128, protocol: u128, creator: u128) -> u128 {
    (amount * protocol).div_ceil(10_000) + (amount * creator).div_ceil(10_000)
}
fn net_quote(budget: u128, protocol: u128, creator: u128) -> Result<u128> {
    let net = budget * 10_000 / (10_000 + protocol + creator);
    let total = net + fees(net, protocol, creator);
    net.checked_sub(total.saturating_sub(budget))
        .ok_or_else(|| anyhow!("V3 budget cannot cover fees"))
}
fn positive_u64(value: u128) -> Result<u64> {
    let value = u64::try_from(value).map_err(|_| anyhow!("Pump V3 quote exceeds u64"))?;
    if value == 0 {
        bail!("Pump V3 quote rounds to zero");
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pool() -> PumpFunV3Pool {
        let mut curve = crate::parser::pumpfun_from_trade(&Default::default());
        curve.mint = Pubkey::new_unique();
        curve.mint_token_program = TOKEN_PROGRAM;
        curve.quote_mint = WSOL_MINT;
        curve.quote_token_program = TOKEN_PROGRAM;
        curve.bonding_curve = Pubkey::find_program_address(
            &[b"bonding-curve", curve.mint.as_ref()],
            &PUMPFUN_PROGRAM,
        )
        .0;
        curve.associated_bonding_curve = ata(&curve.bonding_curve, &curve.mint, &TOKEN_PROGRAM);
        curve.virtual_token_reserves = 1_000_000;
        curve.virtual_sol_reserves = 100_000;
        curve.real_token_reserves = 100_000;
        curve.protocol_fee_bps = 0;
        curve.creator_fee_bps = 0;
        curve.has_creator = true;
        curve.fee_rates_known = true;
        curve.buyback_fee_recipient = Pubkey::new_unique();
        PumpFunV3Pool {
            curve,
            complete: false,
            supports_graduation: true,
            real_quote_reserves: 20_000,
            curve_base_token_balance: 300_000,
            pool_migration_fee: 1_000,
            mayhem_mode: false,
            depth: 0,
            needs_curve_extension: false,
            needs_volume_initialization: false,
        }
    }

    #[test]
    fn official_quote_golden_vectors_cover_graduation_and_split_rounding() {
        // Independently calculated from pump-sdk 3.2.0 bondingCurve.ts:
        // net=budget, curve input=net-1; crossing costs floor(100000*100000/900000)+1=11112.
        let mut p = pool();
        assert_eq!(p.buy_quote(10_000).unwrap(), 90_900);
        // pool-to-be B=200000,Q=20000+11112-1000; remaining input=8888-1.
        assert_eq!(p.buy_quote(20_000).unwrap(), 145_575);
        p.supports_graduation = false;
        assert!(p.buy_quote(20_000).is_err());
        assert!(p.buy_leg(&Pubkey::new_unique(), 20_000, 1).is_err());
        assert!(p
            .buy_exact_out_leg(&Pubkey::new_unique(), 150_000, 30_000)
            .is_err());
        p.supports_graduation = true;
        p.curve.quote_mint = Pubkey::new_unique();
        assert_eq!(p.buy_quote(20_000).unwrap(), 144_436);
        p.curve.quote_mint = WSOL_MINT;
        p.curve.protocol_fee_bps = 95;
        p.curve.creator_fee_bps = 5;
        // ceil of both independent fees: net(10000)=9900, fees=95+5.
        assert_eq!(p.buy_quote(10_000).unwrap(), 90_073);
        assert_eq!(net_quote(101, 95, 5).unwrap(), 99);
        assert!(p.buy_quote(1).is_err());
        p.mayhem_mode = true;
        assert!(p.buy_quote(20_000).is_err());
        p.mayhem_mode = false;
        p.curve_base_token_balance = p.curve.real_token_reserves;
        assert!(p.buy_quote(20_000).is_err());
    }

    #[test]
    fn exact_output_quote_and_encoding_follow_official_curve_and_pool_math() {
        let mut p = pool();
        assert_eq!(p.buy_quote_for_tokens(100_000).unwrap(), 11_112);
        // Pool-to-be Q=30112,B=200000. ceil(30112*50000/150000)=10038.
        assert_eq!(p.buy_quote_for_tokens(150_000).unwrap(), 21_150);
        assert!(p.buy_quote_for_tokens(300_000).is_err());
        let user = Pubkey::new_unique();
        let trade = p
            .build_buy_exact_out_route(
                &PROGRAM_ID,
                &user,
                &Pubkey::new_unique(),
                30_000,
                100,
                150_000,
            )
            .unwrap();
        assert_eq!(&trade.route.data[86..94], &PUMPFUN_BUY_V3);
        assert_eq!(
            u64::from_le_bytes(trade.route.data[94..102].try_into().unwrap()),
            150_000
        );
        assert_eq!(
            u64::from_le_bytes(trade.route.data[102..110].try_into().unwrap()),
            29_700
        );
        assert_eq!(trade.route.data[110], 0);
        p.supports_graduation = false;
        assert_eq!(
            p.buy_quote_for_tokens(p.curve.real_token_reserves).unwrap(),
            11_112
        );
        assert!(p
            .buy_exact_out_leg(&user, p.curve.real_token_reserves, 30_000)
            .is_ok());
        assert!(p
            .buy_exact_out_leg(&user, p.curve.real_token_reserves + 1, 30_000)
            .is_err());
        // A large cap must not be mistaken for the actual exact-output spend.
        assert!(p
            .build_buy_exact_out_route(
                &PROGRAM_ID,
                &user,
                &Pubkey::new_unique(),
                30_000,
                100,
                50_000
            )
            .is_ok());
        p.mayhem_mode = true;
        assert!(p.buy_quote_for_tokens(150_000).is_err());
        p.complete = true;
        assert!(p.buy_quote_for_tokens(100).is_err());
    }

    #[test]
    fn official_v3_account_layout_and_partial_fill_false() {
        let mut p = pool();
        let user = Pubkey::new_unique();
        let leg = p.buy_leg(&user, 10_000, 1).unwrap();
        let keys: Vec<_> = leg.accounts.iter().map(|a| a.pubkey).collect();
        assert_eq!(
            keys,
            vec![
                PUMPFUN_GLOBAL,
                p.curve.mint,
                WSOL_MINT,
                TOKEN_PROGRAM,
                TOKEN_PROGRAM,
                p.curve.bonding_curve,
                p.curve.associated_bonding_curve,
                ata(&p.curve.bonding_curve, &WSOL_MINT, &TOKEN_PROGRAM),
                user,
                ata(&user, &p.curve.mint, &TOKEN_PROGRAM),
                ata(&user, &WSOL_MINT, &TOKEN_PROGRAM),
                Pubkey::find_program_address(
                    &[b"user_volume_accumulator", user.as_ref()],
                    &PUMPFUN_PROGRAM
                )
                .0,
                PUMPFUN_FEE_CONFIG,
                p.curve.buyback_fee_recipient,
                SYSTEM_PROGRAM,
                PUMPFUN_EVENT_AUTHORITY,
                PUMPFUN_PROGRAM
            ]
        );
        assert_eq!(leg.accounts.iter().filter(|a| a.is_signer).count(), 1);
        for i in [5, 6, 7, 8, 9, 10, 11, 13] {
            assert!(leg.accounts[i].is_writable);
        }
        for i in [0, 1, 2, 3, 4, 12, 14, 15, 16] {
            assert!(!leg.accounts[i].is_writable);
        }
        assert_eq!(leg.data.len(), 25);
        assert_eq!(leg.data[24], 0);
        assert_eq!(p.sell_leg(&user, 10, 1).unwrap().data.len(), 24);
        p.curve.quote_mint = Pubkey::new_unique();
        p.curve.quote_token_program = TOKEN_2022_PROGRAM;
        let leg = p.sell_leg(&user, 10, 1).unwrap();
        assert_eq!(
            leg.accounts[13].pubkey,
            ata(
                &p.curve.buyback_fee_recipient,
                &p.curve.quote_mint,
                &TOKEN_2022_PROGRAM
            )
        );
        p.curve.is_cashback_coin = true;
        assert!(p.sell_leg(&user, 10, 1).is_err());
        p.curve.is_cashback_coin = false;
        p.complete = true;
        assert!(p.buy_leg(&user, 10, 1).is_err());
        p.complete = false;
        p.curve.associated_bonding_curve = Pubkey::new_unique();
        assert!(p.buy_leg(&user, 10, 1).is_err());
    }

    #[test]
    fn routes_bind_output_and_max_budget_with_setup_outside_debit() {
        let mut p = pool();
        let user = Pubkey::new_unique();
        let fee = Pubkey::new_unique();
        p.needs_curve_extension = true;
        p.needs_volume_initialization = true;
        let buy = p
            .build_buy_route(&PROGRAM_ID, &user, &fee, 10_000, 100, 20)
            .unwrap();
        assert_eq!(buy.setup.len(), 3);
        assert_eq!(buy.route.data[17], 0x80); // max input, fee charged on 10000
        assert_eq!(&buy.route.data[19..51], p.curve.mint.as_ref());
        assert_eq!(buy.route.accounts[3].pubkey, user);
        assert_eq!(
            u64::from_le_bytes(buy.route.data[94..102].try_into().unwrap()),
            9_900
        );
        let sell = p
            .build_sell_route(&PROGRAM_ID, &user, &fee, 10_000, 100, 20)
            .unwrap();
        assert_eq!(sell.route.data[17], 1);
        assert_eq!(sell.route.accounts[4].pubkey, user);
        assert_eq!(&sell.route.data[19..51], SYSTEM_PROGRAM.as_ref());
        p.curve.quote_mint = Pubkey::new_unique();
        let sell = p
            .build_sell_route(&PROGRAM_ID, &user, &fee, 10_000, 100, 20)
            .unwrap();
        assert_eq!(&sell.route.data[19..51], p.curve.quote_mint.as_ref());
        assert_eq!(
            sell.route.accounts[4].pubkey,
            ata(&user, &p.curve.quote_mint, &TOKEN_PROGRAM)
        );
        assert!(p
            .build_buy_route(&PROGRAM_ID, &user, &fee, 10, 10_000, 1)
            .is_err());
    }
}
