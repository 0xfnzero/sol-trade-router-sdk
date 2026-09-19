//! Convert filled `sol-parser-sdk` events into router market snapshots.
//!
//! Bots feed `DexEvent` from [sol-parser-sdk](https://github.com/0xfnzero/sol-parser-sdk)
//! (gRPC / ShredStream / RPC parsers). Account fields must already be filled
//! (`fill_accounts`); CLMM / Whirlpool / DLMM still need tick/bin arrays from the
//! swap instruction remaining accounts (or a local cache merge).

use sol_parser_sdk::core::events::{
    normalize_pumpfun_quote_mint, DexEvent, MeteoraDammV2SwapEvent, MeteoraDlmmSwapEvent,
    OrcaWhirlpoolAccountEvent, OrcaWhirlpoolSwapEvent, PumpFunTradeEvent, PumpSwapBuyEvent,
    PumpSwapSellEvent, RaydiumAmmV4SwapEvent, RaydiumClmmPoolStateAccountEvent,
    RaydiumClmmSwapEvent, RaydiumCpmmPoolStateAccountEvent, RaydiumCpmmSwapEvent,
    RaydiumLaunchlabTradeEvent,
};
use solana_sdk::pubkey::Pubkey;

use crate::{
    constants::{
        PUMPFUN_BUYBACK_FEE_RECIPIENT, PUMPFUN_EVENT_AUTHORITY, PUMPFUN_FEE_CONFIG,
        PUMPFUN_FEE_PROGRAM, PUMPFUN_GLOBAL, PUMPFUN_GLOBAL_VOLUME_ACCUMULATOR, TOKEN_PROGRAM,
        WSOL_MINT,
    },
    market::{
        launchlab_platform_associated_account, CpmmPool, LaunchLabPool, Market, MeteoraDammV2Pool,
        MeteoraDlmmPool, PumpFunPool, PumpSwapPool, RaydiumAmmV4Pool, RaydiumClmmPool,
        WhirlpoolPool,
    },
    transfer_fee::TokenTransferFee,
};

/// Best-effort market snapshot from a single parser event.
///
/// Returns `None` for non-trade / incomplete events. Concentrated-liquidity
/// snapshots include `quoted_amount_in` + `expected_out` from the observed swap
/// so the hot path can bind slippage without a second quote.
///
/// Does **not** run wild-pool checks — use [`market_from_dex_event_checked`] when
/// feeding aggregator / untrusted event streams.
pub fn market_from_dex_event(event: &DexEvent) -> Option<Market> {
    match event {
        DexEvent::PumpFunTrade(e)
        | DexEvent::PumpFunBuy(e)
        | DexEvent::PumpFunSell(e)
        | DexEvent::PumpFunBuyExactSolIn(e) => Some(Market::PumpFunInner(pumpfun_from_trade(e))),
        DexEvent::PumpSwapBuy(e) => Some(Market::PumpSwapOuter(pumpswap_from_buy(e))),
        DexEvent::PumpSwapSell(e) => Some(Market::PumpSwapOuter(pumpswap_from_sell(e))),
        DexEvent::RaydiumLaunchlabTrade(e) => Some(Market::LaunchLabInner(launchlab_from_trade(e)?)),
        DexEvent::RaydiumCpmmSwap(e) => Some(Market::CpmmOuter(cpmm_from_swap(e)?)),
        DexEvent::RaydiumCpmmPoolStateAccount(e) => {
            Some(Market::CpmmOuter(cpmm_from_pool_state(e)))
        }
        DexEvent::RaydiumClmmSwap(e) => Some(Market::RaydiumClmm(clmm_from_swap(e)?)),
        DexEvent::RaydiumClmmPoolStateAccount(e) => {
            Some(Market::RaydiumClmm(clmm_from_pool_state(e)))
        }
        DexEvent::OrcaWhirlpoolSwap(e) => Some(Market::Whirlpool(whirlpool_from_swap(e)?)),
        DexEvent::OrcaWhirlpoolAccount(e) => Some(Market::Whirlpool(whirlpool_from_account(e))),
        DexEvent::MeteoraDlmmSwap(e) => Some(Market::MeteoraDlmm(dlmm_from_swap(e)?)),
        DexEvent::MeteoraDammV2Swap(e) => Some(Market::MeteoraDammV2(damm_v2_from_swap(e)?)),
        DexEvent::RaydiumAmmV4Swap(e) => Some(Market::RaydiumAmmV4(amm_v4_from_swap(e)?)),
        _ => None,
    }
}

/// Like [`market_from_dex_event`], but drops snapshots that fail [`crate::pool_guard`].
pub fn market_from_dex_event_checked(
    event: &DexEvent,
    policy: &crate::pool_guard::PoolGuardPolicy,
) -> Option<Market> {
    let market = market_from_dex_event(event)?;
    crate::pool_guard::assert_market_ok(&market, policy).ok()?;
    Some(market)
}

#[inline]
fn or_default(pk: Pubkey, fallback: Pubkey) -> Pubkey {
    if pk == Pubkey::default() {
        fallback
    } else {
        pk
    }
}

#[inline]
fn tp_or_spl(pk: Pubkey) -> Pubkey {
    or_default(pk, TOKEN_PROGRAM)
}

pub fn pumpfun_from_trade(e: &PumpFunTradeEvent) -> PumpFunPool {
    let quote_mint = {
        let q = normalize_pumpfun_quote_mint(e.quote_mint);
        // Solscan sentinel → WSOL for router native-SOL settlement.
        if q.to_string() == "So11111111111111111111111111111111111111111" {
            WSOL_MINT
        } else {
            q
        }
    };
    let use_v2 = matches!(
        e.ix_name.as_str(),
        "buy_v2" | "sell_v2" | "buy_exact_quote_in_v2"
    ) || quote_mint != WSOL_MINT;
    let virtual_sol = if e.virtual_quote_reserves != 0 {
        e.virtual_quote_reserves
    } else {
        e.virtual_sol_reserves
    };
    PumpFunPool {
        mint: e.mint,
        mint_token_program: tp_or_spl(e.token_program),
        quote_mint,
        quote_token_program: tp_or_spl(e.quote_token_program),
        use_v2,
        bonding_curve: e.bonding_curve,
        associated_bonding_curve: e.associated_bonding_curve,
        creator_vault: e.creator_vault,
        fee_recipient: e.fee_recipient,
        buyback_fee_recipient: or_default(e.buyback_fee_recipient, PUMPFUN_BUYBACK_FEE_RECIPIENT),
        global: or_default(e.global, PUMPFUN_GLOBAL),
        event_authority: or_default(e.event_authority, PUMPFUN_EVENT_AUTHORITY),
        global_volume_accumulator: or_default(
            e.global_volume_accumulator,
            PUMPFUN_GLOBAL_VOLUME_ACCUMULATOR,
        ),
        user_volume_accumulator: e.user_volume_accumulator,
        fee_config: or_default(e.fee_config, PUMPFUN_FEE_CONFIG),
        fee_program: or_default(e.fee_program, PUMPFUN_FEE_PROGRAM),
        bonding_curve_v2: e.bonding_curve_v2,
        protocol_fee_recipient: e.fee_recipient,
        virtual_token_reserves: e.virtual_token_reserves,
        virtual_sol_reserves: virtual_sol,
        real_token_reserves: e.real_token_reserves,
        protocol_fee_bps: e.fee_basis_points,
        has_creator: e.creator != Pubkey::default() && e.creator_fee_basis_points > 0,
        is_cashback_coin: e.is_cashback_coin || e.cashback_fee_basis_points > 0,
    }
}

pub fn pumpswap_from_buy(e: &PumpSwapBuyEvent) -> PumpSwapPool {
    PumpSwapPool {
        pool: e.pool,
        base_mint: e.base_mint,
        quote_mint: e.quote_mint,
        pool_base_token_account: e.pool_base_token_account,
        pool_quote_token_account: e.pool_quote_token_account,
        base_token_program: tp_or_spl(e.base_token_program),
        quote_token_program: tp_or_spl(e.quote_token_program),
        coin_creator_vault_ata: e.coin_creator_vault_ata,
        coin_creator_vault_authority: e.coin_creator_vault_authority,
        coin_creator: e.coin_creator,
        base_reserve: e.pool_base_token_reserves,
        quote_reserve: e.pool_quote_token_reserves,
        virtual_quote_reserves: e.virtual_quote_reserves,
        lp_fee_bps: e.lp_fee_basis_points,
        protocol_fee_bps: e.protocol_fee_basis_points,
        creator_fee_bps: e.coin_creator_fee_basis_points,
        is_cashback_coin: e.cashback_fee_basis_points > 0,
        protocol_fee_recipient: e.protocol_fee_recipient,
        buyback_fee_recipient: or_default(e.fee_recipient, PUMPFUN_BUYBACK_FEE_RECIPIENT),
    }
}

pub fn pumpswap_from_sell(e: &PumpSwapSellEvent) -> PumpSwapPool {
    PumpSwapPool {
        pool: e.pool,
        base_mint: e.base_mint,
        quote_mint: e.quote_mint,
        pool_base_token_account: e.pool_base_token_account,
        pool_quote_token_account: e.pool_quote_token_account,
        base_token_program: tp_or_spl(e.base_token_program),
        quote_token_program: tp_or_spl(e.quote_token_program),
        coin_creator_vault_ata: e.coin_creator_vault_ata,
        coin_creator_vault_authority: e.coin_creator_vault_authority,
        coin_creator: e.coin_creator,
        base_reserve: e.pool_base_token_reserves,
        quote_reserve: e.pool_quote_token_reserves,
        virtual_quote_reserves: e.virtual_quote_reserves,
        lp_fee_bps: e.lp_fee_basis_points,
        protocol_fee_bps: e.protocol_fee_basis_points,
        creator_fee_bps: e.coin_creator_fee_basis_points,
        is_cashback_coin: e.cashback_fee_basis_points > 0,
        protocol_fee_recipient: e.protocol_fee_recipient,
        buyback_fee_recipient: or_default(e.fee_recipient, PUMPFUN_BUYBACK_FEE_RECIPIENT),
    }
}

pub fn launchlab_from_trade(e: &RaydiumLaunchlabTradeEvent) -> Option<LaunchLabPool> {
    if e.pool_state == Pubkey::default() || e.base_mint == Pubkey::default() {
        return None;
    }
    let platform = e.platform_config;
    let creator = e.creator_associated_account; // may be empty — derive below
    let platform_aa = if e.platform_associated_account == Pubkey::default() {
        launchlab_platform_associated_account(&platform, &e.quote_mint).unwrap_or_default()
    } else {
        e.platform_associated_account
    };
    // Creator AA needs creator pubkey; trade event stores associated account when filled.
    let creator_aa = if creator != Pubkey::default() {
        creator
    } else {
        Pubkey::default()
    };
    Some(LaunchLabPool {
        base_mint: e.base_mint,
        quote_mint: or_default(e.quote_mint, WSOL_MINT),
        base_token_program: tp_or_spl(e.base_token_program),
        quote_token_program: tp_or_spl(e.quote_token_program),
        pool_state: e.pool_state,
        base_vault: e.base_vault,
        quote_vault: e.quote_vault,
        platform_config: platform,
        platform_associated_account: platform_aa,
        creator_associated_account: creator_aa,
        global_config: e.global_config,
        virtual_base: e.virtual_base as u128,
        virtual_quote: e.virtual_quote as u128,
        real_base: e.real_base_after as u128,
        real_quote: e.real_quote_after as u128,
        total_base_sell: e.total_base_sell as u128,
        curve_type: 0,
        trade_fee_rate: 0,
        platform_fee_rate: 0,
        creator_fee_rate: 0,
        base_transfer_fee: TokenTransferFee::default(),
        quote_transfer_fee: TokenTransferFee::default(),
    })
}

pub fn cpmm_from_swap(e: &RaydiumCpmmSwapEvent) -> Option<CpmmPool> {
    if e.pool_id == Pubkey::default() {
        return None;
    }
    let (base_mint, quote_mint, base_vault, quote_vault, base_tp, quote_tp, base_res, quote_res) =
        if e.base_input {
            (
                e.input_token_mint,
                e.output_token_mint,
                e.input_vault,
                e.output_vault,
                e.input_token_program,
                e.output_token_program,
                e.input_vault_before.saturating_add(e.input_amount),
                e.output_vault_before.saturating_sub(e.output_amount),
            )
        } else {
            (
                e.output_token_mint,
                e.input_token_mint,
                e.output_vault,
                e.input_vault,
                e.output_token_program,
                e.input_token_program,
                e.output_vault_before.saturating_sub(e.output_amount),
                e.input_vault_before.saturating_add(e.input_amount),
            )
        };
    Some(CpmmPool {
        pool_state: e.pool_id,
        amm_config: e.amm_config,
        observation_state: e.observation_state,
        base_mint,
        quote_mint,
        base_vault,
        quote_vault,
        base_token_program: tp_or_spl(base_tp),
        quote_token_program: tp_or_spl(quote_tp),
        base_reserve: base_res,
        quote_reserve: quote_res,
        trade_fee_rate: 0,
        creator_fee_rate: 0,
        creator_fee_on: 0,
        enable_creator_fee: false,
        base_transfer_fee: TokenTransferFee::default(),
        quote_transfer_fee: TokenTransferFee::default(),
    })
}

pub fn cpmm_from_pool_state(e: &RaydiumCpmmPoolStateAccountEvent) -> CpmmPool {
    let s = &e.pool_state;
    CpmmPool {
        pool_state: e.pubkey,
        amm_config: s.amm_config,
        observation_state: s.observation_key,
        base_mint: s.token_0_mint,
        quote_mint: s.token_1_mint,
        base_vault: s.token_0_vault,
        quote_vault: s.token_1_vault,
        base_token_program: TOKEN_PROGRAM,
        quote_token_program: TOKEN_PROGRAM,
        base_reserve: 0,
        quote_reserve: 0,
        trade_fee_rate: 0,
        creator_fee_rate: 0,
        creator_fee_on: 0,
        enable_creator_fee: false,
        base_transfer_fee: TokenTransferFee::default(),
        quote_transfer_fee: TokenTransferFee::default(),
    }
}

/// Best-effort token program for a known mint without RPC.
/// Classic quote mints are always SPL Token; others default SPL until overlay.
#[inline]
pub fn known_mint_token_program(mint: Pubkey) -> Pubkey {
    let _ = mint; // reserved for future known Token-2022 allowlists
    TOKEN_PROGRAM
}

/// Overlay Token-2022 / SPL programs onto a CLMM snapshot (call after mint owners are known).
#[inline]
pub fn clmm_apply_token_programs(
    pool: &mut RaydiumClmmPool,
    token_0_program: Pubkey,
    token_1_program: Pubkey,
) {
    pool.token_0_program = tp_or_spl(token_0_program);
    pool.token_1_program = tp_or_spl(token_1_program);
}

pub fn clmm_from_swap(e: &RaydiumClmmSwapEvent) -> Option<RaydiumClmmPool> {
    if e.pool_state == Pubkey::default() || e.tick_arrays.is_empty() {
        return None;
    }
    let (token_0_mint, token_1_mint, token_0_vault, token_1_vault) = if e.zero_for_one {
        (e.input_mint, e.output_mint, e.input_vault, e.output_vault)
    } else {
        (e.output_mint, e.input_mint, e.output_vault, e.input_vault)
    };
    let amount_in = if e.zero_for_one { e.amount_0 } else { e.amount_1 };
    let amount_out = if e.zero_for_one { e.amount_1 } else { e.amount_0 };
    // CLMM swap events do not carry per-mint token programs. Heuristic: a non-zero
    // transfer_fee on a side strongly implies Token-2022 for that mint; otherwise
    // default SPL. Bots with mint-owner cache should call [`clmm_apply_token_programs`].
    let token_0_program = if e.transfer_fee_0 > 0 {
        crate::constants::TOKEN_2022_PROGRAM
    } else {
        known_mint_token_program(token_0_mint)
    };
    let token_1_program = if e.transfer_fee_1 > 0 {
        crate::constants::TOKEN_2022_PROGRAM
    } else {
        known_mint_token_program(token_1_mint)
    };
    Some(RaydiumClmmPool {
        amm_config: e.amm_config,
        pool_state: e.pool_state,
        observation_state: e.observation_state,
        token_0_mint,
        token_1_mint,
        token_0_vault,
        token_1_vault,
        token_0_program,
        token_1_program,
        tick_arrays: e.tick_arrays.clone(),
        tick_array_bitmap_extension: e.tick_array_bitmap_extension,
        quoted_amount_in: Some(amount_in).filter(|&a| a > 0),
        expected_out: Some(amount_out).filter(|&a| a > 0),
        fee_bps: 0,
    })
}

pub fn clmm_from_pool_state(e: &RaydiumClmmPoolStateAccountEvent) -> RaydiumClmmPool {
    let s = &e.pool_state;
    RaydiumClmmPool {
        amm_config: s.amm_config,
        pool_state: e.pubkey,
        observation_state: s.observation_key,
        token_0_mint: s.token_mint_0,
        token_1_mint: s.token_mint_1,
        token_0_vault: s.token_vault_0,
        token_1_vault: s.token_vault_1,
        token_0_program: known_mint_token_program(s.token_mint_0),
        token_1_program: known_mint_token_program(s.token_mint_1),
        tick_arrays: Vec::new(),
        tick_array_bitmap_extension: None,
        quoted_amount_in: None,
        expected_out: None,
        fee_bps: 0,
    }
}

pub fn whirlpool_from_swap(e: &OrcaWhirlpoolSwapEvent) -> Option<WhirlpoolPool> {
    if e.whirlpool == Pubkey::default() {
        return None;
    }
    let ticks = [e.tick_array_0, e.tick_array_1, e.tick_array_2];
    if ticks.iter().any(|t| *t == Pubkey::default()) {
        return None;
    }
    Some(WhirlpoolPool {
        whirlpool: e.whirlpool,
        mint_a: e.token_mint_a,
        mint_b: e.token_mint_b,
        vault_a: e.token_vault_a,
        vault_b: e.token_vault_b,
        token_program_a: tp_or_spl(e.token_program_a),
        token_program_b: tp_or_spl(e.token_program_b),
        tick_arrays: ticks.to_vec(),
        quoted_amount_in: Some(e.input_amount).filter(|&a| a > 0),
        expected_out: Some(e.output_amount).filter(|&a| a > 0),
        fee_bps: 0,
    })
}

pub fn whirlpool_from_account(e: &OrcaWhirlpoolAccountEvent) -> WhirlpoolPool {
    let w = &e.whirlpool;
    WhirlpoolPool {
        whirlpool: e.pubkey,
        mint_a: w.token_mint_a,
        mint_b: w.token_mint_b,
        vault_a: w.token_vault_a,
        vault_b: w.token_vault_b,
        token_program_a: TOKEN_PROGRAM,
        token_program_b: TOKEN_PROGRAM,
        tick_arrays: Vec::new(),
        quoted_amount_in: None,
        expected_out: None,
        fee_bps: (w.fee_rate / 100) as u16, // hundredths of a bip → bps
    }
}

/// Merge swap-instruction tick arrays / quote into an account-state snapshot.
pub fn merge_whirlpool_swap(pool: &mut WhirlpoolPool, e: &OrcaWhirlpoolSwapEvent) {
    if e.tick_array_0 != Pubkey::default() {
        pool.tick_arrays = vec![e.tick_array_0, e.tick_array_1, e.tick_array_2];
    }
    if e.token_vault_a != Pubkey::default() {
        pool.vault_a = e.token_vault_a;
        pool.vault_b = e.token_vault_b;
    }
    if e.token_mint_a != Pubkey::default() {
        pool.mint_a = e.token_mint_a;
        pool.mint_b = e.token_mint_b;
    }
    if e.token_program_a != Pubkey::default() {
        pool.token_program_a = e.token_program_a;
        pool.token_program_b = e.token_program_b;
    }
    if e.input_amount > 0 {
        pool.quoted_amount_in = Some(e.input_amount);
        pool.expected_out = Some(e.output_amount);
    }
}

pub fn merge_clmm_swap(pool: &mut RaydiumClmmPool, e: &RaydiumClmmSwapEvent) {
    if !e.tick_arrays.is_empty() {
        pool.tick_arrays = e.tick_arrays.clone();
    }
    if e.tick_array_bitmap_extension.is_some() {
        pool.tick_array_bitmap_extension = e.tick_array_bitmap_extension;
    }
    if e.amm_config != Pubkey::default() {
        pool.amm_config = e.amm_config;
    }
    if e.observation_state != Pubkey::default() {
        pool.observation_state = e.observation_state;
    }
    let amount_in = if e.zero_for_one { e.amount_0 } else { e.amount_1 };
    let amount_out = if e.zero_for_one { e.amount_1 } else { e.amount_0 };
    if amount_in > 0 {
        pool.quoted_amount_in = Some(amount_in);
        pool.expected_out = Some(amount_out);
    }
}

pub fn dlmm_from_swap(e: &MeteoraDlmmSwapEvent) -> Option<MeteoraDlmmPool> {
    if e.pool == Pubkey::default() || e.bin_arrays.is_empty() {
        return None;
    }
    Some(MeteoraDlmmPool {
        lb_pair: e.pool,
        bitmap_extension: e.bitmap_extension,
        reserve_x: e.reserve_x,
        reserve_y: e.reserve_y,
        token_x_mint: e.token_x_mint,
        token_y_mint: e.token_y_mint,
        token_x_program: tp_or_spl(e.token_x_program),
        token_y_program: tp_or_spl(e.token_y_program),
        oracle: e.oracle,
        bin_arrays: e.bin_arrays.clone(),
        quoted_amount_in: Some(e.amount_in).filter(|&a| a > 0),
        expected_out: Some(e.amount_out).filter(|&a| a > 0),
        fee_bps: (e.fee_bps.min(u128::from(u16::MAX))) as u16,
    })
}

pub fn damm_v2_from_swap(e: &MeteoraDammV2SwapEvent) -> Option<MeteoraDammV2Pool> {
    if e.pool == Pubkey::default() || e.token_a_mint == Pubkey::default() {
        return None;
    }
    Some(MeteoraDammV2Pool {
        pool: e.pool,
        token_a_vault: e.token_a_vault,
        token_b_vault: e.token_b_vault,
        token_a_mint: e.token_a_mint,
        token_b_mint: e.token_b_mint,
        token_a_program: tp_or_spl(e.token_a_program),
        token_b_program: tp_or_spl(e.token_b_program),
        token_a_reserve: e.reserve_a_amount,
        token_b_reserve: e.reserve_b_amount,
        fee_bps: 0,
        quoted_amount_in: Some(e.amount_in).filter(|&a| a > 0),
        expected_out: Some(e.output_amount).filter(|&a| a > 0),
        swap_mode: e.swap_mode,
        referral_token_account: e.referral_token_account,
        include_rate_limiter_sysvar: false,
    })
}

pub fn amm_v4_from_swap(e: &RaydiumAmmV4SwapEvent) -> Option<RaydiumAmmV4Pool> {
    if e.amm == Pubkey::default() || e.pool_coin_token_account == Pubkey::default() {
        return None;
    }
    Some(RaydiumAmmV4Pool {
        amm: e.amm,
        // SwapBaseInV2 ix has no mint accounts — fill from vault mints / AmmInfo cache.
        coin_mint: Pubkey::default(),
        pc_mint: Pubkey::default(),
        token_coin: e.pool_coin_token_account,
        token_pc: e.pool_pc_token_account,
        token_program: tp_or_spl(e.token_program),
        amm_open_orders: e.amm_open_orders,
        amm_target_orders: e.amm_target_orders.unwrap_or_default(),
        serum_program: e.serum_program,
        serum_market: e.serum_market,
        serum_bids: e.serum_bids,
        serum_asks: e.serum_asks,
        serum_event_queue: e.serum_event_queue,
        serum_coin_vault_account: e.serum_coin_vault_account,
        serum_pc_vault_account: e.serum_pc_vault_account,
        serum_vault_signer: e.serum_vault_signer,
        coin_reserve: 0,
        pc_reserve: 0,
        trade_fee_numerator: 25,
        swap_fee_numerator: 25,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sol_parser_sdk::core::events::EventMetadata;

    #[test]
    fn whirlpool_swap_requires_three_tick_arrays() {
        let mut e = OrcaWhirlpoolSwapEvent {
            metadata: EventMetadata::default(),
            whirlpool: Pubkey::new_unique(),
            input_amount: 100,
            output_amount: 90,
            a_to_b: true,
            ..Default::default()
        };
        assert!(whirlpool_from_swap(&e).is_none());
        e.tick_array_0 = Pubkey::new_unique();
        e.tick_array_1 = Pubkey::new_unique();
        e.tick_array_2 = Pubkey::new_unique();
        e.token_mint_a = WSOL_MINT;
        e.token_mint_b = Pubkey::new_unique();
        e.token_vault_a = Pubkey::new_unique();
        e.token_vault_b = Pubkey::new_unique();
        let pool = whirlpool_from_swap(&e).unwrap();
        assert_eq!(pool.tick_arrays.len(), 3);
        assert_eq!(pool.quoted_amount_in, Some(100));
        assert_eq!(pool.expected_out, Some(90));
    }

    #[test]
    fn pumpfun_solscan_sentinel_maps_to_wsol() {
        let mut e = PumpFunTradeEvent::default();
        e.mint = Pubkey::new_unique();
        e.bonding_curve = Pubkey::new_unique();
        e.associated_bonding_curve = Pubkey::new_unique();
        e.quote_mint = sol_parser_sdk::core::events::PUMPFUN_SOLSCAN_SOL_QUOTE_MINT;
        e.virtual_sol_reserves = 1_000;
        e.virtual_token_reserves = 2_000;
        let pool = pumpfun_from_trade(&e);
        assert_eq!(pool.quote_mint, WSOL_MINT);
        assert!(pool.is_native_sol_quote());
    }
}
