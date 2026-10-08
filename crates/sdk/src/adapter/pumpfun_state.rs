//! Current PumpFun fee/owner refresh for the cold RPC path.
//! Fixed offsets below are from pump-sdk 3.2.0's published Global/BondingCurve IDL.
use crate::{constants::*, market::PumpFunPool};
use anyhow::{anyhow, Result};
use sol_trade_sdk::common::SolanaRpcClient;
use sol_trade_sdk::instruction::utils::pumpswap::{
    calculate_fee_tier, decode_fee_config, PumpSwapFeeBasisPoints,
};
use solana_sdk::{account::Account, pubkey::Pubkey};

fn u64_at(data: &[u8], offset: usize) -> Result<u64> {
    Ok(u64::from_le_bytes(
        data.get(offset..offset + 8)
            .ok_or_else(|| anyhow!("truncated PumpFun account at {offset}"))?
            .try_into()?,
    ))
}
fn key_at(data: &[u8], offset: usize) -> Result<Pubkey> {
    Ok(Pubkey::new_from_array(
        data.get(offset..offset + 32)
            .ok_or_else(|| anyhow!("truncated PumpFun key at {offset}"))?
            .try_into()?,
    ))
}
fn choose_recipient(data: &[u8], first: usize, others: usize, current: Pubkey) -> Result<Pubkey> {
    let mut keys = vec![key_at(data, first)?];
    for i in 0..7 {
        keys.push(key_at(data, others + i * 32)?);
    }
    if current != Pubkey::default() && keys.contains(&current) {
        return Ok(current);
    }
    keys.into_iter()
        .find(|key| *key != Pubkey::default())
        .ok_or_else(|| anyhow!("PumpFun Global has no valid fee recipient"))
}

/// Atomically refresh curve, Global, FeeConfig and mint owners before local quoting.
/// No wallet signatures or transaction submission are involved.
pub(super) async fn refresh(
    rpc: &SolanaRpcClient,
    pool: &mut PumpFunPool,
    expected_creator: Pubkey,
) -> Result<()> {
    let keys = [
        pool.bonding_curve,
        PUMPFUN_GLOBAL,
        PUMPFUN_FEE_CONFIG,
        pool.mint,
        pool.quote_mint,
    ];
    let accounts = rpc.get_multiple_accounts(&keys).await?;
    let get = |i: usize| {
        accounts
            .get(i)
            .and_then(Option::as_ref)
            .ok_or_else(|| anyhow!("PumpFun snapshot missing account {}", keys[i]))
    };
    if key_at(&get(0)?.data, 49)? != expected_creator {
        return Err(anyhow!(
            "PumpFun creator changed during refresh; reload fee-sharing configuration"
        ));
    }
    apply(
        pool,
        get(0)?,
        get(1)?,
        accounts.get(2).and_then(Option::as_ref),
        get(3)?,
        get(4)?,
    )
}

fn mint_program(account: &Account) -> Result<Pubkey> {
    if account.owner != TOKEN_PROGRAM && account.owner != TOKEN_2022_PROGRAM {
        return Err(anyhow!("mint has an unsupported owner"));
    }
    if account.data.len() < 82 || account.data.get(45) != Some(&1) {
        return Err(anyhow!("invalid or uninitialized mint"));
    }
    // The current PumpFun quote math does not implement mint extension fees/hooks.
    // A base Token-2022 mint with metadata/authorities remains usable; reject
    // TransferFeeConfig and TransferHook before presenting an automatic quote.
    if account.owner == TOKEN_2022_PROGRAM && account.data.len() > 82 {
        if account.data.get(165) != Some(&1) {
            return Err(anyhow!("invalid Token-2022 mint account type"));
        }
        let mut offset = 166;
        while offset + 4 <= account.data.len() {
            let kind = u16::from_le_bytes(account.data[offset..offset + 2].try_into()?);
            let len = u16::from_le_bytes(account.data[offset + 2..offset + 4].try_into()?) as usize;
            if kind == 0 {
                break;
            }
            if !matches!(kind, 3 | 10 | 18 | 19 | 20 | 21 | 22 | 23) {
                return Err(anyhow!(
                    "PumpFun mint extension {kind} requires unsupported handling"
                ));
            }
            offset = offset
                .checked_add(4 + len)
                .filter(|end| *end <= account.data.len())
                .ok_or_else(|| anyhow!("malformed Token-2022 mint extensions"))?;
        }
    }
    Ok(account.owner)
}

fn apply(
    pool: &mut PumpFunPool,
    curve: &Account,
    global: &Account,
    config: Option<&Account>,
    mint: &Account,
    quote: &Account,
) -> Result<()> {
    apply_inner(pool, curve, global, config, mint, quote, false)
}

fn apply_inner(
    pool: &mut PumpFunPool,
    curve: &Account,
    global: &Account,
    config: Option<&Account>,
    mint: &Account,
    quote: &Account,
    v3: bool,
) -> Result<()> {
    if v3 && (config.is_none() || curve.data.get(82) == Some(&1)) {
        return Err(anyhow!(
            "Pump V3 requires FeeConfig and does not support cashback"
        ));
    }
    if curve.owner != PUMPFUN_PROGRAM
        || global.owner != PUMPFUN_PROGRAM
        || curve.data.get(..8) != Some(&[23, 183, 248, 55, 96, 216, 172, 96])
        || global.data.get(..8) != Some(&[167, 232, 232, 177, 200, 108, 114, 127])
    {
        return Err(anyhow!(
            "invalid PumpFun curve/Global owner or discriminator"
        ));
    }
    if curve.data.get(48) != Some(&0)
        || (!v3 && curve.data.get(141).is_some_and(|depth| *depth != 0))
    {
        return Err(anyhow!(
            "completed Pump curves must migrate; nested curves require the explicit V3 API"
        ));
    }
    let stored_quote = if curve.data.len() >= 115 {
        key_at(&curve.data, 83)?
    } else {
        Pubkey::default()
    };
    let effective_quote = if stored_quote == Pubkey::default() {
        WSOL_MINT
    } else {
        stored_quote
    };
    if effective_quote != pool.quote_mint {
        return Err(anyhow!(
            "PumpFun quote mint changed during snapshot refresh"
        ));
    }
    let base_program = mint_program(mint)?;
    let quote_program = mint_program(quote)?;
    if pool.quote_mint == solana_sdk::pubkey!("9pan9bMn5HatX4EJdBwg9VgCa7Uz5HL8N1m5D3NdXejP") {
        return Err(anyhow!(
            "Token-2022 native quote mint is unsupported by PumpFun builders"
        ));
    }
    let base_reserve = u64_at(&curve.data, 8)?;
    let quote_reserve = u64_at(&curve.data, 16)?;
    if base_reserve == 0 || quote_reserve == 0 {
        return Err(anyhow!("empty PumpFun virtual reserves"));
    }
    let creator = key_at(&curve.data, 49)?;
    let mayhem = curve.data.get(81) == Some(&1);
    let supply = if mayhem {
        u64_at(&mint.data, 36)?
    } else {
        1_000_000_000_000_000
    };
    let fees = if let Some(config) = config {
        if config.owner != PUMPFUN_FEE_PROGRAM {
            return Err(anyhow!("invalid PumpFun FeeConfig owner"));
        }
        let config =
            decode_fee_config(&config.data).ok_or_else(|| anyhow!("invalid PumpFun FeeConfig"))?;
        if pool.quote_mint == WSOL_MINT || pool.quote_mint == USDC_MINT {
            let tiers = if pool.quote_mint == USDC_MINT && !config.stable_fee_tiers.is_empty() {
                &config.stable_fee_tiers
            } else {
                &config.fee_tiers
            };
            calculate_fee_tier(
                tiers,
                supply as u128 * quote_reserve as u128 / base_reserve as u128,
            )
            .ok_or_else(|| anyhow!("empty PumpFun fee schedule"))?
        } else if config.exotic_flat_fees == PumpSwapFeeBasisPoints::new(0, 0, 0) {
            config.flat_fees
        } else {
            config.exotic_flat_fees
        }
    } else {
        PumpSwapFeeBasisPoints::new(0, u64_at(&global.data, 105)?, u64_at(&global.data, 154)?)
    };
    let override_creator = if global.data.get(1045) == Some(&1) && curve.data.len() >= 123 {
        u64_at(&curve.data, 115)?
    } else {
        0
    };
    pool.protocol_fee_bps = fees.protocol_fee_basis_points;
    pool.creator_fee_bps = if creator == Pubkey::default() {
        0
    } else if override_creator != 0 {
        override_creator
    } else {
        fees.coin_creator_fee_basis_points
    };
    pool.has_creator = creator != Pubkey::default();
    pool.is_cashback_coin = curve.data.get(82) == Some(&1);
    pool.virtual_token_reserves = base_reserve;
    pool.virtual_sol_reserves = quote_reserve;
    pool.real_token_reserves = u64_at(&curve.data, 24)?;
    pool.mint_token_program = base_program;
    pool.quote_token_program = quote_program;
    pool.associated_bonding_curve = crate::ata::ata(&pool.bonding_curve, &pool.mint, &base_program);
    pool.fee_recipient = if mayhem {
        choose_recipient(&global.data, 483, 516, pool.fee_recipient)?
    } else {
        choose_recipient(&global.data, 41, 162, pool.fee_recipient)?
    };
    pool.buyback_fee_recipient =
        choose_recipient(&global.data, 741, 773, pool.buyback_fee_recipient)?;
    pool.fee_rates_known = true;
    Ok(())
}

/// Load a coherent V3 state after discovering quote mint and base token owner.
/// The second batch revalidates both discoveries before using any reserves.
pub async fn load_pumpfun_v3_by_rpc(
    rpc: &SolanaRpcClient,
    mint: &Pubkey,
    user: &Pubkey,
) -> Result<crate::pumpfun_v3::PumpFunV3Pool> {
    let curve_key =
        Pubkey::find_program_address(&[b"bonding-curve", mint.as_ref()], &PUMPFUN_PROGRAM).0;
    let first = rpc.get_multiple_accounts(&[curve_key, *mint]).await?;
    let initial_curve = first
        .first()
        .and_then(Option::as_ref)
        .ok_or_else(|| anyhow!("missing Pump curve"))?;
    let initial_mint = first
        .get(1)
        .and_then(Option::as_ref)
        .ok_or_else(|| anyhow!("missing Pump mint"))?;
    let stored_quote = if initial_curve.data.len() >= 115 {
        key_at(&initial_curve.data, 83)?
    } else {
        Pubkey::default()
    };
    let quote_mint = if stored_quote == Pubkey::default() {
        WSOL_MINT
    } else {
        stored_quote
    };
    let token_program = mint_program(initial_mint)?;
    let vault = crate::ata::ata(&curve_key, mint, &token_program);
    let volume = Pubkey::find_program_address(
        &[b"user_volume_accumulator", user.as_ref()],
        &PUMPFUN_PROGRAM,
    )
    .0;
    let keys = [
        curve_key,
        PUMPFUN_GLOBAL,
        PUMPFUN_FEE_CONFIG,
        *mint,
        quote_mint,
        vault,
        volume,
    ];
    let accounts = rpc.get_multiple_accounts(&keys).await?;
    let get = |i: usize| {
        accounts
            .get(i)
            .and_then(Option::as_ref)
            .ok_or_else(|| anyhow!("Pump V3 snapshot missing {}", keys[i]))
    };
    let mut pool = crate::parser::pumpfun_from_trade(&Default::default());
    pool.mint = *mint;
    pool.quote_mint = quote_mint;
    pool.bonding_curve = curve_key;
    apply_inner(
        &mut pool,
        get(0)?,
        get(1)?,
        Some(get(2)?),
        get(3)?,
        get(4)?,
        true,
    )?;
    if pool.mint_token_program != token_program {
        return Err(anyhow!("Pump base mint owner changed during refresh"));
    }
    let base_vault = get(5)?;
    if base_vault.owner != token_program
        || key_at(&base_vault.data, 0)? != *mint
        || key_at(&base_vault.data, 32)? != curve_key
        || base_vault.data.get(108) != Some(&1)
    {
        return Err(anyhow!("invalid Pump curve base token vault"));
    }
    let needs_volume_initialization = match accounts.get(6).and_then(Option::as_ref) {
        None => true,
        Some(account) if account.owner == SYSTEM_PROGRAM && account.data.is_empty() => true,
        Some(account) => {
            if account.owner != PUMPFUN_PROGRAM
                || account.data.get(..8) != Some(&[86, 255, 112, 14, 102, 53, 154, 250])
                || key_at(&account.data, 8)? != *user
            {
                return Err(anyhow!("invalid Pump user volume accumulator"));
            }
            false
        }
    };
    Ok(crate::pumpfun_v3::PumpFunV3Pool {
        curve: pool,
        complete: false, // apply_inner rejects already-completed curves.
        supports_graduation: true,
        real_quote_reserves: u64_at(&get(0)?.data, 32)?,
        curve_base_token_balance: u64_at(&base_vault.data, 64)?,
        pool_migration_fee: u64_at(&get(1)?.data, 146)?,
        mayhem_mode: get(0)?.data.get(81) == Some(&1),
        depth: get(0)?.data.get(141).copied().unwrap_or(0),
        needs_curve_extension: get(0)?.data.len() < 166,
        needs_volume_initialization,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn write_u64(data: &mut [u8], offset: usize, value: u64) {
        data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    fn fixture() -> (PumpFunPool, Account, Account, Account) {
        let mut pool = crate::parser::pumpfun_from_trade(&Default::default());
        pool.quote_mint = WSOL_MINT;
        let mut curve = Account {
            owner: PUMPFUN_PROGRAM,
            data: vec![0; 166],
            ..Default::default()
        };
        curve.data[..8].copy_from_slice(&[23, 183, 248, 55, 96, 216, 172, 96]);
        write_u64(&mut curve.data, 8, 1_000_000);
        write_u64(&mut curve.data, 16, 1_000);
        write_u64(&mut curve.data, 24, 800_000);
        curve.data[49..81].copy_from_slice(Pubkey::new_unique().as_ref());
        let mut global = Account {
            owner: PUMPFUN_PROGRAM,
            data: vec![0; 1088],
            ..Default::default()
        };
        global.data[..8].copy_from_slice(&[167, 232, 232, 177, 200, 108, 114, 127]);
        global.data[41..73].copy_from_slice(Pubkey::new_unique().as_ref());
        global.data[741..773].copy_from_slice(Pubkey::new_unique().as_ref());
        write_u64(&mut global.data, 105, 77);
        write_u64(&mut global.data, 154, 11);
        let mut mint = Account {
            owner: TOKEN_PROGRAM,
            data: vec![0; 82],
            ..Default::default()
        };
        mint.data[45] = 1;
        write_u64(&mut mint.data, 36, 1_000_000_000_000_000);
        (pool, curve, global, mint)
    }
    fn fee_config() -> Account {
        // Official FeeConfig IDL: discriminator, bump/admin, flat triple,
        // vector<threshold:u128, fees:3*u64>, stable vector, exotic triple.
        let mut data = vec![143, 52, 146, 187, 219, 123, 76, 155];
        data.extend([0; 33]);
        for v in [0u64, 90, 31] {
            data.extend(v.to_le_bytes());
        }
        data.extend(2u32.to_le_bytes());
        for (threshold, protocol, creator) in [(0u128, 50u64, 25u64), (1_000_000_000_000, 20, 30)] {
            data.extend(threshold.to_le_bytes());
            for v in [0, protocol, creator] {
                data.extend(v.to_le_bytes());
            }
        }
        data.extend(1u32.to_le_bytes());
        data.extend(0u128.to_le_bytes());
        for v in [0u64, 3, 4] {
            data.extend(v.to_le_bytes());
        }
        for v in [0u64, 7, 19] {
            data.extend(v.to_le_bytes());
        }
        Account {
            owner: PUMPFUN_FEE_PROGRAM,
            data,
            ..Default::default()
        }
    }
    #[test]
    fn current_schedules_override_and_recipient_fields_match_official_idl() {
        let (mut pool, mut curve, mut global, mint) = fixture();
        let config = fee_config();
        global.data[1045] = 1;
        write_u64(&mut curve.data, 115, 47);
        apply(&mut pool, &curve, &global, Some(&config), &mint, &mint).unwrap();
        assert_eq!((pool.protocol_fee_bps, pool.creator_fee_bps), (20, 47));
        assert_eq!(pool.fee_recipient, key_at(&global.data, 41).unwrap());
        assert_eq!(
            pool.buyback_fee_recipient,
            key_at(&global.data, 741).unwrap()
        );
        assert!(pool.fee_rates_known);
        assert_eq!(pool.quote_token_program, TOKEN_PROGRAM);
        global.data[1045] = 0;
        pool.quote_mint = USDC_MINT;
        curve.data[83..115].copy_from_slice(USDC_MINT.as_ref());
        apply(&mut pool, &curve, &global, Some(&config), &mint, &mint).unwrap();
        assert_eq!((pool.protocol_fee_bps, pool.creator_fee_bps), (3, 4));
        pool.quote_mint = Pubkey::new_unique();
        curve.data[83..115].copy_from_slice(pool.quote_mint.as_ref());
        apply(&mut pool, &curve, &global, Some(&config), &mint, &mint).unwrap();
        assert_eq!((pool.protocol_fee_bps, pool.creator_fee_bps), (7, 19));
        apply(&mut pool, &curve, &global, None, &mint, &mint).unwrap();
        assert_eq!((pool.protocol_fee_bps, pool.creator_fee_bps), (77, 11));
    }
    #[test]
    fn v3_requires_fee_config_and_accepts_nested_but_not_completed_or_cashback() {
        let (mut pool, mut curve, global, mint) = fixture();
        let config = fee_config();
        curve.data[141] = 1;
        assert!(apply_inner(&mut pool, &curve, &global, None, &mint, &mint, true).is_err());
        apply_inner(
            &mut pool,
            &curve,
            &global,
            Some(&config),
            &mint,
            &mint,
            true,
        )
        .unwrap();
        assert_eq!((pool.protocol_fee_bps, pool.creator_fee_bps), (20, 30));
        curve.data[82] = 1;
        assert!(apply_inner(
            &mut pool,
            &curve,
            &global,
            Some(&config),
            &mint,
            &mint,
            true
        )
        .is_err());
        curve.data[82] = 0;
        curve.data[48] = 1;
        assert!(apply_inner(
            &mut pool,
            &curve,
            &global,
            Some(&config),
            &mint,
            &mint,
            true
        )
        .is_err());
    }

    #[test]
    fn invalid_owners_unimplemented_extensions_and_v3_states_fail_closed() {
        let (mut pool, mut curve, mut global, mint) = fixture();
        global.owner = TOKEN_PROGRAM;
        assert!(apply(&mut pool, &curve, &global, None, &mint, &mint).is_err());
        global.owner = PUMPFUN_PROGRAM;
        curve.data[48] = 1;
        assert!(apply(&mut pool, &curve, &global, None, &mint, &mint).is_err());
        curve.data[48] = 0;
        curve.data[141] = 1;
        assert!(apply(&mut pool, &curve, &global, None, &mint, &mint).is_err());
        curve.data[141] = 0;
        let mut hook = mint.clone();
        hook.owner = TOKEN_2022_PROGRAM;
        hook.data.resize(170, 0);
        hook.data[165] = 1;
        hook.data[166] = 14;
        assert!(apply(&mut pool, &curve, &global, None, &mint, &hook).is_err());
    }
}
