//! Pool snapshots for the hot path — filled from streamer / local cache, never RPC.

use solana_sdk::pubkey::Pubkey;

use crate::{
    constants::{TOKEN_PROGRAM, WSOL_MINT},
    transfer_fee::TokenTransferFee,
};

/// LaunchLab / StonkFun **inner** bonding curve pool.
#[derive(Clone, Debug)]
pub struct LaunchLabPool {
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub base_token_program: Pubkey,
    pub quote_token_program: Pubkey,
    pub pool_state: Pubkey,
    pub base_vault: Pubkey,
    pub quote_vault: Pubkey,
    pub platform_config: Pubkey,
    pub platform_associated_account: Pubkey,
    pub creator_associated_account: Pubkey,
    pub global_config: Pubkey,
    pub virtual_base: u128,
    pub virtual_quote: u128,
    pub real_base: u128,
    pub real_quote: u128,
    /// Graduation cap on base sold (0 = ignore clamp).
    pub total_base_sell: u128,
    /// 0 = constant-product (only type we quote).
    pub curve_type: u8,
    pub trade_fee_rate: u64,
    pub platform_fee_rate: u64,
    pub creator_fee_rate: u64,
    pub base_transfer_fee: TokenTransferFee,
    pub quote_transfer_fee: TokenTransferFee,
}

impl LaunchLabPool {
    #[inline]
    pub fn is_sol_quote(&self) -> bool {
        self.quote_mint == WSOL_MINT
    }
}

/// Raydium CPMM pool — StonkFun **outer** / SOL↔stock bridge.
///
/// `base_reserve` / `quote_reserve` must already subtract protocol/fund/creator fees
/// from vault balances (same as sol-trade-sdk).
#[derive(Clone, Debug)]
pub struct CpmmPool {
    pub pool_state: Pubkey,
    pub amm_config: Pubkey,
    pub observation_state: Pubkey,
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub base_vault: Pubkey,
    pub quote_vault: Pubkey,
    pub base_token_program: Pubkey,
    pub quote_token_program: Pubkey,
    pub base_reserve: u64,
    pub quote_reserve: u64,
    pub trade_fee_rate: u64,
    pub creator_fee_rate: u64,
    /// 0 = always on input; 1 = on input when base_in; 2 = on input when quote_in.
    pub creator_fee_on: u8,
    pub enable_creator_fee: bool,
    pub base_transfer_fee: TokenTransferFee,
    pub quote_transfer_fee: TokenTransferFee,
}

impl CpmmPool {
    #[inline]
    pub fn other_mint(&self, mint: &Pubkey) -> Option<Pubkey> {
        if *mint == self.base_mint {
            Some(self.quote_mint)
        } else if *mint == self.quote_mint {
            Some(self.base_mint)
        } else {
            None
        }
    }

    #[inline]
    pub fn token_program_for(&self, mint: &Pubkey) -> Option<Pubkey> {
        if *mint == self.base_mint {
            Some(self.base_token_program)
        } else if *mint == self.quote_mint {
            Some(self.quote_token_program)
        } else {
            None
        }
    }

    /// Non-WSOL side when SOL-paired; otherwise `None` (use bridge / explicit meme).
    #[inline]
    pub fn meme_mint_sol_paired(&self) -> Option<Pubkey> {
        if self.base_mint == WSOL_MINT {
            Some(self.quote_mint)
        } else if self.quote_mint == WSOL_MINT {
            Some(self.base_mint)
        } else {
            None
        }
    }

    /// Meme for SOL-paired pools; for stock pairs prefer `RoutedMarket::meme_mint()`.
    #[inline]
    pub fn meme_mint(&self) -> Pubkey {
        self.meme_mint_sol_paired().unwrap_or(self.base_mint)
    }

    #[inline]
    pub fn pay_mint(&self) -> Pubkey {
        if self.base_mint == WSOL_MINT || self.quote_mint == WSOL_MINT {
            WSOL_MINT
        } else {
            self.quote_mint
        }
    }
}

/// PumpFun **inner** bonding curve (SOL quote, V1 layout).
#[derive(Clone, Debug)]
pub struct PumpFunPool {
    pub mint: Pubkey,
    pub mint_token_program: Pubkey,
    pub bonding_curve: Pubkey,
    pub associated_bonding_curve: Pubkey,
    pub creator_vault: Pubkey,
    pub fee_recipient: Pubkey,
    pub global: Pubkey,
    pub event_authority: Pubkey,
    pub global_volume_accumulator: Pubkey,
    pub user_volume_accumulator: Pubkey,
    pub fee_config: Pubkey,
    pub fee_program: Pubkey,
    pub bonding_curve_v2: Pubkey,
    pub protocol_fee_recipient: Pubkey,
    pub virtual_token_reserves: u64,
    pub virtual_sol_reserves: u64,
    pub real_token_reserves: u64,
    /// 0 → use `95 + (has_creator ? 30 : 0)`; else override.
    pub protocol_fee_bps: u64,
    pub has_creator: bool,
    /// Cashback coins need `user_volume_accumulator` on sell + track_volume=1 on buy.
    pub is_cashback_coin: bool,
}

impl PumpFunPool {
    #[inline]
    pub fn track_volume_byte(&self) -> u8 {
        u8::from(self.is_cashback_coin)
    }
}

/// SOL↔quote bridge when the meme market does not quote SOL.
pub type BridgePool = CpmmPool;

/// Target market (inner or outer).
#[derive(Clone, Debug)]
pub enum Market {
    LaunchLabInner(LaunchLabPool),
    CpmmOuter(CpmmPool),
    PumpFunInner(PumpFunPool),
}

impl Market {
    #[inline]
    pub fn base_mint(&self) -> Pubkey {
        match self {
            Self::LaunchLabInner(p) => p.base_mint,
            Self::CpmmOuter(p) => p.meme_mint(),
            Self::PumpFunInner(p) => p.mint,
        }
    }

    #[inline]
    pub fn quote_mint(&self) -> Pubkey {
        match self {
            Self::LaunchLabInner(p) => p.quote_mint,
            Self::CpmmOuter(p) => p.pay_mint(),
            Self::PumpFunInner(_) => WSOL_MINT,
        }
    }

    #[inline]
    pub fn needs_sol_bridge(&self) -> bool {
        match self {
            Self::LaunchLabInner(p) => !p.is_sol_quote(),
            Self::CpmmOuter(p) => p.base_mint != WSOL_MINT && p.quote_mint != WSOL_MINT,
            Self::PumpFunInner(_) => false,
        }
    }

    #[inline]
    pub fn base_token_program(&self) -> Pubkey {
        match self {
            Self::LaunchLabInner(p) => p.base_token_program,
            Self::CpmmOuter(p) => {
                let meme = p.meme_mint();
                p.token_program_for(&meme).unwrap_or(p.base_token_program)
            }
            Self::PumpFunInner(p) => p.mint_token_program,
        }
    }

    #[inline]
    pub fn quote_token_program(&self) -> Pubkey {
        match self {
            Self::LaunchLabInner(p) => p.quote_token_program,
            Self::CpmmOuter(p) => {
                let pay = p.pay_mint();
                p.token_program_for(&pay).unwrap_or(p.quote_token_program)
            }
            Self::PumpFunInner(_) => TOKEN_PROGRAM,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RoutedMarket {
    pub market: Market,
    pub bridge: Option<BridgePool>,
}

impl RoutedMarket {
    pub fn new(market: Market) -> Self {
        Self {
            market,
            bridge: None,
        }
    }

    pub fn with_bridge(market: Market, bridge: BridgePool) -> Self {
        Self {
            market,
            bridge: Some(bridge),
        }
    }

    pub fn stonk_inner(pool: LaunchLabPool, bridge: Option<BridgePool>) -> Self {
        Self {
            market: Market::LaunchLabInner(pool),
            bridge,
        }
    }

    /// For non-WSOL CPMM pairs, set `pool.base_mint` = meme and provide `bridge`.
    pub fn stonk_outer(pool: CpmmPool, bridge: Option<BridgePool>) -> Self {
        Self {
            market: Market::CpmmOuter(pool),
            bridge,
        }
    }

    pub fn pumpfun(pool: PumpFunPool) -> Self {
        Self {
            market: Market::PumpFunInner(pool),
            bridge: None,
        }
    }

    pub fn meme_mint(&self) -> Pubkey {
        match (&self.market, &self.bridge) {
            (Market::CpmmOuter(pool), Some(bridge)) if self.market.needs_sol_bridge() => {
                match shared_mint(pool, bridge) {
                    Some(stock) => pool.other_mint(&stock).unwrap_or(pool.meme_mint()),
                    None => pool.meme_mint(),
                }
            }
            (Market::CpmmOuter(pool), _) => {
                pool.meme_mint_sol_paired().unwrap_or(pool.base_mint)
            }
            _ => self.market.base_mint(),
        }
    }

    pub fn meme_token_program(&self) -> Pubkey {
        match &self.market {
            Market::CpmmOuter(pool) => {
                let meme = self.meme_mint();
                pool.token_program_for(&meme)
                    .unwrap_or(self.market.base_token_program())
            }
            _ => self.market.base_token_program(),
        }
    }
}

fn shared_mint(pool: &CpmmPool, bridge: &CpmmPool) -> Option<Pubkey> {
    let b = [bridge.base_mint, bridge.quote_mint];
    if b.contains(&pool.quote_mint) {
        Some(pool.quote_mint)
    } else if b.contains(&pool.base_mint) {
        Some(pool.base_mint)
    } else {
        None
    }
}

/// PDA: platform fee vault for LaunchLab (`[platform_config, quote_mint]`).
pub fn launchlab_platform_associated_account(
    platform_config: &Pubkey,
    quote_mint: &Pubkey,
) -> Option<Pubkey> {
    Pubkey::try_find_program_address(
        &[platform_config.as_ref(), quote_mint.as_ref()],
        &crate::constants::LAUNCHLAB_PROGRAM,
    )
    .map(|(k, _)| k)
}

/// PDA: creator fee vault for LaunchLab (`[creator, quote_mint]`).
pub fn launchlab_creator_associated_account(
    creator: &Pubkey,
    quote_mint: &Pubkey,
) -> Option<Pubkey> {
    Pubkey::try_find_program_address(
        &[creator.as_ref(), quote_mint.as_ref()],
        &crate::constants::LAUNCHLAB_PROGRAM,
    )
    .map(|(k, _)| k)
}
