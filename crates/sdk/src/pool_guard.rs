//! Reject wild / non-canonical pools before building a trade.
//!
//! Aggregators often route through shallow or spoofed AMM pools sharing a mint
//! ("野池"). This module pins known 1:1 markets via PDA derivation and optional
//! allowlists for multi-pool venues (CPMM / CLMM / Whirlpool / DLMM / …).

use std::collections::HashSet;

use anyhow::{anyhow, Result};
use solana_sdk::pubkey::Pubkey;

use crate::{
    constants::{
        LAUNCHLAB_PROGRAM, PUMPFUN_PROGRAM, PUMPSWAP_PROGRAM, RAYDIUM_CPMM_PROGRAM,
        STONKFUN_REWARD_PLATFORM_CONFIG, STONKFUN_STANDARD_PLATFORM_CONFIG, WSOL_MINT,
    },
    market::{
        CpmmPool, LaunchLabPool, Market, PumpFunPool, PumpSwapPool, RoutedMarket,
    },
};

/// How strictly to reject non-canonical / unlisted pools.
#[derive(Clone, Debug)]
pub struct PoolGuardPolicy {
    /// Master switch (default on).
    pub enabled: bool,
    /// LaunchLab `platform_config` must be StonkFun standard/reward.
    pub stonk_platform_only: bool,
    /// Explicitly trusted pool addresses (graduation CPMM, pinned CLMM, …).
    pub trusted_pools: HashSet<Pubkey>,
    /// When true, multi-pool AMMs (CPMM / CLMM / Whirlpool / DLMM / AmmV4 / DAMM)
    /// must appear in [`Self::trusted_pools`]. PDA-unique venues
    /// (LaunchLab / PumpFun / canonical PumpSwap) still use derivation checks.
    pub require_trusted_for_amm: bool,
}

impl Default for PoolGuardPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            stonk_platform_only: false,
            trusted_pools: HashSet::new(),
            require_trusted_for_amm: false,
        }
    }
}

impl PoolGuardPolicy {
    /// Off — layout / unit tests with synthetic keys.
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Self::default()
        }
    }

    /// Stonk-focused: only StonkFun LaunchLab platforms; AMMs must be allowlisted.
    pub fn stonk_strict() -> Self {
        Self {
            enabled: true,
            stonk_platform_only: true,
            trusted_pools: HashSet::new(),
            require_trusted_for_amm: true,
        }
    }

    pub fn with_trusted_pools(mut self, pools: impl IntoIterator<Item = Pubkey>) -> Self {
        self.trusted_pools.extend(pools);
        self
    }

    pub fn trust(mut self, pool: Pubkey) -> Self {
        self.trusted_pools.insert(pool);
        self
    }

    #[inline]
    pub fn is_trusted(&self, pool: &Pubkey) -> bool {
        self.trusted_pools.contains(pool)
    }
}

/// True for the two StonkFun LaunchLab platform configs.
#[inline]
pub fn is_stonkfun_platform(platform_config: &Pubkey) -> bool {
    *platform_config == STONKFUN_STANDARD_PLATFORM_CONFIG
        || *platform_config == STONKFUN_REWARD_PLATFORM_CONFIG
}

/// LaunchLab pool PDA: `["pool", base_mint, quote_mint]`.
#[inline]
pub fn launchlab_pool_pda(base_mint: &Pubkey, quote_mint: &Pubkey) -> Option<Pubkey> {
    Pubkey::try_find_program_address(
        &[b"pool", base_mint.as_ref(), quote_mint.as_ref()],
        &LAUNCHLAB_PROGRAM,
    )
    .map(|(k, _)| k)
}

/// LaunchLab vault PDA: `["pool_vault", pool_state, mint]`.
#[inline]
pub fn launchlab_vault_pda(pool_state: &Pubkey, mint: &Pubkey) -> Option<Pubkey> {
    Pubkey::try_find_program_address(
        &[b"pool_vault", pool_state.as_ref(), mint.as_ref()],
        &LAUNCHLAB_PROGRAM,
    )
    .map(|(k, _)| k)
}

/// PumpFun bonding-curve PDA: `["bonding-curve", mint]`.
#[inline]
pub fn pumpfun_bonding_curve_pda(mint: &Pubkey) -> Option<Pubkey> {
    Pubkey::try_find_program_address(&[b"bonding-curve", mint.as_ref()], &PUMPFUN_PROGRAM)
        .map(|(k, _)| k)
}

/// Pump program pool-authority PDA used as creator for the canonical PumpSwap pool.
#[inline]
pub fn pump_pool_authority_pda(mint: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[b"pool-authority", mint.as_ref()], &PUMPFUN_PROGRAM).0
}

/// Canonical PumpSwap pool after bonding-curve graduation (WSOL quote).
/// Seeds: `["pool", index=0, pumpPoolAuthority(mint), mint, WSOL]`.
#[inline]
pub fn pumpswap_canonical_pool_pda(mint: &Pubkey) -> Pubkey {
    const INDEX: u16 = 0;
    let authority = pump_pool_authority_pda(mint);
    Pubkey::find_program_address(
        &[
            b"pool",
            &INDEX.to_le_bytes(),
            authority.as_ref(),
            mint.as_ref(),
            WSOL_MINT.as_ref(),
        ],
        &PUMPSWAP_PROGRAM,
    )
    .0
}

/// Raydium CPMM vault PDA: `["pool_vault", pool_state, mint]`.
#[inline]
pub fn cpmm_vault_pda(pool_state: &Pubkey, mint: &Pubkey) -> Option<Pubkey> {
    Pubkey::try_find_program_address(
        &[b"pool_vault", pool_state.as_ref(), mint.as_ref()],
        &RAYDIUM_CPMM_PROGRAM,
    )
    .map(|(k, _)| k)
}

/// Raydium CPMM observation PDA: `["observation", pool_state]`.
#[inline]
pub fn cpmm_observation_pda(pool_state: &Pubkey) -> Option<Pubkey> {
    Pubkey::try_find_program_address(&[b"observation", pool_state.as_ref()], &RAYDIUM_CPMM_PROGRAM)
        .map(|(k, _)| k)
}

/// Raydium CPMM pool PDA: `["pool", amm_config, mint_a, mint_b]` (order as stored).
#[inline]
pub fn cpmm_pool_pda(amm_config: &Pubkey, mint_a: &Pubkey, mint_b: &Pubkey) -> Option<Pubkey> {
    Pubkey::try_find_program_address(
        &[
            b"pool",
            amm_config.as_ref(),
            mint_a.as_ref(),
            mint_b.as_ref(),
        ],
        &RAYDIUM_CPMM_PROGRAM,
    )
    .map(|(k, _)| k)
}

fn reject(msg: impl Into<String>) -> Result<()> {
    Err(anyhow!("wild pool rejected: {}", msg.into()))
}

fn require_trusted_or(policy: &PoolGuardPolicy, pool: &Pubkey, label: &str) -> Result<()> {
    if policy.is_trusted(pool) {
        return Ok(());
    }
    if policy.require_trusted_for_amm {
        return reject(format!(
            "{label} pool {pool} not in trusted_pools (set require_trusted_for_amm=false or trust())"
        ));
    }
    Ok(())
}

/// Validate a LaunchLab (inner) pool snapshot.
pub fn assert_launchlab_ok(pool: &LaunchLabPool, policy: &PoolGuardPolicy) -> Result<()> {
    if !policy.enabled {
        return Ok(());
    }
    if policy.is_trusted(&pool.pool_state) {
        return Ok(());
    }
    if policy.stonk_platform_only && !is_stonkfun_platform(&pool.platform_config) {
        return reject(format!(
            "LaunchLab platform_config {} is not StonkFun",
            pool.platform_config
        ));
    }
    let expected = launchlab_pool_pda(&pool.base_mint, &pool.quote_mint)
        .ok_or_else(|| anyhow!("LaunchLab pool PDA derivation failed"))?;
    if pool.pool_state != expected {
        return reject(format!(
            "LaunchLab pool_state {} ≠ PDA {} for {} / {}",
            pool.pool_state, expected, pool.base_mint, pool.quote_mint
        ));
    }
    if let Some(v) = launchlab_vault_pda(&pool.pool_state, &pool.base_mint) {
        if pool.base_vault != Pubkey::default() && pool.base_vault != v {
            return reject(format!(
                "LaunchLab base_vault {} ≠ PDA {}",
                pool.base_vault, v
            ));
        }
    }
    if let Some(v) = launchlab_vault_pda(&pool.pool_state, &pool.quote_mint) {
        if pool.quote_vault != Pubkey::default() && pool.quote_vault != v {
            return reject(format!(
                "LaunchLab quote_vault {} ≠ PDA {}",
                pool.quote_vault, v
            ));
        }
    }
    Ok(())
}

/// Validate a PumpFun bonding-curve snapshot.
pub fn assert_pumpfun_ok(pool: &PumpFunPool, policy: &PoolGuardPolicy) -> Result<()> {
    if !policy.enabled {
        return Ok(());
    }
    if policy.is_trusted(&pool.bonding_curve) {
        return Ok(());
    }
    let expected = pumpfun_bonding_curve_pda(&pool.mint)
        .ok_or_else(|| anyhow!("PumpFun bonding-curve PDA derivation failed"))?;
    if pool.bonding_curve != expected {
        return reject(format!(
            "PumpFun bonding_curve {} ≠ PDA {} for mint {}",
            pool.bonding_curve, expected, pool.mint
        ));
    }
    Ok(())
}

/// Validate PumpSwap: WSOL-quote pools must be the canonical graduation PDA
/// (unless allowlisted). Non-WSOL quote requires allowlist when
/// `require_trusted_for_amm` is set.
pub fn assert_pumpswap_ok(pool: &PumpSwapPool, policy: &PoolGuardPolicy) -> Result<()> {
    if !policy.enabled {
        return Ok(());
    }
    if policy.is_trusted(&pool.pool) {
        return Ok(());
    }
    if pool.quote_mint == WSOL_MINT {
        let canonical = pumpswap_canonical_pool_pda(&pool.base_mint);
        if pool.pool == canonical {
            return Ok(());
        }
        // pool-v2 address is a separate account, not the swap pool — reject mismatch.
        return reject(format!(
            "PumpSwap pool {} ≠ canonical {} for mint {} (wild / non-graduation pool)",
            pool.pool, canonical, pool.base_mint
        ));
    }
    require_trusted_or(policy, &pool.pool, "PumpSwap non-WSOL")
}

/// Structural CPMM checks + optional allowlist / PDA when `amm_config` is known.
pub fn assert_cpmm_ok(pool: &CpmmPool, policy: &PoolGuardPolicy) -> Result<()> {
    if !policy.enabled {
        return Ok(());
    }
    if policy.is_trusted(&pool.pool_state) {
        return Ok(());
    }
    require_trusted_or(policy, &pool.pool_state, "CPMM")?;

    if pool.amm_config != Pubkey::default() {
        let a = cpmm_pool_pda(&pool.amm_config, &pool.base_mint, &pool.quote_mint);
        let b = cpmm_pool_pda(&pool.amm_config, &pool.quote_mint, &pool.base_mint);
        let ok = a == Some(pool.pool_state) || b == Some(pool.pool_state);
        if !ok {
            return reject(format!(
                "CPMM pool_state {} does not match PDA for amm_config {}",
                pool.pool_state, pool.amm_config
            ));
        }
    }

    if let Some(v) = cpmm_vault_pda(&pool.pool_state, &pool.base_mint) {
        if pool.base_vault != Pubkey::default() && pool.base_vault != v {
            return reject(format!("CPMM base_vault {} ≠ PDA {}", pool.base_vault, v));
        }
    }
    if let Some(v) = cpmm_vault_pda(&pool.pool_state, &pool.quote_mint) {
        if pool.quote_vault != Pubkey::default() && pool.quote_vault != v {
            return reject(format!("CPMM quote_vault {} ≠ PDA {}", pool.quote_vault, v));
        }
    }
    if let Some(obs) = cpmm_observation_pda(&pool.pool_state) {
        if pool.observation_state != Pubkey::default() && pool.observation_state != obs {
            return reject(format!(
                "CPMM observation_state {} ≠ PDA {}",
                pool.observation_state, obs
            ));
        }
    }
    Ok(())
}

fn assert_amm_listed(policy: &PoolGuardPolicy, pool: &Pubkey, label: &str) -> Result<()> {
    if !policy.enabled {
        return Ok(());
    }
    require_trusted_or(policy, pool, label)
}

/// Validate a single [`Market`] snapshot.
pub fn assert_market_ok(market: &Market, policy: &PoolGuardPolicy) -> Result<()> {
    match market {
        Market::LaunchLabInner(p) => assert_launchlab_ok(p, policy),
        Market::PumpFunInner(p) => assert_pumpfun_ok(p, policy),
        Market::PumpSwapOuter(p) => assert_pumpswap_ok(p, policy),
        Market::CpmmOuter(p) => assert_cpmm_ok(p, policy),
        Market::RaydiumAmmV4(p) => assert_amm_listed(policy, &p.amm, "Raydium AmmV4"),
        Market::MeteoraDammV2(p) => assert_amm_listed(policy, &p.pool, "Meteora DAMM V2"),
        Market::RaydiumClmm(p) => assert_amm_listed(policy, &p.pool_state, "Raydium CLMM"),
        Market::Whirlpool(p) => assert_amm_listed(policy, &p.whirlpool, "Orca Whirlpool"),
        Market::MeteoraDlmm(p) => assert_amm_listed(policy, &p.lb_pair, "Meteora DLMM"),
    }
}

/// Validate market + optional SOL↔quote bridge.
pub fn assert_routed_market_ok(routed: &RoutedMarket, policy: &PoolGuardPolicy) -> Result<()> {
    assert_market_ok(&routed.market, policy)?;
    if let Some(bridge) = &routed.bridge {
        assert_cpmm_ok(bridge, policy)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        constants::{TOKEN_PROGRAM, pumpfun_bonding_curve_v2},
        market::LaunchLabPool,
        transfer_fee::TokenTransferFee,
    };

    fn dummy_ll(platform: Pubkey) -> LaunchLabPool {
        let base = Pubkey::new_unique();
        let quote = WSOL_MINT;
        let pool_state = launchlab_pool_pda(&base, &quote).unwrap();
        let base_vault = launchlab_vault_pda(&pool_state, &base).unwrap();
        let quote_vault = launchlab_vault_pda(&pool_state, &quote).unwrap();
        LaunchLabPool {
            base_mint: base,
            quote_mint: quote,
            base_token_program: TOKEN_PROGRAM,
            quote_token_program: TOKEN_PROGRAM,
            pool_state,
            base_vault,
            quote_vault,
            platform_config: platform,
            platform_associated_account: Pubkey::default(),
            creator_associated_account: Pubkey::default(),
            global_config: Pubkey::default(),
            virtual_base: 1_000_000,
            virtual_quote: 1_000_000,
            real_base: 0,
            real_quote: 0,
            total_base_sell: 0,
            curve_type: 0,
            trade_fee_rate: 2_500,
            platform_fee_rate: 10_000,
            creator_fee_rate: 0,
            base_transfer_fee: TokenTransferFee::default(),
            quote_transfer_fee: TokenTransferFee::default(),
        }
    }

    #[test]
    fn launchlab_pda_ok_stonk_platform() {
        let pool = dummy_ll(STONKFUN_STANDARD_PLATFORM_CONFIG);
        assert_launchlab_ok(&pool, &PoolGuardPolicy::default()).unwrap();
        assert_launchlab_ok(&pool, &PoolGuardPolicy::stonk_strict().trust(pool.pool_state))
            .unwrap();
        // stonk_strict without trust still passes LaunchLab (PDA venue).
        assert_launchlab_ok(&pool, &PoolGuardPolicy::stonk_strict()).unwrap();
    }

    #[test]
    fn launchlab_rejects_wrong_pool_state() {
        let mut pool = dummy_ll(STONKFUN_STANDARD_PLATFORM_CONFIG);
        pool.pool_state = Pubkey::new_unique();
        assert!(assert_launchlab_ok(&pool, &PoolGuardPolicy::default()).is_err());
    }

    #[test]
    fn launchlab_stonk_only_rejects_foreign_platform() {
        let pool = dummy_ll(Pubkey::new_unique());
        assert!(assert_launchlab_ok(&pool, &PoolGuardPolicy::default()).is_ok());
        assert!(assert_launchlab_ok(&pool, &PoolGuardPolicy::stonk_strict()).is_err());
    }

    #[test]
    fn pumpfun_rejects_non_pda_curve() {
        let mint = Pubkey::new_unique();
        let curve = pumpfun_bonding_curve_pda(&mint).unwrap();
        let pool = PumpFunPool {
            mint,
            mint_token_program: TOKEN_PROGRAM,
            quote_mint: WSOL_MINT,
            quote_token_program: TOKEN_PROGRAM,
            use_v2: false,
            bonding_curve: curve,
            associated_bonding_curve: Pubkey::new_unique(),
            creator_vault: Pubkey::new_unique(),
            fee_recipient: Pubkey::new_unique(),
            buyback_fee_recipient: Pubkey::new_unique(),
            global: Pubkey::new_unique(),
            event_authority: Pubkey::new_unique(),
            global_volume_accumulator: Pubkey::new_unique(),
            user_volume_accumulator: Pubkey::new_unique(),
            fee_config: Pubkey::new_unique(),
            fee_program: Pubkey::new_unique(),
            bonding_curve_v2: pumpfun_bonding_curve_v2(&mint),
            protocol_fee_recipient: Pubkey::new_unique(),
            virtual_token_reserves: 1,
            virtual_sol_reserves: 1,
            real_token_reserves: 1,
            protocol_fee_bps: 0,
            has_creator: false,
            is_cashback_coin: false,
        };
        assert_pumpfun_ok(&pool, &PoolGuardPolicy::default()).unwrap();
        let mut bad = pool.clone();
        bad.bonding_curve = Pubkey::new_unique();
        assert!(assert_pumpfun_ok(&bad, &PoolGuardPolicy::default()).is_err());
    }

    #[test]
    fn pumpswap_requires_canonical_for_wsol() {
        let mint = Pubkey::new_unique();
        let canonical = pumpswap_canonical_pool_pda(&mint);
        let ok = PumpSwapPool {
            pool: canonical,
            base_mint: mint,
            quote_mint: WSOL_MINT,
            pool_base_token_account: Pubkey::new_unique(),
            pool_quote_token_account: Pubkey::new_unique(),
            base_token_program: TOKEN_PROGRAM,
            quote_token_program: TOKEN_PROGRAM,
            coin_creator_vault_ata: Pubkey::new_unique(),
            coin_creator_vault_authority: Pubkey::new_unique(),
            coin_creator: Pubkey::new_unique(),
            base_reserve: 1,
            quote_reserve: 1,
            virtual_quote_reserves: 0,
            lp_fee_bps: 0,
            protocol_fee_bps: 0,
            creator_fee_bps: 0,
            is_cashback_coin: false,
            protocol_fee_recipient: Pubkey::new_unique(),
            buyback_fee_recipient: Pubkey::new_unique(),
        };
        assert_pumpswap_ok(&ok, &PoolGuardPolicy::default()).unwrap();
        let mut wild = ok.clone();
        wild.pool = Pubkey::new_unique();
        assert!(assert_pumpswap_ok(&wild, &PoolGuardPolicy::default()).is_err());
        assert_pumpswap_ok(&wild, &PoolGuardPolicy::default().trust(wild.pool)).unwrap();
    }

    #[test]
    fn cpmm_strict_needs_trust() {
        let pool = CpmmPool {
            pool_state: Pubkey::new_unique(),
            amm_config: Pubkey::default(),
            observation_state: Pubkey::default(),
            base_mint: Pubkey::new_unique(),
            quote_mint: WSOL_MINT,
            base_vault: Pubkey::default(),
            quote_vault: Pubkey::default(),
            base_token_program: TOKEN_PROGRAM,
            quote_token_program: TOKEN_PROGRAM,
            base_reserve: 1,
            quote_reserve: 1,
            trade_fee_rate: 2_500,
            creator_fee_rate: 0,
            creator_fee_on: 0,
            enable_creator_fee: false,
            base_transfer_fee: TokenTransferFee::default(),
            quote_transfer_fee: TokenTransferFee::default(),
        };
        assert_cpmm_ok(&pool, &PoolGuardPolicy::default()).unwrap();
        assert!(assert_cpmm_ok(&pool, &PoolGuardPolicy::stonk_strict()).is_err());
        assert_cpmm_ok(
            &pool,
            &PoolGuardPolicy::stonk_strict().trust(pool.pool_state),
        )
        .unwrap();
    }
}
