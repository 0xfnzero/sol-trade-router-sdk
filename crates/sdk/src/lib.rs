//! Hot-path trading SDK for `sol-trade-router`.
//!
//! # Design
//! - **No RPC on the hot path.** Pool snapshots come from streamer / local cache.
//! - Symmetric API: `buy_with_{sol|wsol|token}` / `sell_to_{sol|wsol|token}`.
//! - **WSOL / stock**: prepare ahead (`prepare_*_atas`). **Meme**: create in the buy tx
//!   (atomic — failed trade leaves no empty ATA). Closes are always opt-in except
//!   `sell_to_sol` which unwraps WSOL by default.
//! - On-chain fee integrity: `fee_source` must spend ≥ `amount_in` (fee + swap).
//!
//! ```ignore
//! // cold: WSOL + stock only (no meme)
//! let prep = client.prepare_buy_atas(&market, BuyWith::Sol);
//! // hot: creates meme ATA inside the same buy tx
//! let ixs = client.buy_with_sol(amount, &market)?.into_instructions();
//! ```

mod admin;
mod asset;
mod ata;
mod constants;
mod legs;
mod market;
mod quote;
mod route_ix;
mod trade;
mod transfer_fee;

pub use admin::{initialize_config, update_config};
pub use asset::{BuyWith, SellTo};
pub use ata::{
    ata, close_ata, close_wsol_ata, create_ata, create_wsol_ata, warm_ata_cache, wrap_sol,
    wrap_sol_with_options, AtaKind, AtaPolicy,
};
pub use constants::{
    LAUNCHLAB_PROGRAM, PROGRAM_ID, RAYDIUM_CPMM_PROGRAM, STONKFUN_REWARD_PLATFORM_CONFIG,
    STONKFUN_STANDARD_PLATFORM_CONFIG, TOKEN_2022_PROGRAM, WSOL_MINT,
};
pub use market::{
    launchlab_creator_associated_account, launchlab_platform_associated_account, BridgePool,
    CpmmPool, LaunchLabPool, Market, PumpFunPool, RoutedMarket,
};
pub use quote::{
    apply_slippage_min_out, clamp_slippage_bps, cpmm_out, fee_amount, launchlab_buy_base_out,
    launchlab_buy_quote, launchlab_sell_quote_out, pumpfun_buy_token_out, pumpfun_sell_sol_out,
    pumpfun_total_fee_bps, LaunchLabBuyQuote, MAX_SLIPPAGE_BPS,
};
pub use route_ix::{sol_fee_program, spl_fee_program, token_fee_program};
pub use trade::{BuiltTrade, RouterClient, TradeOpts};
pub use transfer_fee::TokenTransferFee;

/// Config PDA seeds.
pub const CONFIG_SEED: &[u8] = b"config";

/// Derive router config PDA.
pub fn config_pda(program_id: &solana_sdk::pubkey::Pubkey) -> (solana_sdk::pubkey::Pubkey, u8) {
    solana_sdk::pubkey::Pubkey::find_program_address(&[CONFIG_SEED], program_id)
}
