//! Adapt `sol-trade-sdk` DEX params into router [`RoutedMarket`] snapshots.

mod from_params;
mod from_rpc;

pub use from_params::{
    bonk_to_launchlab, cpmm_from_params, damm_v2_from_params, dlmm_from_params,
    pumpfun_from_params, pumpswap_from_params, raydium_amm_v4_from_params, raydium_clmm_from_params,
    to_routed_market, to_routed_market_for_user, whirlpool_from_params, TransferFeeConvert,
};
pub use from_rpc::{load_routed_market_by_rpc, LoadMarketRequest};
