//! Pool snapshots for the hot path — filled from `sol-parser-sdk` events / local cache, never RPC.

use solana_sdk::pubkey::Pubkey;

use crate::{
    constants::{TOKEN_PROGRAM, USDC_MINT, WSOL_MINT},
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

/// PumpFun inner bonding curve. Non-WSOL quote pools always use V2.
#[derive(Clone, Debug)]
pub struct PumpFunPool {
    pub mint: Pubkey,
    pub mint_token_program: Pubkey,
    pub quote_mint: Pubkey,
    pub quote_token_program: Pubkey,
    /// Opt into V2 for WSOL-quote pools when settling via **WSOL ATA**
    /// (`BuyWith::Wsol` / `SellTo::Wsol`). Native SOL (`BuyWith::Sol` /
    /// `SellTo::Sol`) always uses V1 layout. Non-WSOL quote always selects V2.
    pub use_v2: bool,
    pub bonding_curve: Pubkey,
    pub associated_bonding_curve: Pubkey,
    pub creator_vault: Pubkey,
    pub fee_recipient: Pubkey,
    /// V2 buyback recipient (IDL buy_v2/sell_v2 account #9). Prefer GlobalConfig list.
    pub buyback_fee_recipient: Pubkey,
    pub global: Pubkey,
    pub event_authority: Pubkey,
    pub global_volume_accumulator: Pubkey,
    pub user_volume_accumulator: Pubkey,
    pub fee_config: Pubkey,
    pub fee_program: Pubkey,
    pub bonding_curve_v2: Pubkey,
    /// Legacy alias for V1 trailing buyback recipient (same 8-key buyback pool as
    /// `buyback_fee_recipient`). Prefer `buyback_fee_recipient` for new code.
    pub protocol_fee_recipient: Pubkey,
    pub virtual_token_reserves: u64,
    /// Virtual *quote* reserves (lamports for WSOL quote; USDC units for USDC quote V2).
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

    #[inline]
    pub fn uses_v2(&self) -> bool {
        // Non-WSOL quote always V2. WSOL-quote V2 is only for explicit WSOL-ATA
        // settlement (see trade.rs); native SOL settlement ignores this flag.
        self.use_v2 || self.quote_mint != WSOL_MINT
    }

    /// WSOL quote mint is the native-SOL sentinel — Pump spends/credits lamports
    /// on the V1 path (and when `BuyWith::Sol` / `SellTo::Sol`).
    #[inline]
    pub fn is_native_sol_quote(&self) -> bool {
        self.quote_mint == WSOL_MINT
    }

    /// Explicit WSOL-ATA settlement on a SOL-paired curve (`use_v2` + WSOL quote).
    #[inline]
    pub fn uses_wsol_ata_settlement(&self) -> bool {
        self.is_native_sol_quote() && self.use_v2
    }
}

#[derive(Clone, Debug)]
pub struct PumpSwapPool {
    pub pool: Pubkey,
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub pool_base_token_account: Pubkey,
    pub pool_quote_token_account: Pubkey,
    pub base_token_program: Pubkey,
    pub quote_token_program: Pubkey,
    pub coin_creator_vault_ata: Pubkey,
    pub coin_creator_vault_authority: Pubkey,
    pub coin_creator: Pubkey,
    pub base_reserve: u64,
    pub quote_reserve: u64,
    pub virtual_quote_reserves: i128,
    pub lp_fee_bps: u64,
    pub protocol_fee_bps: u64,
    pub creator_fee_bps: u64,
    pub is_cashback_coin: bool,
    /// Protocol fee recipient (IDL index 9). Observed from trade, or mayhem/standard default.
    pub protocol_fee_recipient: Pubkey,
    /// Buyback fee recipient (remaining account). Prefer GlobalConfig list.
    pub buyback_fee_recipient: Pubkey,
}

#[derive(Clone, Debug)]
pub struct RaydiumAmmV4Pool {
    pub amm: Pubkey,
    pub coin_mint: Pubkey,
    pub pc_mint: Pubkey,
    pub token_coin: Pubkey,
    pub token_pc: Pubkey,
    pub amm_open_orders: Pubkey,
    pub amm_target_orders: Pubkey,
    pub serum_program: Pubkey,
    pub serum_market: Pubkey,
    pub serum_bids: Pubkey,
    pub serum_asks: Pubkey,
    pub serum_event_queue: Pubkey,
    pub serum_coin_vault_account: Pubkey,
    pub serum_pc_vault_account: Pubkey,
    pub serum_vault_signer: Pubkey,
    pub coin_reserve: u64,
    pub pc_reserve: u64,
    /// From AmmInfo — default 25 (0.25%).
    pub trade_fee_numerator: u64,
    /// From AmmInfo — default 25 (taken from trade_fee, deducted from out).
    pub swap_fee_numerator: u64,
}

impl RaydiumAmmV4Pool {
    /// Historical helper: OpenBook keys may still be present on pool accounts.
    /// Hot-path swaps always use `SwapBaseInV2` (Raydium 2026-07-22); this flag is
    /// informational only and must not gate instruction selection.
    #[inline]
    pub fn uses_openbook_market(&self) -> bool {
        self.serum_market != Pubkey::default() && self.amm_open_orders != Pubkey::default()
    }
}

#[derive(Clone, Debug)]
pub struct MeteoraDammV2Pool {
    pub pool: Pubkey,
    pub token_a_vault: Pubkey,
    pub token_b_vault: Pubkey,
    pub token_a_mint: Pubkey,
    pub token_b_mint: Pubkey,
    pub token_a_program: Pubkey,
    pub token_b_program: Pubkey,
    pub token_a_reserve: u64,
    pub token_b_reserve: u64,
    pub fee_bps: u64,
    /// Input size this `expected_out` was quoted for (required when using expected_out).
    pub quoted_amount_in: Option<u64>,
    pub expected_out: Option<u64>,
    /// `swap2` mode: only `0` (exact-in) is supported by the router fee_source check.
    pub swap_mode: u8,
    pub referral_token_account: Option<Pubkey>,
    pub include_rate_limiter_sysvar: bool,
}

#[derive(Clone, Debug)]
pub struct RaydiumClmmPool {
    pub amm_config: Pubkey,
    pub pool_state: Pubkey,
    pub observation_state: Pubkey,
    pub token_0_mint: Pubkey,
    pub token_1_mint: Pubkey,
    pub token_0_vault: Pubkey,
    pub token_1_vault: Pubkey,
    pub token_0_program: Pubkey,
    pub token_1_program: Pubkey,
    pub tick_arrays: Vec<Pubkey>,
    pub tick_array_bitmap_extension: Option<Pubkey>,
    pub quoted_amount_in: Option<u64>,
    pub expected_out: Option<u64>,
    pub fee_bps: u16,
}

#[derive(Clone, Debug)]
pub struct WhirlpoolPool {
    pub whirlpool: Pubkey,
    pub mint_a: Pubkey,
    pub mint_b: Pubkey,
    pub vault_a: Pubkey,
    pub vault_b: Pubkey,
    pub token_program_a: Pubkey,
    pub token_program_b: Pubkey,
    pub tick_arrays: Vec<Pubkey>,
    pub quoted_amount_in: Option<u64>,
    pub expected_out: Option<u64>,
    pub fee_bps: u16,
}

#[derive(Clone, Debug)]
pub struct MeteoraDlmmPool {
    pub lb_pair: Pubkey,
    pub bitmap_extension: Option<Pubkey>,
    pub reserve_x: Pubkey,
    pub reserve_y: Pubkey,
    pub token_x_mint: Pubkey,
    pub token_y_mint: Pubkey,
    pub token_x_program: Pubkey,
    pub token_y_program: Pubkey,
    pub oracle: Pubkey,
    pub bin_arrays: Vec<Pubkey>,
    pub quoted_amount_in: Option<u64>,
    pub expected_out: Option<u64>,
    pub fee_bps: u16,
}

/// SOL↔quote bridge when the meme market does not quote SOL.
pub type BridgePool = CpmmPool;

/// Target market (inner or outer).
#[derive(Clone, Debug)]
pub enum Market {
    LaunchLabInner(LaunchLabPool),
    CpmmOuter(CpmmPool),
    PumpFunInner(PumpFunPool),
    PumpSwapOuter(PumpSwapPool),
    RaydiumAmmV4(RaydiumAmmV4Pool),
    MeteoraDammV2(MeteoraDammV2Pool),
    RaydiumClmm(RaydiumClmmPool),
    Whirlpool(WhirlpoolPool),
    MeteoraDlmm(MeteoraDlmmPool),
}

impl Market {
    #[inline]
    pub fn base_mint(&self) -> Pubkey {
        match self {
            Self::LaunchLabInner(p) => p.base_mint,
            Self::CpmmOuter(p) => p.meme_mint(),
            Self::PumpFunInner(p) => p.mint,
            Self::PumpSwapOuter(p) => p.base_mint,
            Self::RaydiumAmmV4(p) => {
                if p.coin_mint == WSOL_MINT || p.coin_mint == USDC_MINT {
                    p.pc_mint
                } else {
                    p.coin_mint
                }
            }
            Self::MeteoraDammV2(p) => {
                if p.token_a_mint == WSOL_MINT || p.token_a_mint == USDC_MINT {
                    p.token_b_mint
                } else {
                    p.token_a_mint
                }
            }
            Self::RaydiumClmm(p) => pair_base(p.token_0_mint, p.token_1_mint),
            Self::Whirlpool(p) => pair_base(p.mint_a, p.mint_b),
            Self::MeteoraDlmm(p) => pair_base(p.token_x_mint, p.token_y_mint),
        }
    }

    #[inline]
    pub fn quote_mint(&self) -> Pubkey {
        match self {
            Self::LaunchLabInner(p) => p.quote_mint,
            Self::CpmmOuter(p) => p.pay_mint(),
            Self::PumpFunInner(p) => p.quote_mint,
            Self::PumpSwapOuter(p) => p.quote_mint,
            Self::RaydiumAmmV4(p) => {
                if p.coin_mint == WSOL_MINT || p.coin_mint == USDC_MINT {
                    p.coin_mint
                } else {
                    p.pc_mint
                }
            }
            Self::MeteoraDammV2(p) => {
                if p.token_a_mint == WSOL_MINT || p.token_b_mint == WSOL_MINT {
                    WSOL_MINT
                } else if p.token_a_mint == USDC_MINT {
                    USDC_MINT
                } else {
                    p.token_b_mint
                }
            }
            Self::RaydiumClmm(p) => pair_quote(p.token_0_mint, p.token_1_mint),
            Self::Whirlpool(p) => pair_quote(p.mint_a, p.mint_b),
            Self::MeteoraDlmm(p) => pair_quote(p.token_x_mint, p.token_y_mint),
        }
    }

    #[inline]
    pub fn needs_sol_bridge(&self) -> bool {
        match self {
            Self::LaunchLabInner(p) => !p.is_sol_quote(),
            Self::CpmmOuter(p) => p.base_mint != WSOL_MINT && p.quote_mint != WSOL_MINT,
            Self::PumpFunInner(p) => p.quote_mint != WSOL_MINT,
            Self::PumpSwapOuter(p) => p.quote_mint != WSOL_MINT,
            Self::RaydiumAmmV4(p) => p.coin_mint != WSOL_MINT && p.pc_mint != WSOL_MINT,
            Self::MeteoraDammV2(p) => p.token_a_mint != WSOL_MINT && p.token_b_mint != WSOL_MINT,
            Self::RaydiumClmm(p) => p.token_0_mint != WSOL_MINT && p.token_1_mint != WSOL_MINT,
            Self::Whirlpool(p) => p.mint_a != WSOL_MINT && p.mint_b != WSOL_MINT,
            Self::MeteoraDlmm(p) => p.token_x_mint != WSOL_MINT && p.token_y_mint != WSOL_MINT,
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
            Self::PumpSwapOuter(p) => p.base_token_program,
            Self::RaydiumAmmV4(_) => TOKEN_PROGRAM,
            Self::MeteoraDammV2(p) => {
                if self.base_mint() == p.token_a_mint {
                    p.token_a_program
                } else {
                    p.token_b_program
                }
            }
            Self::RaydiumClmm(p) => {
                if self.base_mint() == p.token_0_mint {
                    p.token_0_program
                } else {
                    p.token_1_program
                }
            }
            Self::Whirlpool(p) => {
                if self.base_mint() == p.mint_a {
                    p.token_program_a
                } else {
                    p.token_program_b
                }
            }
            Self::MeteoraDlmm(p) => {
                if self.base_mint() == p.token_x_mint {
                    p.token_x_program
                } else {
                    p.token_y_program
                }
            }
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
            Self::PumpFunInner(p) => p.quote_token_program,
            Self::PumpSwapOuter(p) => p.quote_token_program,
            Self::RaydiumAmmV4(_) => TOKEN_PROGRAM,
            Self::MeteoraDammV2(p) => {
                if self.quote_mint() == p.token_a_mint {
                    p.token_a_program
                } else {
                    p.token_b_program
                }
            }
            Self::RaydiumClmm(p) => {
                if self.quote_mint() == p.token_0_mint {
                    p.token_0_program
                } else {
                    p.token_1_program
                }
            }
            Self::Whirlpool(p) => {
                if self.quote_mint() == p.mint_a {
                    p.token_program_a
                } else {
                    p.token_program_b
                }
            }
            Self::MeteoraDlmm(p) => {
                if self.quote_mint() == p.token_x_mint {
                    p.token_x_program
                } else {
                    p.token_y_program
                }
            }
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

    pub fn pumpswap(pool: PumpSwapPool) -> Self {
        Self::new(Market::PumpSwapOuter(pool))
    }

    pub fn raydium_amm_v4(pool: RaydiumAmmV4Pool) -> Self {
        Self::new(Market::RaydiumAmmV4(pool))
    }

    pub fn meteora_damm_v2(pool: MeteoraDammV2Pool) -> Self {
        Self::new(Market::MeteoraDammV2(pool))
    }

    pub fn raydium_clmm(pool: RaydiumClmmPool) -> Self {
        Self::new(Market::RaydiumClmm(pool))
    }

    pub fn whirlpool(pool: WhirlpoolPool) -> Self {
        Self::new(Market::Whirlpool(pool))
    }

    pub fn meteora_dlmm(pool: MeteoraDlmmPool) -> Self {
        Self::new(Market::MeteoraDlmm(pool))
    }

    pub fn meme_mint(&self) -> Pubkey {
        match (&self.market, &self.bridge) {
            (Market::CpmmOuter(pool), Some(bridge)) if self.market.needs_sol_bridge() => {
                match shared_mint(pool, bridge) {
                    Some(stock) => pool.other_mint(&stock).unwrap_or(pool.meme_mint()),
                    None => pool.meme_mint(),
                }
            }
            (Market::CpmmOuter(pool), _) => pool.meme_mint_sol_paired().unwrap_or(pool.base_mint),
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

#[inline]
fn pair_base(a: Pubkey, b: Pubkey) -> Pubkey {
    if a == WSOL_MINT {
        b
    } else if b == WSOL_MINT {
        a
    } else if a == USDC_MINT {
        b
    } else {
        a
    }
}

#[inline]
fn pair_quote(a: Pubkey, b: Pubkey) -> Pubkey {
    if a == WSOL_MINT || b == WSOL_MINT {
        WSOL_MINT
    } else if a == USDC_MINT {
        USDC_MINT
    } else {
        b
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
