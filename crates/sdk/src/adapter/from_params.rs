//! Convert `sol-trade-sdk` [`DexParamEnum`] / per-DEX params → router pool snapshots.

use anyhow::{anyhow, Result};
use solana_sdk::pubkey::Pubkey;
use sol_trade_sdk::instruction::utils::pumpfun::{
    get_bonding_curve_v2_pda, get_user_volume_accumulator_pda,
};
use sol_trade_sdk::trading::core::params::{
    BonkParams, DexParamEnum, MeteoraDammV2Params, MeteoraDlmmParams, PumpFunParams,
    PumpSwapParams, RaydiumAmmV4Params, RaydiumClmmParams, RaydiumCpmmParams, StonkFunMemeLeg,
    StonkFunSolHop, StonkFunViaSolParams, WhirlpoolParams,
};

use crate::{
    constants::{
        PUMPFUN_BUYBACK_FEE_RECIPIENT, PUMPFUN_EVENT_AUTHORITY, PUMPFUN_FEE_CONFIG,
        PUMPFUN_FEE_PROGRAM, PUMPFUN_GLOBAL, PUMPFUN_GLOBAL_VOLUME_ACCUMULATOR, TOKEN_PROGRAM,
        WSOL_MINT,
    },
    market::{
        CpmmPool, LaunchLabPool, Market, MeteoraDammV2Pool, MeteoraDlmmPool, PumpFunPool,
        PumpSwapPool, RaydiumAmmV4Pool, RaydiumClmmPool, RoutedMarket, WhirlpoolPool,
    },
    transfer_fee::TokenTransferFee,
};

/// Map trade-sdk transfer fee into router [`TokenTransferFee`].
pub trait TransferFeeConvert {
    fn to_router(&self) -> TokenTransferFee;
}

impl TransferFeeConvert for sol_trade_sdk::trading::core::params::TokenTransferFee {
    #[inline]
    fn to_router(&self) -> TokenTransferFee {
        TokenTransferFee {
            basis_points: self.basis_points,
            maximum_fee: self.maximum_fee,
        }
    }
}

#[inline]
fn quote_or_wsol(quote_mint: Pubkey) -> Pubkey {
    if quote_mint == Pubkey::default() {
        WSOL_MINT
    } else {
        quote_mint
    }
}

#[inline]
fn token_program_or_spl(tp: Pubkey) -> Pubkey {
    if tp == Pubkey::default() {
        TOKEN_PROGRAM
    } else {
        tp
    }
}

/// Convert LaunchLab / Bonk / StonkFun curve params.
pub fn bonk_to_launchlab(p: &BonkParams, base_mint: Pubkey) -> LaunchLabPool {
    let quote_mint = quote_or_wsol(p.quote_mint);
    LaunchLabPool {
        base_mint,
        quote_mint,
        base_token_program: token_program_or_spl(p.mint_token_program),
        quote_token_program: token_program_or_spl(p.quote_token_program),
        pool_state: p.pool_state,
        base_vault: p.base_vault,
        quote_vault: p.quote_vault,
        platform_config: p.platform_config,
        platform_associated_account: p.platform_associated_account,
        creator_associated_account: p.creator_associated_account,
        global_config: p.global_config,
        virtual_base: p.virtual_base,
        virtual_quote: p.virtual_quote,
        real_base: p.real_base,
        real_quote: p.real_quote,
        total_base_sell: p.total_base_sell,
        curve_type: p.curve_type,
        trade_fee_rate: p.trade_fee_rate,
        platform_fee_rate: p.platform_fee_rate,
        creator_fee_rate: p.creator_fee_rate,
        base_transfer_fee: p.base_transfer_fee.to_router(),
        quote_transfer_fee: p.quote_transfer_fee.to_router(),
    }
}

pub fn cpmm_from_params(p: &RaydiumCpmmParams) -> CpmmPool {
    CpmmPool {
        pool_state: p.pool_state,
        amm_config: p.amm_config,
        observation_state: p.observation_state,
        base_mint: p.base_mint,
        quote_mint: p.quote_mint,
        base_vault: p.base_vault,
        quote_vault: p.quote_vault,
        base_token_program: token_program_or_spl(p.base_token_program),
        quote_token_program: token_program_or_spl(p.quote_token_program),
        base_reserve: p.base_reserve,
        quote_reserve: p.quote_reserve,
        trade_fee_rate: p.trade_fee_rate,
        creator_fee_rate: p.creator_fee_rate,
        creator_fee_on: p.creator_fee_on,
        enable_creator_fee: p.enable_creator_fee,
        base_transfer_fee: p.base_transfer_fee.to_router(),
        quote_transfer_fee: p.quote_transfer_fee.to_router(),
    }
}

pub fn pumpfun_from_params(
    p: &PumpFunParams,
    mint: Pubkey,
    user: &Pubkey,
) -> Result<PumpFunPool> {
    let bc = p.bonding_curve.as_ref();
    if mint == Pubkey::default() {
        return Err(anyhow!("PumpFun mint is default"));
    }
    let bonding_curve = if bc.account == Pubkey::default() {
        return Err(anyhow!("PumpFunParams bonding_curve.account is default"));
    } else {
        bc.account
    };
    let quote_mint = quote_or_wsol(if p.quote_mint == Pubkey::default() {
        bc.effective_quote_mint()
    } else {
        p.quote_mint
    });
    let use_v2 = quote_mint != WSOL_MINT;
    let bonding_curve_v2 = get_bonding_curve_v2_pda(&mint).unwrap_or_default();
    let user_volume_accumulator = get_user_volume_accumulator_pda(user).unwrap_or_default();
    Ok(PumpFunPool {
        mint,
        mint_token_program: token_program_or_spl(p.token_program),
        quote_mint,
        quote_token_program: TOKEN_PROGRAM,
        use_v2,
        bonding_curve,
        associated_bonding_curve: p.associated_bonding_curve,
        creator_vault: p.creator_vault,
        fee_recipient: p.fee_recipient,
        buyback_fee_recipient: PUMPFUN_BUYBACK_FEE_RECIPIENT,
        global: PUMPFUN_GLOBAL,
        event_authority: PUMPFUN_EVENT_AUTHORITY,
        global_volume_accumulator: PUMPFUN_GLOBAL_VOLUME_ACCUMULATOR,
        user_volume_accumulator,
        fee_config: PUMPFUN_FEE_CONFIG,
        fee_program: PUMPFUN_FEE_PROGRAM,
        bonding_curve_v2,
        protocol_fee_recipient: p.fee_recipient,
        virtual_token_reserves: bc.virtual_token_reserves,
        virtual_sol_reserves: bc.virtual_sol_reserves,
        real_token_reserves: bc.real_token_reserves,
        protocol_fee_bps: 0,
        has_creator: bc.creator != Pubkey::default(),
        is_cashback_coin: bc.is_cashback_coin,
    })
}

pub fn pumpswap_from_params(p: &PumpSwapParams) -> PumpSwapPool {
    let fees = &p.fee_basis_points;
    PumpSwapPool {
        pool: p.pool,
        base_mint: p.base_mint,
        quote_mint: p.quote_mint,
        pool_base_token_account: p.pool_base_token_account,
        pool_quote_token_account: p.pool_quote_token_account,
        base_token_program: token_program_or_spl(p.base_token_program),
        quote_token_program: token_program_or_spl(p.quote_token_program),
        coin_creator_vault_ata: p.coin_creator_vault_ata,
        coin_creator_vault_authority: p.coin_creator_vault_authority,
        coin_creator: p.coin_creator,
        base_reserve: p.pool_base_token_reserves,
        quote_reserve: p.pool_quote_token_reserves,
        virtual_quote_reserves: p.virtual_quote_reserves,
        lp_fee_bps: fees.lp_fee_basis_points,
        protocol_fee_bps: fees.protocol_fee_basis_points,
        creator_fee_bps: fees.coin_creator_fee_basis_points,
        is_cashback_coin: p.is_cashback_coin,
        protocol_fee_recipient: sol_trade_sdk::instruction::utils::pumpswap::accounts::PROTOCOL_FEE_RECIPIENT,
        buyback_fee_recipient: PUMPFUN_BUYBACK_FEE_RECIPIENT,
    }
}

pub fn raydium_amm_v4_from_params(p: &RaydiumAmmV4Params) -> RaydiumAmmV4Pool {
    RaydiumAmmV4Pool {
        amm: p.amm,
        coin_mint: p.coin_mint,
        pc_mint: p.pc_mint,
        token_coin: p.token_coin,
        token_pc: p.token_pc,
        amm_open_orders: p.amm_open_orders,
        amm_target_orders: p.amm_target_orders,
        serum_program: p.serum_program,
        serum_market: p.serum_market,
        serum_bids: p.serum_bids,
        serum_asks: p.serum_asks,
        serum_event_queue: p.serum_event_queue,
        serum_coin_vault_account: p.serum_coin_vault_account,
        serum_pc_vault_account: p.serum_pc_vault_account,
        serum_vault_signer: p.serum_vault_signer,
        coin_reserve: p.coin_reserve,
        pc_reserve: p.pc_reserve,
        trade_fee_numerator: 25,
        swap_fee_numerator: 25,
    }
}

pub fn damm_v2_from_params(p: &MeteoraDammV2Params) -> MeteoraDammV2Pool {
    MeteoraDammV2Pool {
        pool: p.pool,
        token_a_vault: p.token_a_vault,
        token_b_vault: p.token_b_vault,
        token_a_mint: p.token_a_mint,
        token_b_mint: p.token_b_mint,
        token_a_program: token_program_or_spl(p.token_a_program),
        token_b_program: token_program_or_spl(p.token_b_program),
        token_a_reserve: 0,
        token_b_reserve: 0,
        fee_bps: 0,
        quoted_amount_in: None,
        expected_out: None,
        swap_mode: p.swap_mode,
        referral_token_account: p.referral_token_account,
        include_rate_limiter_sysvar: p.include_rate_limiter_sysvar,
    }
}

pub fn raydium_clmm_from_params(p: &RaydiumClmmParams) -> RaydiumClmmPool {
    RaydiumClmmPool {
        amm_config: p.amm_config,
        pool_state: p.pool_state,
        observation_state: p.observation_state,
        token_0_mint: p.token_0_mint,
        token_1_mint: p.token_1_mint,
        token_0_vault: p.token_0_vault,
        token_1_vault: p.token_1_vault,
        token_0_program: token_program_or_spl(p.token_0_program),
        token_1_program: token_program_or_spl(p.token_1_program),
        tick_arrays: p.tick_arrays.clone(),
        tick_array_bitmap_extension: p.tick_array_bitmap_extension,
        quoted_amount_in: None,
        expected_out: None,
        fee_bps: 0,
    }
}

pub fn whirlpool_from_params(p: &WhirlpoolParams) -> WhirlpoolPool {
    WhirlpoolPool {
        whirlpool: p.whirlpool,
        mint_a: p.mint_a,
        mint_b: p.mint_b,
        vault_a: p.vault_a,
        vault_b: p.vault_b,
        token_program_a: token_program_or_spl(p.token_program_a),
        token_program_b: token_program_or_spl(p.token_program_b),
        tick_arrays: p.tick_arrays.clone(),
        quoted_amount_in: None,
        expected_out: None,
        fee_bps: 0,
    }
}

pub fn dlmm_from_params(p: &MeteoraDlmmParams) -> MeteoraDlmmPool {
    MeteoraDlmmPool {
        lb_pair: p.lb_pair,
        bitmap_extension: p.bitmap_extension,
        reserve_x: p.reserve_x,
        reserve_y: p.reserve_y,
        token_x_mint: p.token_x_mint,
        token_y_mint: p.token_y_mint,
        token_x_program: token_program_or_spl(p.token_x_program),
        token_y_program: token_program_or_spl(p.token_y_program),
        oracle: p.oracle,
        bin_arrays: p.bin_arrays.clone(),
        quoted_amount_in: None,
        expected_out: None,
        fee_bps: 0,
    }
}

fn via_sol_to_routed(via: &StonkFunViaSolParams, mint: Pubkey) -> Result<RoutedMarket> {
    let bridge = match &via.sol_hop {
        StonkFunSolHop::RaydiumCpmm(p) => cpmm_from_params(p),
        StonkFunSolHop::RaydiumAmmV4(_) => {
            return Err(anyhow!(
                "StonkFunViaSol AMM v4 SOL hop is not supported as router bridge yet; use CPMM hop"
            ));
        }
    };
    let market = match &via.meme_leg {
        StonkFunMemeLeg::Curve(p) => Market::LaunchLabInner(bonk_to_launchlab(p, mint)),
        StonkFunMemeLeg::Graduated(p) => Market::CpmmOuter(cpmm_from_params(p)),
    };
    Ok(RoutedMarket::with_bridge(market, bridge))
}

/// Convert without a user pubkey (PumpFun user volume PDA uses `Pubkey::default()`).
pub fn to_routed_market(params: &DexParamEnum, mint: Pubkey) -> Result<RoutedMarket> {
    to_routed_market_for_user(params, mint, &Pubkey::default())
}

/// Preferred converter when the payer is known (fills PumpFun user volume accumulator).
pub fn to_routed_market_for_user(
    params: &DexParamEnum,
    mint: Pubkey,
    user: &Pubkey,
) -> Result<RoutedMarket> {
    Ok(match params {
        DexParamEnum::PumpFun(p) => RoutedMarket::pumpfun(pumpfun_from_params(p, mint, user)?),
        DexParamEnum::PumpSwap(p) => RoutedMarket::pumpswap(pumpswap_from_params(p)),
        DexParamEnum::LaunchLab(p) | DexParamEnum::Bonk(p) | DexParamEnum::StonkFun(p) => {
            RoutedMarket::stonk_inner(bonk_to_launchlab(p, mint), None)
        }
        DexParamEnum::StonkFunSwap(p) | DexParamEnum::RaydiumCpmm(p) => {
            RoutedMarket::new(Market::CpmmOuter(cpmm_from_params(p)))
        }
        DexParamEnum::StonkFunViaSol(via) => via_sol_to_routed(via, mint)?,
        DexParamEnum::RaydiumAmmV4(p) => {
            RoutedMarket::raydium_amm_v4(raydium_amm_v4_from_params(p))
        }
        DexParamEnum::MeteoraDammV2(p) => RoutedMarket::meteora_damm_v2(damm_v2_from_params(p)),
        DexParamEnum::RaydiumClmm(p) => RoutedMarket::raydium_clmm(raydium_clmm_from_params(p)),
        DexParamEnum::OrcaWhirlpool(p) => RoutedMarket::whirlpool(whirlpool_from_params(p)),
        DexParamEnum::MeteoraDlmm(p) => RoutedMarket::meteora_dlmm(dlmm_from_params(p)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sol_trade_sdk::trading::core::params::RaydiumCpmmParams;

    #[test]
    fn cpmm_params_round_trip_fields() {
        let p = RaydiumCpmmParams {
            pool_state: Pubkey::new_unique(),
            amm_config: Pubkey::new_unique(),
            base_mint: WSOL_MINT,
            quote_mint: Pubkey::new_unique(),
            base_reserve: 1_000,
            quote_reserve: 2_000,
            base_vault: Pubkey::new_unique(),
            quote_vault: Pubkey::new_unique(),
            base_token_program: TOKEN_PROGRAM,
            quote_token_program: TOKEN_PROGRAM,
            observation_state: Pubkey::new_unique(),
            trade_fee_rate: 2500,
            protocol_fee_rate: 0,
            fund_fee_rate: 0,
            creator_fee_rate: 0,
            creator_fee_on: 0,
            enable_creator_fee: false,
            base_transfer_fee: Default::default(),
            quote_transfer_fee: Default::default(),
        };
        let pool = cpmm_from_params(&p);
        assert_eq!(pool.pool_state, p.pool_state);
        assert_eq!(pool.base_reserve, 1_000);
        let market = to_routed_market(&DexParamEnum::RaydiumCpmm(p.clone()), p.quote_mint).unwrap();
        assert!(matches!(market.market, Market::CpmmOuter(_)));
    }

    #[test]
    fn via_sol_builds_bridge() {
        let meme = Pubkey::new_unique();
        let stock = Pubkey::new_unique();
        let curve = BonkParams {
            pool_state: Pubkey::new_unique(),
            quote_mint: stock,
            ..Default::default()
        };
        let hop = RaydiumCpmmParams {
            pool_state: Pubkey::new_unique(),
            amm_config: Pubkey::new_unique(),
            base_mint: WSOL_MINT,
            quote_mint: stock,
            base_reserve: 10,
            quote_reserve: 20,
            base_vault: Pubkey::new_unique(),
            quote_vault: Pubkey::new_unique(),
            base_token_program: TOKEN_PROGRAM,
            quote_token_program: TOKEN_PROGRAM,
            observation_state: Pubkey::new_unique(),
            trade_fee_rate: 2500,
            protocol_fee_rate: 0,
            fund_fee_rate: 0,
            creator_fee_rate: 0,
            creator_fee_on: 0,
            enable_creator_fee: false,
            base_transfer_fee: Default::default(),
            quote_transfer_fee: Default::default(),
        };
        let via = StonkFunViaSolParams::curve_with_cpmm(curve, hop);
        let routed = to_routed_market(&DexParamEnum::StonkFunViaSol(via), meme).unwrap();
        assert!(routed.bridge.is_some());
        assert!(matches!(routed.market, Market::LaunchLabInner(_)));
    }
}
