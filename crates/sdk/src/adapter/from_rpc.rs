//! Cold-path RPC loaders: reuse `sol-trade-sdk` `from_*_by_rpc`, then convert.

use anyhow::Result;
use solana_sdk::pubkey::Pubkey;
use sol_trade_sdk::common::SolanaRpcClient;
use sol_trade_sdk::trading::core::params::{
    BonkParams, DexParamEnum, MeteoraDammV2Params, MeteoraDlmmParams, PumpFunParams,
    PumpSwapParams, RaydiumAmmV4Params, RaydiumClmmParams, RaydiumCpmmParams, WhirlpoolParams,
};
use sol_trade_sdk::trading::factory::DexType;

use super::from_params::to_routed_market_for_user;
use crate::market::RoutedMarket;

/// Cold-path request describing which pool to load over RPC.
#[derive(Clone, Debug)]
pub enum LoadMarketRequest {
    PumpFun { mint: Pubkey },
    PumpSwap { mint: Pubkey },
    LaunchLab { mint: Pubkey },
    Bonk { mint: Pubkey },
    StonkFun { mint: Pubkey },
    RaydiumCpmm { pool: Pubkey },
    RaydiumAmmV4 { amm: Pubkey },
    MeteoraDammV2 { pool: Pubkey },
    RaydiumClmm {
        pool: Pubkey,
        input_mint: Pubkey,
        output_mint: Pubkey,
    },
    OrcaWhirlpool {
        pool: Pubkey,
        input_mint: Pubkey,
        output_mint: Pubkey,
    },
    MeteoraDlmm {
        pool: Pubkey,
        input_mint: Pubkey,
        output_mint: Pubkey,
    },
}

impl LoadMarketRequest {
    pub fn dex_type(&self) -> DexType {
        match self {
            Self::PumpFun { .. } => DexType::PumpFun,
            Self::PumpSwap { .. } => DexType::PumpSwap,
            Self::LaunchLab { .. } => DexType::LaunchLab,
            Self::Bonk { .. } => DexType::Bonk,
            Self::StonkFun { .. } => DexType::StonkFun,
            Self::RaydiumCpmm { .. } => DexType::RaydiumCpmm,
            Self::RaydiumAmmV4 { .. } => DexType::RaydiumAmmV4,
            Self::MeteoraDammV2 { .. } => DexType::MeteoraDammV2,
            Self::RaydiumClmm { .. } => DexType::RaydiumClmm,
            Self::OrcaWhirlpool { .. } => DexType::OrcaWhirlpool,
            Self::MeteoraDlmm { .. } => DexType::MeteoraDlmm,
        }
    }

    pub fn mint_hint(&self) -> Pubkey {
        match self {
            Self::PumpFun { mint }
            | Self::PumpSwap { mint }
            | Self::LaunchLab { mint }
            | Self::Bonk { mint }
            | Self::StonkFun { mint } => *mint,
            Self::RaydiumClmm { output_mint, .. }
            | Self::OrcaWhirlpool { output_mint, .. }
            | Self::MeteoraDlmm { output_mint, .. } => *output_mint,
            Self::RaydiumCpmm { .. }
            | Self::RaydiumAmmV4 { .. }
            | Self::MeteoraDammV2 { .. } => Pubkey::default(),
        }
    }
}

/// Load trade-sdk params via RPC, then convert to [`RoutedMarket`].
pub async fn load_routed_market_by_rpc(
    rpc: &SolanaRpcClient,
    request: LoadMarketRequest,
    user: &Pubkey,
) -> Result<(DexParamEnum, RoutedMarket)> {
    let mint = request.mint_hint();
    let extension = match &request {
        LoadMarketRequest::PumpFun { mint } => {
            DexParamEnum::PumpFun(PumpFunParams::from_mint_by_rpc(rpc, mint).await?)
        }
        LoadMarketRequest::PumpSwap { mint } => {
            DexParamEnum::PumpSwap(PumpSwapParams::from_mint_by_rpc(rpc, mint).await?)
        }
        LoadMarketRequest::LaunchLab { mint } => {
            DexParamEnum::LaunchLab(BonkParams::from_mint_by_rpc(rpc, mint, false).await?)
        }
        LoadMarketRequest::Bonk { mint } => {
            DexParamEnum::Bonk(BonkParams::from_mint_by_rpc(rpc, mint, false).await?)
        }
        LoadMarketRequest::StonkFun { mint } => {
            DexParamEnum::StonkFun(BonkParams::from_mint_by_rpc(rpc, mint, false).await?)
        }
        LoadMarketRequest::RaydiumCpmm { pool } => {
            DexParamEnum::RaydiumCpmm(RaydiumCpmmParams::from_pool_address_by_rpc(rpc, pool).await?)
        }
        LoadMarketRequest::RaydiumAmmV4 { amm } => DexParamEnum::RaydiumAmmV4(
            RaydiumAmmV4Params::from_amm_address_by_rpc(rpc, *amm).await?,
        ),
        LoadMarketRequest::MeteoraDammV2 { pool } => DexParamEnum::MeteoraDammV2(
            MeteoraDammV2Params::from_pool_address_by_rpc(rpc, pool).await?,
        ),
        LoadMarketRequest::RaydiumClmm {
            pool,
            input_mint,
            output_mint,
        } => DexParamEnum::RaydiumClmm(
            RaydiumClmmParams::from_pool_address_by_rpc(rpc, pool, input_mint, output_mint).await?,
        ),
        LoadMarketRequest::OrcaWhirlpool {
            pool,
            input_mint,
            output_mint,
        } => DexParamEnum::OrcaWhirlpool(
            WhirlpoolParams::from_pool_address_by_rpc(rpc, pool, input_mint, output_mint).await?,
        ),
        LoadMarketRequest::MeteoraDlmm {
            pool,
            input_mint,
            output_mint,
        } => DexParamEnum::MeteoraDlmm(
            MeteoraDlmmParams::from_pool_address_by_rpc(rpc, pool, input_mint, output_mint).await?,
        ),
    };

    let mint = match &extension {
        DexParamEnum::RaydiumCpmm(p) => {
            if p.base_mint == crate::constants::WSOL_MINT {
                p.quote_mint
            } else if p.quote_mint == crate::constants::WSOL_MINT {
                p.base_mint
            } else {
                p.base_mint
            }
        }
        DexParamEnum::RaydiumAmmV4(p) => {
            if p.coin_mint == crate::constants::WSOL_MINT {
                p.pc_mint
            } else {
                p.coin_mint
            }
        }
        DexParamEnum::MeteoraDammV2(p) => {
            if p.token_a_mint == crate::constants::WSOL_MINT {
                p.token_b_mint
            } else {
                p.token_a_mint
            }
        }
        _ => mint,
    };

    let market = to_routed_market_for_user(&extension, mint, user)?;
    Ok((extension, market))
}
