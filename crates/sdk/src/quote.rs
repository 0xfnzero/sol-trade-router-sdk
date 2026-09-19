//! Local quotes aligned with sol-trade-sdk / on-chain math (no RPC).

use anyhow::{anyhow, Result};

use crate::{
    market::{
        CpmmPool, LaunchLabPool, MeteoraDammV2Pool, MeteoraDlmmPool, PumpFunPool, PumpSwapPool,
        RaydiumAmmV4Pool, RaydiumClmmPool, WhirlpoolPool,
    },
    transfer_fee::TokenTransferFee,
};

/// Max slippage 99.99% — matches sol-trade-sdk (prevents min_out = 0 at 100%).
pub const MAX_SLIPPAGE_BPS: u64 = 9_999;

const FEE_DENOM: u128 = 1_000_000;

#[inline(always)]
pub fn clamp_slippage_bps(slippage_bps: u64) -> u64 {
    slippage_bps.min(MAX_SLIPPAGE_BPS)
}

#[inline(always)]
pub fn apply_slippage_min_out(amount: u64, slippage_bps: u64) -> u64 {
    let slip = clamp_slippage_bps(slippage_bps) as u128;
    let out = (amount as u128).saturating_mul(10_000u128.saturating_sub(slip)) / 10_000u128;
    out.min(u64::MAX as u128) as u64
}

#[inline(always)]
pub fn fee_amount(amount: u64, fee_bps: u16) -> u64 {
    let out = (amount as u128).saturating_mul(fee_bps as u128) / 10_000u128;
    out.min(u64::MAX as u128) as u64
}

/// Ceiling fee: `ceil(amount * rate / 1e6)`.
#[inline(always)]
fn fee_ceil(amount: u128, rate: u128) -> u128 {
    amount.saturating_mul(rate).div_ceil(FEE_DENOM)
}

/// PumpFun-style fee with split ceil (sol-trade-sdk `compute_fee`).
#[inline(always)]
fn compute_fee_bps(amount: u128, fee_basis_points: u128) -> u128 {
    let whole = match (amount / 10_000).checked_mul(fee_basis_points) {
        Some(v) => v,
        None => return u128::MAX,
    };
    let rem = match (amount % 10_000).checked_mul(fee_basis_points) {
        Some(v) => v,
        None => return u128::MAX,
    };
    let rem_ceil = if rem % 10_000 == 0 {
        rem / 10_000
    } else {
        rem / 10_000 + 1
    };
    whole.saturating_add(rem_ceil)
}

#[inline(always)]
fn cpmm_trade_fee(amount: u64, fee_rate: u64) -> u64 {
    let numerator = (amount as u128).saturating_mul(fee_rate as u128);
    ((numerator + FEE_DENOM - 1) / FEE_DENOM) as u64
}

// ── LaunchLab ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LaunchLabBuyQuote {
    /// May be lower than requested near graduation.
    pub amount_in: u64,
    pub amount_out: u64,
}

fn launchlab_total_fee_rate(pool: &LaunchLabPool, share_fee_rate: u64) -> Result<u128> {
    let rate = (pool.trade_fee_rate as u128)
        .saturating_add(pool.platform_fee_rate as u128)
        .saturating_add(pool.creator_fee_rate as u128)
        .saturating_add(share_fee_rate as u128);
    if rate > FEE_DENOM {
        return Err(anyhow!("LaunchLab total fee rate exceeds 1_000_000"));
    }
    Ok(rate)
}

fn pre_fee_amount(post_fee: u128, fee_rate: u128) -> Result<u128> {
    let denom = FEE_DENOM
        .checked_sub(fee_rate)
        .filter(|v| *v > 0)
        .ok_or_else(|| anyhow!("LaunchLab fee rate must be below 1_000_000"))?;
    Ok(post_fee.saturating_mul(FEE_DENOM).div_ceil(denom))
}

/// LaunchLab buy quote (constant-product). Matches sol-trade-sdk `get_buy_quote`.
pub fn launchlab_buy_quote(
    pool: &LaunchLabPool,
    amount_in: u64,
    share_fee_rate: u64,
) -> Result<LaunchLabBuyQuote> {
    if amount_in == 0 {
        return Err(anyhow!("quote_in is zero"));
    }
    if pool.curve_type != 0 {
        return Err(anyhow!(
            "unsupported LaunchLab curve_type {}",
            pool.curve_type
        ));
    }
    let total_fee_rate = launchlab_total_fee_rate(pool, share_fee_rate)?;
    let quote_xfer = pool.quote_transfer_fee.calculate(amount_in);
    let vault_input = amount_in
        .checked_sub(quote_xfer)
        .ok_or_else(|| anyhow!("LaunchLab quote transfer fee exceeds input"))?
        as u128;
    let fee = fee_ceil(vault_input, total_fee_rate);
    let curve_input = vault_input
        .checked_sub(fee)
        .ok_or_else(|| anyhow!("LaunchLab fees exceed input"))?;
    let input_reserve = pool
        .virtual_quote
        .checked_add(pool.real_quote)
        .ok_or_else(|| anyhow!("LaunchLab quote reserve overflow"))?;
    let output_reserve = pool
        .virtual_base
        .checked_sub(pool.real_base)
        .ok_or_else(|| anyhow!("LaunchLab base reserve underflow"))?;
    if input_reserve == 0 || output_reserve == 0 {
        return Err(anyhow!("invalid launchlab reserves"));
    }
    let denominator = input_reserve
        .checked_add(curve_input)
        .ok_or_else(|| anyhow!("LaunchLab buy denominator overflow"))?;
    let quoted_output = curve_input
        .checked_mul(output_reserve)
        .and_then(|v| v.checked_div(denominator))
        .ok_or_else(|| anyhow!("Failed to quote LaunchLab buy"))?;

    let (actual_in, gross_out) = if pool.total_base_sell == 0 {
        (amount_in, quoted_output)
    } else {
        let remaining = pool
            .total_base_sell
            .checked_sub(pool.real_base)
            .ok_or_else(|| anyhow!("LaunchLab sold amount exceeds total_base_sell"))?;
        if quoted_output <= remaining {
            (amount_in, quoted_output)
        } else {
            let output_after = output_reserve
                .checked_sub(remaining)
                .filter(|v| *v > 0)
                .ok_or_else(|| anyhow!("LaunchLab graduation exhausts reserve"))?;
            let required_curve = input_reserve
                .checked_mul(remaining)
                .map(|v| v.div_ceil(output_after))
                .ok_or_else(|| anyhow!("LaunchLab graduation input overflow"))?;
            let required_vault = pre_fee_amount(required_curve, total_fee_rate)?;
            let required_vault = u64::try_from(required_vault)
                .map_err(|_| anyhow!("LaunchLab graduation input exceeds u64"))?;
            let inverse = pool.quote_transfer_fee.calculate_inverse(required_vault);
            let actual = required_vault
                .checked_add(inverse)
                .ok_or_else(|| anyhow!("LaunchLab transfer-fee input overflow"))?;
            (actual.min(amount_in), remaining)
        }
    };

    let gross_out = u64::try_from(gross_out).map_err(|_| anyhow!("LaunchLab buy out > u64"))?;
    let received = gross_out.saturating_sub(pool.base_transfer_fee.calculate(gross_out));
    Ok(LaunchLabBuyQuote {
        amount_in: actual_in,
        amount_out: received,
    })
}

#[inline]
pub fn launchlab_buy_base_out(pool: &LaunchLabPool, quote_in: u64) -> Result<u64> {
    Ok(launchlab_buy_quote(pool, quote_in, 0)?.amount_out)
}

pub fn launchlab_sell_quote_out(pool: &LaunchLabPool, base_in: u64) -> Result<u64> {
    if base_in == 0 {
        return Err(anyhow!("base_in is zero"));
    }
    if pool.curve_type != 0 {
        return Err(anyhow!(
            "unsupported LaunchLab curve_type {}",
            pool.curve_type
        ));
    }
    let base_xfer = pool.base_transfer_fee.calculate(base_in);
    let curve_input = base_in
        .checked_sub(base_xfer)
        .ok_or_else(|| anyhow!("LaunchLab base transfer fee exceeds input"))?
        as u128;
    let input_reserve = pool
        .virtual_base
        .checked_sub(pool.real_base)
        .ok_or_else(|| anyhow!("LaunchLab base reserve underflow"))?;
    let output_reserve = pool
        .virtual_quote
        .checked_add(pool.real_quote)
        .ok_or_else(|| anyhow!("LaunchLab quote reserve overflow"))?;
    if input_reserve == 0 {
        return Err(anyhow!("invalid launchlab reserves"));
    }
    let denominator = input_reserve
        .checked_add(curve_input)
        .ok_or_else(|| anyhow!("LaunchLab sell denominator overflow"))?;
    let gross = curve_input
        .checked_mul(output_reserve)
        .and_then(|v| v.checked_div(denominator))
        .ok_or_else(|| anyhow!("Failed to quote LaunchLab sell"))?;
    let fee = fee_ceil(gross, launchlab_total_fee_rate(pool, 0)?);
    let vault_out = gross
        .checked_sub(fee)
        .ok_or_else(|| anyhow!("LaunchLab fees exceed output"))?;
    let vault_out = u64::try_from(vault_out).map_err(|_| anyhow!("LaunchLab sell out > u64"))?;
    Ok(vault_out.saturating_sub(pool.quote_transfer_fee.calculate(vault_out)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transfer_fee::TokenTransferFee;
    use solana_sdk::pubkey::Pubkey;

    fn dummy_pubkey(n: u8) -> Pubkey {
        Pubkey::new_from_array([n; 32])
    }

    #[test]
    fn slippage_clamped_to_9999() {
        assert_eq!(
            apply_slippage_min_out(1_000_000, 10_000),
            apply_slippage_min_out(1_000_000, 9_999)
        );
        assert_ne!(apply_slippage_min_out(1_000_000, 9_999), 0);
    }

    #[test]
    fn transfer_fee_ceiling() {
        let fee = TokenTransferFee {
            basis_points: 300,
            maximum_fee: 2,
        };
        assert_eq!(fee.calculate(1), 1);
        assert_eq!(fee.calculate(100), 2);
    }

    #[test]
    fn pumpfun_fee_additive() {
        // With 125 bps total, input net = amount * 10000 / 10125
        let pool = PumpFunPool {
            mint: dummy_pubkey(1),
            mint_token_program: dummy_pubkey(2),
            quote_mint: crate::constants::WSOL_MINT,
            quote_token_program: crate::constants::TOKEN_PROGRAM,
            use_v2: false,
            bonding_curve: dummy_pubkey(3),
            associated_bonding_curve: dummy_pubkey(4),
            creator_vault: dummy_pubkey(5),
            fee_recipient: dummy_pubkey(6),
            buyback_fee_recipient: crate::constants::PUMPFUN_BUYBACK_FEE_RECIPIENT,
            global: dummy_pubkey(7),
            event_authority: dummy_pubkey(8),
            global_volume_accumulator: dummy_pubkey(9),
            user_volume_accumulator: dummy_pubkey(10),
            fee_config: dummy_pubkey(11),
            fee_program: dummy_pubkey(12),
            bonding_curve_v2: dummy_pubkey(13),
            protocol_fee_recipient: dummy_pubkey(14),
            virtual_token_reserves: 1_000_000_000,
            virtual_sol_reserves: 30_000_000_000,
            real_token_reserves: 800_000_000,
            protocol_fee_bps: 0,
            has_creator: true,
            is_cashback_coin: false,
        };
        let out = pumpfun_buy_token_out(&pool, 1_000_000);
        assert!(out > 0);
        // Additive fee yields more tokens than subtractive 1.25% would imply as under-estimate check
        let subtractive_net = 1_000_000u128 * 9875 / 10_000;
        let additive_net = 1_000_000u128 * 10_000 / 10_125;
        assert!(additive_net > subtractive_net);
        assert_eq!(pumpfun_total_fee_bps(true), 125);
        assert_eq!(pumpfun_total_fee_bps(false), 95);
    }

    #[test]
    fn cpmm_creator_fee_on_input_combined_rounding() {
        // Mirrors sol-trade-sdk: ceil(101 * 12500 / 1e6) = 2, split creator=1 trade=1
        let pool = CpmmPool {
            pool_state: dummy_pubkey(1),
            amm_config: dummy_pubkey(2),
            observation_state: dummy_pubkey(3),
            base_mint: dummy_pubkey(4),
            quote_mint: dummy_pubkey(5),
            base_vault: dummy_pubkey(6),
            quote_vault: dummy_pubkey(7),
            base_token_program: dummy_pubkey(8),
            quote_token_program: dummy_pubkey(9),
            base_reserve: 1_000_000,
            quote_reserve: 2_000_000,
            trade_fee_rate: 2_500,
            creator_fee_rate: 10_000,
            creator_fee_on: 0,
            enable_creator_fee: true,
            base_transfer_fee: TokenTransferFee::none(),
            quote_transfer_fee: TokenTransferFee::none(),
        };
        let out = cpmm_out(&pool, 101, true).unwrap();
        // input_less = 101 - 2 = 99; out = 2e6 * 99 / (1e6+99)
        let expected = (2_000_000u128 * 99) / (1_000_000 + 99);
        assert_eq!(out as u128, expected);
    }

    #[test]
    fn cpmm_in_for_out_round_trips() {
        let pool = CpmmPool {
            pool_state: dummy_pubkey(1),
            amm_config: dummy_pubkey(2),
            observation_state: dummy_pubkey(3),
            base_mint: dummy_pubkey(4),
            quote_mint: dummy_pubkey(5),
            base_vault: dummy_pubkey(6),
            quote_vault: dummy_pubkey(7),
            base_token_program: dummy_pubkey(8),
            quote_token_program: dummy_pubkey(9),
            base_reserve: 1_000_000_000,
            quote_reserve: 2_000_000_000,
            trade_fee_rate: 2_500,
            creator_fee_rate: 0,
            creator_fee_on: 0,
            enable_creator_fee: false,
            base_transfer_fee: TokenTransferFee::none(),
            quote_transfer_fee: TokenTransferFee::none(),
        };
        let want_out = 1_000_000u64;
        let amount_in = cpmm_in_for_out(&pool, want_out, true).unwrap();
        let got = cpmm_out(&pool, amount_in, true).unwrap();
        assert!(got >= want_out, "got {got} < want {want_out}");
        if amount_in > 1 {
            let under = cpmm_out(&pool, amount_in - 1, true).unwrap();
            assert!(under < want_out, "amount_in-1 still yields >= want");
        }
    }

    #[test]
    fn pumpswap_quotes_match_reference_integer_math() {
        let pool = PumpSwapPool {
            pool: dummy_pubkey(1),
            base_mint: dummy_pubkey(2),
            quote_mint: dummy_pubkey(3),
            pool_base_token_account: dummy_pubkey(4),
            pool_quote_token_account: dummy_pubkey(5),
            base_token_program: dummy_pubkey(6),
            quote_token_program: dummy_pubkey(7),
            coin_creator_vault_ata: dummy_pubkey(8),
            coin_creator_vault_authority: dummy_pubkey(9),
            coin_creator: dummy_pubkey(10),
            base_reserve: 800_000_000_000_000,
            quote_reserve: 100_000_000_000,
            virtual_quote_reserves: 5_000_000_000,
            lp_fee_bps: 20,
            protocol_fee_bps: 5,
            creator_fee_bps: 30,
            is_cashback_coin: false,
            protocol_fee_recipient: crate::constants::PUMPSWAP_PROTOCOL_FEE_RECIPIENT,
            buyback_fee_recipient: crate::constants::PUMPSWAP_BUYBACK_FEE_RECIPIENT,
        };
        assert_eq!(
            pumpswap_buy_base_out(&pool, 1_500_000_000).unwrap(),
            11_206_836_149_304
        );
        assert_eq!(
            pumpswap_sell_quote_out(&pool, 123_456_789_000).unwrap(),
            16_112_095
        );
    }

    #[test]
    fn meteora_damm_v2_prefers_reserve_quote_over_stale_expected_out() {
        let pool = MeteoraDammV2Pool {
            pool: dummy_pubkey(1),
            token_a_vault: dummy_pubkey(2),
            token_b_vault: dummy_pubkey(3),
            token_a_mint: dummy_pubkey(4),
            token_b_mint: dummy_pubkey(5),
            token_a_program: crate::constants::TOKEN_PROGRAM,
            token_b_program: crate::constants::TOKEN_PROGRAM,
            token_a_reserve: 1_000_000,
            token_b_reserve: 2_000_000,
            fee_bps: 0,
            quoted_amount_in: None,
            expected_out: Some(1),
            swap_mode: 0,
            referral_token_account: None,
            include_rate_limiter_sysvar: false,
        };
        let out = meteora_damm_v2_out(&pool, 100_000, true).unwrap();
        assert!(out > 1, "reserve quote must beat stale expected_out, got {out}");
    }

    #[test]
    fn fee_amount_matches_on_chain_floor() {
        assert_eq!(fee_amount(1_000_000, 100), 10_000);
        assert_eq!(fee_amount(999, 100), 9);
        assert_eq!(fee_amount(1, 1), 0);
    }
}

// ── CPMM ───────────────────────────────────────────────────────────────────

fn creator_fee_on_input(pool: &CpmmPool, input_is_base: bool) -> Result<bool> {
    if !pool.enable_creator_fee || pool.creator_fee_rate == 0 {
        return Ok(true); // treat as input-side with 0 rate
    }
    match pool.creator_fee_on {
        0 => Ok(true),
        1 => Ok(input_is_base),
        2 => Ok(!input_is_base),
        v => Err(anyhow!("invalid CPMM creator_fee_on {}", v)),
    }
}

/// CPMM swap quote — matches sol-trade-sdk `compute_swap_amount` (creator + transfer fees).
pub fn cpmm_out(pool: &CpmmPool, amount_in: u64, input_is_base: bool) -> Result<u64> {
    if amount_in == 0 {
        return Err(anyhow!("amount_in is zero"));
    }
    let (input_reserve, output_reserve, in_fee, out_fee): (
        u64,
        u64,
        TokenTransferFee,
        TokenTransferFee,
    ) = if input_is_base {
        (
            pool.base_reserve,
            pool.quote_reserve,
            pool.base_transfer_fee,
            pool.quote_transfer_fee,
        )
    } else {
        (
            pool.quote_reserve,
            pool.base_reserve,
            pool.quote_transfer_fee,
            pool.base_transfer_fee,
        )
    };
    if input_reserve == 0 || output_reserve == 0 {
        return Err(anyhow!("empty cpmm reserves"));
    }

    let actual_in = amount_in.saturating_sub(in_fee.calculate(amount_in));
    let on_input = creator_fee_on_input(pool, input_is_base)?;
    let creator_rate = if pool.enable_creator_fee {
        pool.creator_fee_rate
    } else {
        0
    };

    let (_, output_swapped) = if on_input {
        let total_rate = pool.trade_fee_rate.saturating_add(creator_rate);
        let total_fee = cpmm_trade_fee(actual_in, total_rate);
        let input_less = actual_in.saturating_sub(total_fee);
        let out = ((output_reserve as u128).saturating_mul(input_less as u128)
            / (input_reserve as u128).saturating_add(input_less as u128)) as u64;
        (input_less, out)
    } else {
        let trade_fee = cpmm_trade_fee(actual_in, pool.trade_fee_rate);
        let input_less = actual_in.saturating_sub(trade_fee);
        let out_swapped = ((output_reserve as u128).saturating_mul(input_less as u128)
            / (input_reserve as u128).saturating_add(input_less as u128))
            as u64;
        let creator_fee = cpmm_trade_fee(out_swapped, creator_rate);
        (input_less, out_swapped.saturating_sub(creator_fee))
    };

    Ok(output_swapped.saturating_sub(out_fee.calculate(output_swapped)))
}

/// Exact-out inverse of [`cpmm_out`] (binary search; includes pool fees).
pub fn cpmm_in_for_out(pool: &CpmmPool, amount_out: u64, input_is_base: bool) -> Result<u64> {
    if amount_out == 0 {
        return Ok(0);
    }
    let output_reserve = if input_is_base {
        pool.quote_reserve
    } else {
        pool.base_reserve
    };
    if amount_out >= output_reserve {
        return Err(anyhow!("cpmm amount_out exceeds reserve"));
    }
    let input_reserve = if input_is_base {
        pool.base_reserve
    } else {
        pool.quote_reserve
    };
    let naive = (amount_out as u128)
        .saturating_mul(input_reserve as u128)
        .div_ceil((output_reserve - amount_out) as u128);
    let mut lo = (naive as u64).max(1);
    let mut hi = lo.saturating_mul(2).saturating_add(10_000);
    while cpmm_out(pool, hi, input_is_base).unwrap_or(0) < amount_out {
        if hi == u64::MAX {
            return Err(anyhow!("cpmm_in_for_out overflow"));
        }
        hi = hi.saturating_mul(2).max(hi.saturating_add(1));
    }
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if cpmm_out(pool, mid, input_is_base).unwrap_or(0) >= amount_out {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    Ok(lo)
}

// ── PumpFun ────────────────────────────────────────────────────────────────

/// Protocol 95 bps + optional creator 30 bps (sol-trade-sdk).
#[inline(always)]
pub fn pumpfun_total_fee_bps(has_creator: bool) -> u64 {
    95 + if has_creator { 30 } else { 0 }
}

/// PumpFun buy — fee is **additive** (`amount * 10000 / (10000 + fee_bps)`).
/// `lamports_in` / `virtual_sol_reserves` are in **quote mint units** (WSOL lamports
/// or e.g. USDC micro-units for V2 non-WSOL curves).
pub fn pumpfun_buy_token_out(pool: &PumpFunPool, lamports_in: u64) -> u64 {
    if lamports_in == 0 || pool.virtual_token_reserves == 0 {
        return 0;
    }
    let fee_bps = if pool.protocol_fee_bps > 0 {
        pool.protocol_fee_bps.min(10_000)
    } else {
        pumpfun_total_fee_bps(pool.has_creator)
    } as u128;
    let input = (lamports_in as u128)
        .saturating_mul(10_000)
        .checked_div(fee_bps + 10_000)
        .unwrap_or(0);
    // Official buy_exact_sol_in: tokens_out uses (net_sol - 1) in the constant-product.
    let curve_in = input.saturating_sub(1);
    if curve_in == 0 {
        return 0;
    }
    let vtok = pool.virtual_token_reserves as u128;
    let vsol = pool.virtual_sol_reserves as u128;
    let denom = vsol.saturating_add(curve_in);
    if denom == 0 {
        return 0;
    }
    let tokens = curve_in
        .saturating_mul(vtok)
        .checked_div(denom)
        .unwrap_or(0)
        .min(pool.real_token_reserves as u128);
    tokens.min(u64::MAX as u128) as u64
}

pub fn pumpfun_sell_sol_out(pool: &PumpFunPool, token_in: u64) -> u64 {
    if token_in == 0 || pool.virtual_token_reserves == 0 {
        return 0;
    }
    let amount = token_in as u128;
    let vtok = pool.virtual_token_reserves as u128;
    let vsol = pool.virtual_sol_reserves as u128;
    let denom = vtok.saturating_add(amount);
    if denom == 0 {
        return 0;
    }
    let sol_cost = amount.saturating_mul(vsol).checked_div(denom).unwrap_or(0);
    let fee_bps = if pool.protocol_fee_bps > 0 {
        pool.protocol_fee_bps.min(10_000)
    } else {
        pumpfun_total_fee_bps(pool.has_creator)
    } as u128;
    let fee = compute_fee_bps(sol_cost, fee_bps);
    sol_cost.saturating_sub(fee).min(u64::MAX as u128) as u64
}

fn pumpswap_effective_quote(pool: &PumpSwapPool) -> Result<u64> {
    let effective = (pool.quote_reserve as i128)
        .checked_add(pool.virtual_quote_reserves)
        .ok_or_else(|| anyhow!("PumpSwap effective quote reserve overflow"))?;
    u64::try_from(effective)
        .ok()
        .filter(|reserve| *reserve > 0)
        .ok_or_else(|| anyhow!("invalid PumpSwap effective quote reserve"))
}

pub fn pumpswap_buy_base_out(pool: &PumpSwapPool, quote_in: u64) -> Result<u64> {
    if quote_in == 0 || pool.base_reserve == 0 || pool.quote_reserve == 0 {
        return Err(anyhow!("invalid PumpSwap input or reserves"));
    }
    let total_fee_bps = pool
        .lp_fee_bps
        .checked_add(pool.protocol_fee_bps)
        .and_then(|v| v.checked_add(pool.creator_fee_bps))
        .ok_or_else(|| anyhow!("PumpSwap fee bps overflow"))?;
    let denominator = 10_000u64
        .checked_add(total_fee_bps)
        .ok_or_else(|| anyhow!("PumpSwap fee denominator overflow"))?;
    let mut effective_in = (quote_in as u128).saturating_mul(10_000) / denominator as u128;
    let total_with_fees = effective_in
        .saturating_add(compute_fee_bps(effective_in, pool.lp_fee_bps as u128))
        .saturating_add(compute_fee_bps(effective_in, pool.protocol_fee_bps as u128))
        .saturating_add(compute_fee_bps(effective_in, pool.creator_fee_bps as u128));
    if total_with_fees > quote_in as u128 {
        effective_in = effective_in.saturating_sub(total_with_fees - quote_in as u128);
    }
    let curve_in = effective_in
        .checked_sub(1)
        .ok_or_else(|| anyhow!("PumpSwap quote input is too small after fees"))?;
    let effective_quote = pumpswap_effective_quote(pool)? as u128;
    Ok(((pool.base_reserve as u128).saturating_mul(curve_in)
        / effective_quote.saturating_add(curve_in))
    .min(u64::MAX as u128) as u64)
}

pub fn pumpswap_sell_quote_out(pool: &PumpSwapPool, base_in: u64) -> Result<u64> {
    if base_in == 0 || pool.base_reserve == 0 || pool.quote_reserve == 0 {
        return Err(anyhow!("invalid PumpSwap input or reserves"));
    }
    let gross = (pumpswap_effective_quote(pool)? as u128).saturating_mul(base_in as u128)
        / (pool.base_reserve as u128).saturating_add(base_in as u128);
    let fees = compute_fee_bps(gross, pool.lp_fee_bps as u128)
        .saturating_add(compute_fee_bps(gross, pool.protocol_fee_bps as u128))
        .saturating_add(compute_fee_bps(gross, pool.creator_fee_bps as u128));
    let out = gross.saturating_sub(fees).min(u64::MAX as u128) as u64;
    if gross.saturating_sub(compute_fee_bps(gross, pool.lp_fee_bps as u128))
        > pool.quote_reserve as u128
    {
        return Err(anyhow!("PumpSwap real quote reserve cannot cover output"));
    }
    Ok(out)
}

pub fn raydium_amm_v4_out(
    pool: &RaydiumAmmV4Pool,
    amount_in: u64,
    input_is_coin: bool,
) -> Result<u64> {
    if amount_in == 0 {
        return Err(anyhow!("amount_in is zero"));
    }
    let (input_reserve, output_reserve) = if input_is_coin {
        (pool.coin_reserve, pool.pc_reserve)
    } else {
        (pool.pc_reserve, pool.coin_reserve)
    };
    if input_reserve == 0 || output_reserve == 0 {
        return Err(anyhow!("empty Raydium AMM V4 reserves"));
    }
    // Matches sol-trade-sdk input trade fee; output is pure CP after net input
    // (do not subtract input-denominated swap_fee from output units).
    // Streamers must populate AmmInfo trade_fee_numerator (typical 25); 0 means 0.
    let trade_num = pool.trade_fee_numerator;
    let trade_fee = (amount_in as u128)
        .saturating_mul(trade_num as u128)
        .div_ceil(10_000) as u64;
    let net_in = amount_in.saturating_sub(trade_fee);
    let swapped = (output_reserve as u128).saturating_mul(net_in as u128)
        / (input_reserve as u128).saturating_add(net_in as u128);
    Ok(swapped.min(u64::MAX as u128) as u64)
}

/// Inverse of [`raydium_amm_v4_out`]: gross input needed for at least `amount_out`.
pub fn raydium_amm_v4_in_for_out(
    pool: &RaydiumAmmV4Pool,
    amount_out: u64,
    input_is_coin: bool,
) -> Result<u64> {
    if amount_out == 0 {
        return Err(anyhow!("amount_out is zero"));
    }
    let (input_reserve, output_reserve) = if input_is_coin {
        (pool.coin_reserve, pool.pc_reserve)
    } else {
        (pool.pc_reserve, pool.coin_reserve)
    };
    if input_reserve == 0 || output_reserve == 0 {
        return Err(anyhow!("empty Raydium AMM V4 reserves"));
    }
    if amount_out >= output_reserve {
        return Err(anyhow!("Raydium AMM V4 amount_out exceeds output reserve"));
    }
    let denom = (output_reserve - amount_out) as u128;
    let net_in = (amount_out as u128)
        .saturating_mul(input_reserve as u128)
        .div_ceil(denom);
    let trade_num = pool.trade_fee_numerator as u128;
    if trade_num >= 10_000 {
        return Err(anyhow!("invalid Raydium AMM V4 trade fee numerator"));
    }
    // amount_in - ceil(amount_in * trade_num / 10000) >= net_in
    let amount_in = if trade_num == 0 {
        net_in
    } else {
        net_in.saturating_mul(10_000).div_ceil(10_000 - trade_num)
    };
    let amount_in = amount_in.min(u64::MAX as u128) as u64;
    // Bump once if floor/ceil rounding left us short.
    if raydium_amm_v4_out(pool, amount_in, input_is_coin)? < amount_out {
        Ok(amount_in.saturating_add(1))
    } else {
        Ok(amount_in)
    }
}

pub fn meteora_damm_v2_out(
    pool: &MeteoraDammV2Pool,
    amount_in: u64,
    input_is_a: bool,
) -> Result<u64> {
    let (input_reserve, output_reserve) = if input_is_a {
        (pool.token_a_reserve, pool.token_b_reserve)
    } else {
        (pool.token_b_reserve, pool.token_a_reserve)
    };
    // Prefer amount-aware CP quote. When fee_bps is unknown (0) but streamer
    // provided a matching expected_out, use that — zero-fee CP would overstate.
    if amount_in > 0 && input_reserve > 0 && output_reserve > 0 {
        if pool.fee_bps == 0 {
            if let Some(expected) = pool.expected_out {
                if pool.quoted_amount_in == Some(amount_in) {
                    return Ok(expected);
                }
            }
        }
        let net_in = (amount_in as u128)
            .saturating_mul(10_000u128.saturating_sub(pool.fee_bps.min(10_000) as u128))
            / 10_000;
        return Ok(((output_reserve as u128).saturating_mul(net_in)
            / (input_reserve as u128).saturating_add(net_in))
        .min(u64::MAX as u128) as u64);
    }
    if let Some(expected) = pool.expected_out {
        match pool.quoted_amount_in {
            Some(qin) if qin == amount_in => return Ok(expected),
            Some(qin) => {
                return Err(anyhow!(
                    "Meteora DAMM V2 expected_out quoted for {qin}, got amount_in {amount_in}"
                ));
            }
            None => {
                return Err(anyhow!(
                    "Meteora DAMM V2 expected_out fallback needs quoted_amount_in"
                ));
            }
        }
    }
    Err(anyhow!(
        "Meteora DAMM V2 needs expected_out or non-zero reserves"
    ))
}

fn snapshot_expected_out(
    expected_out: Option<u64>,
    quoted_amount_in: Option<u64>,
    amount_in: u64,
    dex: &str,
) -> Result<u64> {
    let expected = expected_out.ok_or_else(|| {
        anyhow!("{dex} snapshot has no expected_out; provide TradeOpts::with_min_out")
    })?;
    match quoted_amount_in {
        Some(qin) if qin == amount_in => Ok(expected),
        Some(qin) => Err(anyhow!(
            "{dex} expected_out quoted for {qin}, got amount_in {amount_in}"
        )),
        None => Err(anyhow!(
            "{dex} snapshot missing quoted_amount_in; provide TradeOpts::with_min_out"
        )),
    }
}

pub fn raydium_clmm_out(pool: &RaydiumClmmPool, amount_in: u64) -> Result<u64> {
    snapshot_expected_out(pool.expected_out, pool.quoted_amount_in, amount_in, "Raydium CLMM")
}

pub fn whirlpool_out(pool: &WhirlpoolPool, amount_in: u64) -> Result<u64> {
    snapshot_expected_out(pool.expected_out, pool.quoted_amount_in, amount_in, "Orca Whirlpool")
}

pub fn meteora_dlmm_out(pool: &MeteoraDlmmPool, amount_in: u64) -> Result<u64> {
    snapshot_expected_out(pool.expected_out, pool.quoted_amount_in, amount_in, "Meteora DLMM")
}
