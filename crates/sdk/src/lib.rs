//! Hot-path trading SDK for `sol-trade-router`.
//!
//! # Design
//! - **Capability parity with [`sol-trade-sdk`](https://github.com/0xfnzero/sol-trade-sdk)** —
//!   `TradingClient` / SimpleBuy|Sell / SWQoS / nonce / ALT / middleware / RiskGate /
//!   exact-out / RPC pool loaders / ViaSol. Reuses trade-sdk `swqos` / `common` /
//!   `middleware` / `TradingInfrastructure`.
//! - **Only difference**: swap legs are packed through this repo's Pinocchio
//!   `Route` CPI (not direct DEX program calls).
//! - **Ultra-low latency**: warm client + blockhash **before** subscribe; hot path
//!   must not fetch blockhash, balances, or pools. See `docs/LOW_LATENCY_BOTS.md`.
//! - **No RPC on the hot path.** Pool snapshots come from [`sol-parser-sdk`](https://github.com/0xfnzero/sol-parser-sdk)
//!   events (`market_from_dex_event`) / local cache, or from trade-sdk params via
//!   [`adapter::to_routed_market`].
//!
//! ```ignore
//! // Warm before subscribe (production):
//! let client = TradingClient::new(payer, RouterTradeConfig::new(trade_cfg, fee_recipient, fee_bps)).await;
//! // Hot path: cached blockhash + event params only
//! client.buy(buy_params).await?;
//! ```

mod adapter;
mod admin;
mod asset;
mod ata;
mod client;
mod constants;
mod legs;
mod market;
mod parser;
mod pool_guard;
mod quote;
mod route_ix;
mod trade;
mod transfer_fee;

pub use adapter::{
    bonk_to_launchlab, cpmm_from_params, damm_v2_from_params, dlmm_from_params,
    load_routed_market_by_rpc, pumpfun_from_params, pumpswap_from_params,
    raydium_amm_v4_from_params, raydium_clmm_from_params, to_routed_market,
    to_routed_market_for_user, whirlpool_from_params, LoadMarketRequest,
};
pub use admin::{initialize_config, update_config};
pub use asset::{BuyWith, SellTo};
pub use ata::{
    ata, close_ata, close_wsol_ata, create_ata, create_wsol_ata, warm_ata_cache, wrap_sol,
    wrap_sol_with_options, AtaKind, AtaPolicy,
};
pub use client::{
    validate_protocol_params, RouterTradeConfig, SolanaTrade, TradingClient,
};
pub use constants::{
    LAUNCHLAB_PROGRAM, MEMO_PROGRAM, METEORA_DAMM_V2_PROGRAM, METEORA_DLMM_PROGRAM,
    ORCA_WHIRLPOOL_PROGRAM, PROGRAM_ID, PUMPSWAP_PROGRAM, RAYDIUM_AMM_V4_PROGRAM,
    RAYDIUM_CLMM_PROGRAM, RAYDIUM_CPMM_PROGRAM, STONKFUN_REWARD_PLATFORM_CONFIG,
    STONKFUN_STANDARD_PLATFORM_CONFIG, TOKEN_2022_PROGRAM, USDC_MINT, WSOL_MINT,
    raydium_clmm_tick_array_bitmap_extension,
};
pub use market::{
    launchlab_creator_associated_account, launchlab_platform_associated_account, BridgePool,
    CpmmPool, LaunchLabPool, Market, MeteoraDammV2Pool, MeteoraDlmmPool, PumpFunPool, PumpSwapPool,
    RaydiumAmmV4Pool, RaydiumClmmPool, RoutedMarket, WhirlpoolPool,
};
pub use parser::{
    amm_v4_from_swap, clmm_apply_token_programs, clmm_from_pool_state, clmm_from_swap,
    cpmm_from_pool_state, cpmm_from_swap, damm_v2_from_swap, dlmm_from_swap, launchlab_from_trade,
    market_from_dex_event, market_from_dex_event_checked, merge_clmm_swap, merge_whirlpool_swap,
    pumpfun_from_trade, pumpswap_from_buy, pumpswap_from_sell, whirlpool_from_account,
    whirlpool_from_swap,
};
pub use pool_guard::{
    assert_cpmm_ok, assert_launchlab_ok, assert_market_ok, assert_pumpfun_ok, assert_pumpswap_ok,
    assert_routed_market_ok, cpmm_observation_pda, cpmm_pool_pda, cpmm_vault_pda, is_stonkfun_platform,
    launchlab_pool_pda, launchlab_vault_pda, pump_pool_authority_pda, pumpfun_bonding_curve_pda,
    pumpswap_canonical_pool_pda, PoolGuardPolicy,
};
pub use quote::{
    apply_slippage_min_out, clamp_slippage_bps, cpmm_out, fee_amount, launchlab_buy_base_out,
    launchlab_buy_quote, launchlab_sell_quote_out, meteora_damm_v2_out, meteora_dlmm_out,
    pumpfun_buy_token_out, pumpfun_sell_sol_out, pumpfun_total_fee_bps, pumpswap_buy_base_out,
    pumpswap_sell_quote_out, raydium_amm_v4_in_for_out, raydium_amm_v4_out, raydium_clmm_out,
    whirlpool_out, LaunchLabBuyQuote, MAX_SLIPPAGE_BPS,
};
pub use route_ix::{sol_fee_program, spl_fee_program, token_fee_program, FEE_ASSET_EXACT_OUT};
pub use trade::{BuiltTrade, RouterClient, TradeOpts};
pub use transfer_fee::TokenTransferFee;

// ── Re-export sol-trade-sdk surface used by callers (parity) ─────────────────
pub use sol_trade_sdk::common::{
    GasFeeStrategy, InfrastructureConfig, TradeConfig, TradeTransactionVersion,
};
pub use sol_trade_sdk::{AstralaneTransport, DurableNonceInfo, SwqosTransport, fetch_nonce_info};
pub use sol_trade_sdk::swqos::{SwqosClient, SwqosConfig, SwqosType, TradeType};
pub use sol_trade_sdk::trading::{
    factory::DexType,
    MiddlewareManager, InstructionMiddleware,
    core::params::{
        BonkParams, DexParamEnum, LaunchLabParams, MeteoraDammV2Params, MeteoraDlmmParams,
        PumpFunParams, PumpSwapParams, RaydiumAmmV4Params, RaydiumClmmParams, RaydiumCpmmParams,
        StonkFunMemeLeg, StonkFunParams, StonkFunSolHop, StonkFunSwapParams, StonkFunViaSolParams,
        WhirlpoolParams,
    },
};
pub use sol_trade_sdk::{
    AccountPolicy, BuyAmount, SellAmount, SimpleBuyParams, SimpleSellParams, TradeBuyParams,
    TradeRiskGate, TradeSellParams, TradeTokenType, TradingInfrastructure,
};
pub use sol_trade_sdk::common::keypair;
pub use sol_trade_sdk::recommended_sender_thread_core_indices;

#[cfg(test)]
mod mainnet_sim;
#[cfg(test)]
mod mainnet_sim_tests;
#[cfg(test)]
mod mainnet_router_tests;
#[cfg(test)]
mod offline_tests;

/// Config PDA seeds.
pub const CONFIG_SEED: &[u8] = b"config";

/// Derive router config PDA.
pub fn config_pda(program_id: &solana_sdk::pubkey::Pubkey) -> (solana_sdk::pubkey::Pubkey, u8) {
    solana_sdk::pubkey::Pubkey::find_program_address(&[CONFIG_SEED], program_id)
}
