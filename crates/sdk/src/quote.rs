//! Local quotes aligned with sol-trade-sdk / on-chain math (no RPC).

use anyhow::{anyhow, Result};

use crate::{
    market::{CpmmPool, LaunchLabPool, PumpFunPool},
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
        return Err(anyhow!("unsupported LaunchLab curve_type {}", pool.curve_type));
    }
    let total_fee_rate = launchlab_total_fee_rate(pool, share_fee_rate)?;
    let quote_xfer = pool.quote_transfer_fee.calculate(amount_in);
    let vault_input = amount_in
        .checked_sub(quote_xfer)
        .ok_or_else(|| anyhow!("LaunchLab quote transfer fee exceeds input"))? as u128;
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
        return Err(anyhow!("unsupported LaunchLab curve_type {}", pool.curve_type));
    }
    let base_xfer = pool.base_transfer_fee.calculate(base_in);
    let curve_input = base_in
        .checked_sub(base_xfer)
        .ok_or_else(|| anyhow!("LaunchLab base transfer fee exceeds input"))? as u128;
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
        assert_eq!(apply_slippage_min_out(1_000_000, 10_000), apply_slippage_min_out(1_000_000, 9_999));
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
            bonding_curve: dummy_pubkey(3),
            associated_bonding_curve: dummy_pubkey(4),
            creator_vault: dummy_pubkey(5),
            fee_recipient: dummy_pubkey(6),
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
    let (input_reserve, output_reserve, in_fee, out_fee): (u64, u64, TokenTransferFee, TokenTransferFee) =
        if input_is_base {
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

    let (input_less_fees, output_swapped) = if on_input {
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
            / (input_reserve as u128).saturating_add(input_less as u128)) as u64;
        let creator_fee = cpmm_trade_fee(out_swapped, creator_rate);
        (input_less, out_swapped.saturating_sub(creator_fee))
    };

    let _ = input_less_fees;
    Ok(output_swapped.saturating_sub(out_fee.calculate(output_swapped)))
}

// ── PumpFun ────────────────────────────────────────────────────────────────

/// Protocol 95 bps + optional creator 30 bps (sol-trade-sdk).
#[inline(always)]
pub fn pumpfun_total_fee_bps(has_creator: bool) -> u64 {
    95 + if has_creator { 30 } else { 0 }
}

/// PumpFun buy — fee is **additive** (`amount * 10000 / (10000 + fee_bps)`).
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
    if input == 0 {
        return 0;
    }
    let vtok = pool.virtual_token_reserves as u128;
    let vsol = pool.virtual_sol_reserves as u128;
    let denom = vsol.saturating_add(input);
    if denom == 0 {
        return 0;
    }
    let tokens = input
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
