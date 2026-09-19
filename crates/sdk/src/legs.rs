//! Build individual DEX legs (AccountMeta + data) for the router CPI payload.
//! Hot path: stack buffers + const AccountMetas (aligned with sol-trade-sdk).

use anyhow::{anyhow, Result};
use solana_sdk::{instruction::AccountMeta, pubkey::Pubkey};

use crate::{
    ata::ata,
    constants::*,
    market::{
        CpmmPool, LaunchLabPool, MeteoraDammV2Pool, MeteoraDlmmPool, PumpFunPool, PumpSwapPool,
        RaydiumAmmV4Pool, RaydiumClmmPool, WhirlpoolPool,
    },
};

/// Must stay in sync with on-chain `MAX_LEG_ACCOUNTS` in `programs/router/.../route.rs`.
pub const MAX_LEG_ACCOUNTS: usize = 64;

#[derive(Clone, Debug)]
pub struct Leg {
    pub program_id: Pubkey,
    pub accounts: Vec<AccountMeta>,
    pub data: Vec<u8>,
}

#[inline]
fn ensure_leg_account_budget(n: usize) -> Result<()> {
    if n == 0 || n > MAX_LEG_ACCOUNTS {
        return Err(anyhow!(
            "leg account count {n} exceeds router MAX_LEG_ACCOUNTS ({MAX_LEG_ACCOUNTS})"
        ));
    }
    Ok(())
}

#[inline(always)]
fn encode_u64_triple(disc: &[u8; 8], a: u64, b: u64, c: u64) -> [u8; 32] {
    let mut data = [0u8; 32];
    data[..8].copy_from_slice(disc);
    data[8..16].copy_from_slice(&a.to_le_bytes());
    data[16..24].copy_from_slice(&b.to_le_bytes());
    data[24..32].copy_from_slice(&c.to_le_bytes());
    data
}

#[inline(always)]
fn encode_u64_pair(disc: &[u8; 8], a: u64, b: u64) -> [u8; 24] {
    let mut data = [0u8; 24];
    data[..8].copy_from_slice(disc);
    data[8..16].copy_from_slice(&a.to_le_bytes());
    data[16..24].copy_from_slice(&b.to_le_bytes());
    data
}

pub fn launchlab_buy_leg(
    user: &Pubkey,
    pool: &LaunchLabPool,
    amount_in: u64,
    min_out: u64,
    user_base_ata: Pubkey,
    user_quote_ata: Pubkey,
) -> Leg {
    let data = encode_u64_triple(&BUY_EXACT_IN_LAUNCHLAB, amount_in, min_out, 0);
    // 18 accounts — matches sol-trade-sdk bonk.rs
    let accounts = vec![
        AccountMeta::new(*user, true),
        LAUNCHLAB_AUTHORITY_META,
        AccountMeta::new_readonly(pool.global_config, false),
        AccountMeta::new_readonly(pool.platform_config, false),
        AccountMeta::new(pool.pool_state, false),
        AccountMeta::new(user_base_ata, false),
        AccountMeta::new(user_quote_ata, false),
        AccountMeta::new(pool.base_vault, false),
        AccountMeta::new(pool.quote_vault, false),
        AccountMeta::new_readonly(pool.base_mint, false),
        AccountMeta::new_readonly(pool.quote_mint, false),
        AccountMeta::new_readonly(pool.base_token_program, false),
        AccountMeta::new_readonly(pool.quote_token_program, false),
        LAUNCHLAB_EVENT_AUTHORITY_META,
        LAUNCHLAB_PROGRAM_META,
        SYSTEM_PROGRAM_META,
        AccountMeta::new(pool.platform_associated_account, false),
        AccountMeta::new(pool.creator_associated_account, false),
    ];

    Leg {
        program_id: LAUNCHLAB_PROGRAM,
        accounts,
        data: data.to_vec(),
    }
}

pub fn launchlab_sell_leg(
    user: &Pubkey,
    pool: &LaunchLabPool,
    amount_in: u64,
    min_out: u64,
    user_base_ata: Pubkey,
    user_quote_ata: Pubkey,
) -> Leg {
    let data = encode_u64_triple(&SELL_EXACT_IN_LAUNCHLAB, amount_in, min_out, 0);
    let accounts = vec![
        AccountMeta::new(*user, true),
        LAUNCHLAB_AUTHORITY_META,
        AccountMeta::new_readonly(pool.global_config, false),
        AccountMeta::new_readonly(pool.platform_config, false),
        AccountMeta::new(pool.pool_state, false),
        AccountMeta::new(user_base_ata, false),
        AccountMeta::new(user_quote_ata, false),
        AccountMeta::new(pool.base_vault, false),
        AccountMeta::new(pool.quote_vault, false),
        AccountMeta::new_readonly(pool.base_mint, false),
        AccountMeta::new_readonly(pool.quote_mint, false),
        AccountMeta::new_readonly(pool.base_token_program, false),
        AccountMeta::new_readonly(pool.quote_token_program, false),
        LAUNCHLAB_EVENT_AUTHORITY_META,
        LAUNCHLAB_PROGRAM_META,
        SYSTEM_PROGRAM_META,
        AccountMeta::new(pool.platform_associated_account, false),
        AccountMeta::new(pool.creator_associated_account, false),
    ];

    Leg {
        program_id: LAUNCHLAB_PROGRAM,
        accounts,
        data: data.to_vec(),
    }
}

pub fn cpmm_swap_leg(
    user: &Pubkey,
    pool: &CpmmPool,
    amount_in: u64,
    min_out: u64,
    input_mint: Pubkey,
    output_mint: Pubkey,
    user_input_ata: Pubkey,
    user_output_ata: Pubkey,
) -> Result<Leg> {
    let (input_vault, output_vault, input_tp, output_tp) =
        if input_mint == pool.base_mint && output_mint == pool.quote_mint {
            (
                pool.base_vault,
                pool.quote_vault,
                pool.base_token_program,
                pool.quote_token_program,
            )
        } else if input_mint == pool.quote_mint && output_mint == pool.base_mint {
            (
                pool.quote_vault,
                pool.base_vault,
                pool.quote_token_program,
                pool.base_token_program,
            )
        } else {
            return Err(anyhow!(
                "cpmm swap mints do not match pool {} / {}",
                pool.base_mint,
                pool.quote_mint
            ));
        };

    let data = encode_u64_pair(&CPMM_SWAP_BASE_IN, amount_in, min_out);
    let accounts = vec![
        AccountMeta::new_readonly(*user, true),
        RAYDIUM_CPMM_AUTHORITY_META,
        AccountMeta::new_readonly(pool.amm_config, false),
        AccountMeta::new(pool.pool_state, false),
        AccountMeta::new(user_input_ata, false),
        AccountMeta::new(user_output_ata, false),
        AccountMeta::new(input_vault, false),
        AccountMeta::new(output_vault, false),
        AccountMeta::new_readonly(input_tp, false),
        AccountMeta::new_readonly(output_tp, false),
        AccountMeta::new_readonly(input_mint, false),
        AccountMeta::new_readonly(output_mint, false),
        AccountMeta::new(pool.observation_state, false),
    ];

    Ok(Leg {
        program_id: RAYDIUM_CPMM_PROGRAM,
        accounts,
        data: data.to_vec(),
    })
}

/// Exact-out CPMM swap: `amount_out` fixed, `max_amount_in` budget.
pub fn cpmm_swap_exact_out_leg(
    user: &Pubkey,
    pool: &CpmmPool,
    max_amount_in: u64,
    amount_out: u64,
    input_mint: Pubkey,
    output_mint: Pubkey,
    user_input_ata: Pubkey,
    user_output_ata: Pubkey,
) -> Result<Leg> {
    let (input_vault, output_vault, input_tp, output_tp) =
        if input_mint == pool.base_mint && output_mint == pool.quote_mint {
            (
                pool.base_vault,
                pool.quote_vault,
                pool.base_token_program,
                pool.quote_token_program,
            )
        } else if input_mint == pool.quote_mint && output_mint == pool.base_mint {
            (
                pool.quote_vault,
                pool.base_vault,
                pool.quote_token_program,
                pool.base_token_program,
            )
        } else {
            return Err(anyhow!(
                "cpmm swap mints do not match pool {} / {}",
                pool.base_mint,
                pool.quote_mint
            ));
        };

    // swap_base_output: max_amount_in, amount_out
    let data = encode_u64_pair(&CPMM_SWAP_BASE_OUT, max_amount_in, amount_out);
    let accounts = vec![
        AccountMeta::new_readonly(*user, true),
        RAYDIUM_CPMM_AUTHORITY_META,
        AccountMeta::new_readonly(pool.amm_config, false),
        AccountMeta::new(pool.pool_state, false),
        AccountMeta::new(user_input_ata, false),
        AccountMeta::new(user_output_ata, false),
        AccountMeta::new(input_vault, false),
        AccountMeta::new(output_vault, false),
        AccountMeta::new_readonly(input_tp, false),
        AccountMeta::new_readonly(output_tp, false),
        AccountMeta::new_readonly(input_mint, false),
        AccountMeta::new_readonly(output_mint, false),
        AccountMeta::new(pool.observation_state, false),
    ];

    Ok(Leg {
        program_id: RAYDIUM_CPMM_PROGRAM,
        accounts,
        data: data.to_vec(),
    })
}

/// PumpFun V1 `buy_exact_sol_in` (18 accounts).
pub fn pumpfun_buy_leg(
    user: &Pubkey,
    pool: &PumpFunPool,
    lamports_in: u64,
    min_tokens_out: u64,
    user_token_ata: Pubkey,
) -> Leg {
    let mut data = [0u8; 25];
    data[..8].copy_from_slice(&PUMPFUN_BUY_EXACT_SOL_IN);
    data[8..16].copy_from_slice(&lamports_in.to_le_bytes());
    data[16..24].copy_from_slice(&min_tokens_out.to_le_bytes());
    data[24] = pool.track_volume_byte();

    let uva = pumpfun_user_volume_accumulator(user);
    let bonding_curve_v2 = if pool.bonding_curve_v2 == Pubkey::default() {
        pumpfun_bonding_curve_v2(&pool.mint)
    } else {
        pool.bonding_curve_v2
    };
    let buyback = if pool.buyback_fee_recipient == Pubkey::default() {
        PUMPFUN_BUYBACK_FEE_RECIPIENT
    } else {
        pool.buyback_fee_recipient
    };

    let accounts = vec![
        AccountMeta::new_readonly(pool.global, false),
        AccountMeta::new(pool.fee_recipient, false),
        AccountMeta::new_readonly(pool.mint, false),
        AccountMeta::new(pool.bonding_curve, false),
        AccountMeta::new(pool.associated_bonding_curve, false),
        AccountMeta::new(user_token_ata, false),
        AccountMeta::new(*user, true),
        SYSTEM_PROGRAM_META,
        AccountMeta::new_readonly(pool.mint_token_program, false),
        AccountMeta::new(pool.creator_vault, false),
        AccountMeta::new_readonly(pool.event_authority, false),
        PUMPFUN_PROGRAM_META,
        AccountMeta::new_readonly(pool.global_volume_accumulator, false),
        AccountMeta::new(uva, false),
        AccountMeta::new_readonly(pool.fee_config, false),
        AccountMeta::new_readonly(pool.fee_program, false),
        // Remaining (post-IDL / @pump-fun/pump-sdk@2.0.0): bonding_curve_v2 (readonly)
        // + buybackFeeRecipient (writable) — see BREAKING_FEE_RECIPIENT.md.
        AccountMeta::new_readonly(bonding_curve_v2, false),
        AccountMeta::new(buyback, false),
    ];

    Leg {
        program_id: PUMPFUN_PROGRAM,
        accounts,
        data: data.to_vec(),
    }
}

/// PumpFun V1 sell. Cashback coins insert `user_volume_accumulator` before bonding_curve_v2.
pub fn pumpfun_sell_leg(
    user: &Pubkey,
    pool: &PumpFunPool,
    token_in: u64,
    min_sol_out: u64,
    user_token_ata: Pubkey,
) -> Leg {
    let data = encode_u64_pair(&PUMPFUN_SELL, token_in, min_sol_out);

    let uva = pumpfun_user_volume_accumulator(user);
    let bonding_curve_v2 = if pool.bonding_curve_v2 == Pubkey::default() {
        pumpfun_bonding_curve_v2(&pool.mint)
    } else {
        pool.bonding_curve_v2
    };
    let buyback = if pool.buyback_fee_recipient == Pubkey::default() {
        PUMPFUN_BUYBACK_FEE_RECIPIENT
    } else {
        pool.buyback_fee_recipient
    };

    let mut accounts = Vec::with_capacity(17);
    accounts.push(AccountMeta::new_readonly(pool.global, false));
    accounts.push(AccountMeta::new(pool.fee_recipient, false));
    accounts.push(AccountMeta::new_readonly(pool.mint, false));
    accounts.push(AccountMeta::new(pool.bonding_curve, false));
    accounts.push(AccountMeta::new(pool.associated_bonding_curve, false));
    accounts.push(AccountMeta::new(user_token_ata, false));
    accounts.push(AccountMeta::new(*user, true));
    accounts.push(SYSTEM_PROGRAM_META);
    // sell: creator_vault before token_program (order differs from buy)
    accounts.push(AccountMeta::new(pool.creator_vault, false));
    accounts.push(AccountMeta::new_readonly(pool.mint_token_program, false));
    accounts.push(AccountMeta::new_readonly(pool.event_authority, false));
    accounts.push(PUMPFUN_PROGRAM_META);
    accounts.push(AccountMeta::new_readonly(pool.fee_config, false));
    accounts.push(AccountMeta::new_readonly(pool.fee_program, false));
    if pool.is_cashback_coin {
        accounts.push(AccountMeta::new(uva, false));
    }
    accounts.push(AccountMeta::new_readonly(bonding_curve_v2, false));
    // Official pump-sdk: buybackFeeRecipient remaining is writable.
    accounts.push(AccountMeta::new(buyback, false));

    Leg {
        program_id: PUMPFUN_PROGRAM,
        accounts,
        data: data.to_vec(),
    }
}

fn pumpfun_v2_accounts(user: &Pubkey, pool: &PumpFunPool, is_buy: bool) -> Vec<AccountMeta> {
    let buyback = if pool.buyback_fee_recipient == Pubkey::default() {
        PUMPFUN_BUYBACK_FEE_RECIPIENT
    } else {
        pool.buyback_fee_recipient
    };
    let quote_fee_recipient = ata(
        &pool.fee_recipient,
        &pool.quote_mint,
        &pool.quote_token_program,
    );
    let quote_buyback = ata(&buyback, &pool.quote_mint, &pool.quote_token_program);
    let quote_curve = ata(
        &pool.bonding_curve,
        &pool.quote_mint,
        &pool.quote_token_program,
    );
    let base_user = ata(user, &pool.mint, &pool.mint_token_program);
    let quote_user = ata(user, &pool.quote_mint, &pool.quote_token_program);
    let associated_creator_vault = ata(
        &pool.creator_vault,
        &pool.quote_mint,
        &pool.quote_token_program,
    );
    let sharing_config =
        Pubkey::find_program_address(&[b"sharing-config", pool.mint.as_ref()], &pool.fee_program).0;
    // Official IDL: UVA PDA seeds = ["user_volume_accumulator", signer].
    let uva = pumpfun_user_volume_accumulator(user);
    let associated_user_volume = ata(&uva, &pool.quote_mint, &pool.quote_token_program);
    let mut accounts = Vec::with_capacity(if is_buy { 27 } else { 26 });
    accounts.extend([
        AccountMeta::new_readonly(pool.global, false),
        AccountMeta::new_readonly(pool.mint, false),
        AccountMeta::new_readonly(pool.quote_mint, false),
        AccountMeta::new_readonly(pool.mint_token_program, false),
        AccountMeta::new_readonly(pool.quote_token_program, false),
        ASSOCIATED_TOKEN_PROGRAM_META,
        AccountMeta::new(pool.fee_recipient, false),
        AccountMeta::new(quote_fee_recipient, false),
        AccountMeta::new(buyback, false),
        AccountMeta::new(quote_buyback, false),
        AccountMeta::new(pool.bonding_curve, false),
        AccountMeta::new(pool.associated_bonding_curve, false),
        AccountMeta::new(quote_curve, false),
        AccountMeta::new(*user, true),
        AccountMeta::new(base_user, false),
        AccountMeta::new(quote_user, false),
        AccountMeta::new(pool.creator_vault, false),
        AccountMeta::new(associated_creator_vault, false),
        AccountMeta::new_readonly(sharing_config, false),
    ]);
    if is_buy {
        accounts.push(AccountMeta::new_readonly(
            pool.global_volume_accumulator,
            false,
        ));
    }
    accounts.extend([
        AccountMeta::new(uva, false),
        AccountMeta::new(associated_user_volume, false),
        AccountMeta::new_readonly(pool.fee_config, false),
        AccountMeta::new_readonly(pool.fee_program, false),
        SYSTEM_PROGRAM_META,
        AccountMeta::new_readonly(pool.event_authority, false),
        PUMPFUN_PROGRAM_META,
    ]);
    accounts
}

pub fn pumpfun_buy_v2_leg(
    user: &Pubkey,
    pool: &PumpFunPool,
    quote_in: u64,
    min_tokens_out: u64,
) -> Leg {
    let data = encode_u64_pair(&PUMPFUN_BUY_EXACT_QUOTE_IN_V2, quote_in, min_tokens_out);
    Leg {
        program_id: PUMPFUN_PROGRAM,
        accounts: pumpfun_v2_accounts(user, pool, true),
        data: data.to_vec(),
    }
}

pub fn pumpfun_sell_v2_leg(
    user: &Pubkey,
    pool: &PumpFunPool,
    token_in: u64,
    min_quote_out: u64,
) -> Leg {
    let data = encode_u64_pair(&PUMPFUN_SELL_V2, token_in, min_quote_out);
    Leg {
        program_id: PUMPFUN_PROGRAM,
        accounts: pumpfun_v2_accounts(user, pool, false),
        data: data.to_vec(),
    }
}

fn pumpswap_accounts(user: &Pubkey, pool: &PumpSwapPool, is_buy: bool) -> Result<Vec<AccountMeta>> {
    let user_base = ata(user, &pool.base_mint, &pool.base_token_program);
    let user_quote = ata(user, &pool.quote_mint, &pool.quote_token_program);
    // Observed protocol fee recipient (mayhem or standard). Do not hardcode only
    // PUMPSWAP_PROTOCOL_FEE_RECIPIENT — mayhem pools require the mayhem fee pool.
    let protocol_fee = if pool.protocol_fee_recipient != Pubkey::default() {
        pool.protocol_fee_recipient
    } else {
        PUMPSWAP_PROTOCOL_FEE_RECIPIENT
    };
    let fee_ata = ata(&protocol_fee, &pool.quote_mint, &pool.quote_token_program);
    let mut accounts = Vec::with_capacity(28);
    accounts.extend([
        AccountMeta::new(pool.pool, false),
        AccountMeta::new(*user, true),
        PUMPSWAP_GLOBAL_META,
        AccountMeta::new_readonly(pool.base_mint, false),
        AccountMeta::new_readonly(pool.quote_mint, false),
        AccountMeta::new(user_base, false),
        AccountMeta::new(user_quote, false),
        AccountMeta::new(pool.pool_base_token_account, false),
        AccountMeta::new(pool.pool_quote_token_account, false),
        AccountMeta::new_readonly(protocol_fee, false),
        AccountMeta::new(fee_ata, false),
        AccountMeta::new_readonly(pool.base_token_program, false),
        AccountMeta::new_readonly(pool.quote_token_program, false),
        SYSTEM_PROGRAM_META,
        ASSOCIATED_TOKEN_PROGRAM_META,
        PUMPSWAP_EVENT_AUTHORITY_META,
        PUMPSWAP_PROGRAM_META,
        AccountMeta::new(pool.coin_creator_vault_ata, false),
        AccountMeta::new_readonly(pool.coin_creator_vault_authority, false),
    ]);
    let accumulator = Pubkey::try_find_program_address(
        &[b"user_volume_accumulator", user.as_ref()],
        &PUMPSWAP_PROGRAM,
    )
    .map(|(key, _)| key)
    .ok_or_else(|| anyhow!("PumpSwap user volume accumulator derivation failed"))?;
    if is_buy {
        accounts.push(PUMPSWAP_GLOBAL_VOLUME_ACCUMULATOR_META);
        accounts.push(AccountMeta::new(accumulator, false));
    }
    accounts.push(PUMPSWAP_FEE_CONFIG_META);
    accounts.push(PUMPSWAP_FEE_PROGRAM_META);
    if pool.is_cashback_coin {
        accounts.push(AccountMeta::new(
            ata(&accumulator, &pool.quote_mint, &pool.quote_token_program),
            false,
        ));
        if !is_buy {
            accounts.push(AccountMeta::new(accumulator, false));
        }
    }
    if pool.coin_creator != Pubkey::default() {
        let pool_v2 = Pubkey::try_find_program_address(
            &[b"pool-v2", pool.base_mint.as_ref()],
            &PUMPSWAP_PROGRAM,
        )
        .map(|(key, _)| key)
        .ok_or_else(|| anyhow!("PumpSwap pool-v2 derivation failed"))?;
        accounts.push(AccountMeta::new_readonly(pool_v2, false));
    }
    accounts.push(AccountMeta::new_readonly(
        pool.buyback_fee_recipient,
        false,
    ));
    accounts.push(AccountMeta::new(
        ata(
            &pool.buyback_fee_recipient,
            &pool.quote_mint,
            &pool.quote_token_program,
        ),
        false,
    ));
    ensure_leg_account_budget(accounts.len())?;
    Ok(accounts)
}

pub fn pumpswap_buy_leg(
    user: &Pubkey,
    pool: &PumpSwapPool,
    quote_in: u64,
    min_base_out: u64,
) -> Result<Leg> {
    let mut data = [0u8; 25];
    data[..8].copy_from_slice(&PUMPSWAP_BUY_EXACT_QUOTE_IN);
    data[8..16].copy_from_slice(&quote_in.to_le_bytes());
    data[16..24].copy_from_slice(&min_base_out.to_le_bytes());
    data[24] = 1;
    Ok(Leg {
        program_id: PUMPSWAP_PROGRAM,
        accounts: pumpswap_accounts(user, pool, true)?,
        data: data.to_vec(),
    })
}

/// PumpSwap exact-out buy: `base_amount_out` + `max_quote_amount_in`.
pub fn pumpswap_buy_exact_out_leg(
    user: &Pubkey,
    pool: &PumpSwapPool,
    base_amount_out: u64,
    max_quote_in: u64,
) -> Result<Leg> {
    let data = encode_u64_pair(&PUMPSWAP_BUY, base_amount_out, max_quote_in);
    Ok(Leg {
        program_id: PUMPSWAP_PROGRAM,
        accounts: pumpswap_accounts(user, pool, true)?,
        data: data.to_vec(),
    })
}

pub fn pumpswap_sell_leg(
    user: &Pubkey,
    pool: &PumpSwapPool,
    base_in: u64,
    min_quote_out: u64,
) -> Result<Leg> {
    let data = encode_u64_pair(&PUMPSWAP_SELL, base_in, min_quote_out);
    Ok(Leg {
        program_id: PUMPSWAP_PROGRAM,
        accounts: pumpswap_accounts(user, pool, false)?,
        data: data.to_vec(),
    })
}

pub fn raydium_amm_v4_swap_leg(
    user: &Pubkey,
    pool: &RaydiumAmmV4Pool,
    amount_in: u64,
    min_out: u64,
    input_mint: Pubkey,
) -> Result<Leg> {
    let output_mint = if input_mint == pool.coin_mint {
        pool.pc_mint
    } else if input_mint == pool.pc_mint {
        pool.coin_mint
    } else {
        return Err(anyhow!("Raydium AMM V4 input mint does not match pool"));
    };
    let mut data = [0u8; 17];
    data[1..9].copy_from_slice(&amount_in.to_le_bytes());
    data[9..17].copy_from_slice(&min_out.to_le_bytes());
    let user_src = ata(user, &input_mint, &TOKEN_PROGRAM);
    let user_dst = ata(user, &output_mint, &TOKEN_PROGRAM);
    // Official Raydium 2026-07-22: routers must use SwapBaseInV2 (tag 16).
    // raydium-sdk-V2: owner is signer-only (`isWritable: false`).
    data[0] = RAYDIUM_AMM_V4_SWAP_BASE_IN_V2;
    let accounts = vec![
        AccountMeta::new_readonly(TOKEN_PROGRAM, false),
        AccountMeta::new(pool.amm, false),
        RAYDIUM_AMM_V4_AUTHORITY_META,
        AccountMeta::new(pool.token_coin, false),
        AccountMeta::new(pool.token_pc, false),
        AccountMeta::new(user_src, false),
        AccountMeta::new(user_dst, false),
        AccountMeta::new_readonly(*user, true),
    ];
    Ok(Leg {
        program_id: RAYDIUM_AMM_V4_PROGRAM,
        accounts,
        data: data.to_vec(),
    })
}

/// Exact-out AMM V4 swap (tag 17): amount_out + max_amount_in.
pub fn raydium_amm_v4_swap_exact_out_leg(
    user: &Pubkey,
    pool: &RaydiumAmmV4Pool,
    amount_out: u64,
    max_amount_in: u64,
    input_mint: Pubkey,
) -> Result<Leg> {
    let output_mint = if input_mint == pool.coin_mint {
        pool.pc_mint
    } else if input_mint == pool.pc_mint {
        pool.coin_mint
    } else {
        return Err(anyhow!("Raydium AMM V4 input mint does not match pool"));
    };
    let mut data = [0u8; 17];
    data[0] = RAYDIUM_AMM_V4_SWAP_BASE_OUT_V2;
    data[1..9].copy_from_slice(&max_amount_in.to_le_bytes());
    data[9..17].copy_from_slice(&amount_out.to_le_bytes());
    let user_src = ata(user, &input_mint, &TOKEN_PROGRAM);
    let user_dst = ata(user, &output_mint, &TOKEN_PROGRAM);
    let accounts = vec![
        AccountMeta::new_readonly(TOKEN_PROGRAM, false),
        AccountMeta::new(pool.amm, false),
        RAYDIUM_AMM_V4_AUTHORITY_META,
        AccountMeta::new(pool.token_coin, false),
        AccountMeta::new(pool.token_pc, false),
        AccountMeta::new(user_src, false),
        AccountMeta::new(user_dst, false),
        AccountMeta::new_readonly(*user, true),
    ];
    Ok(Leg {
        program_id: RAYDIUM_AMM_V4_PROGRAM,
        accounts,
        data: data.to_vec(),
    })
}

pub fn meteora_damm_v2_swap_leg(
    user: &Pubkey,
    pool: &MeteoraDammV2Pool,
    amount_in: u64,
    min_out: u64,
    input_mint: Pubkey,
) -> Result<Leg> {
    if pool.swap_mode == METEORA_DAMM_V2_PARTIAL_FILL {
        return Err(anyhow!(
            "Meteora DAMM V2 partial-fill unsupported (router requires exact-in/exact-out spend)"
        ));
    }
    if pool.swap_mode != METEORA_DAMM_V2_EXACT_IN && pool.swap_mode != METEORA_DAMM_V2_EXACT_OUT {
        return Err(anyhow!(
            "unsupported Meteora DAMM V2 swap mode {}",
            pool.swap_mode
        ));
    }
    let (input_program, output_mint, output_program) = if input_mint == pool.token_a_mint {
        (
            pool.token_a_program,
            pool.token_b_mint,
            pool.token_b_program,
        )
    } else if input_mint == pool.token_b_mint {
        (
            pool.token_b_program,
            pool.token_a_mint,
            pool.token_a_program,
        )
    } else {
        return Err(anyhow!("Meteora DAMM V2 input mint does not match pool"));
    };
    let mut data = [0u8; 25];
    data[..8].copy_from_slice(&METEORA_DAMM_V2_SWAP2);
    data[8..16].copy_from_slice(&amount_in.to_le_bytes());
    data[16..24].copy_from_slice(&min_out.to_le_bytes());
    data[24] = pool.swap_mode;
    let event_authority =
        Pubkey::find_program_address(&[b"__event_authority"], &METEORA_DAMM_V2_PROGRAM).0;
    let mut accounts = Vec::with_capacity(16);
    accounts.extend([
        METEORA_DAMM_V2_AUTHORITY_META,
        AccountMeta::new(pool.pool, false),
        AccountMeta::new(ata(user, &input_mint, &input_program), false),
        AccountMeta::new(ata(user, &output_mint, &output_program), false),
        AccountMeta::new(pool.token_a_vault, false),
        AccountMeta::new(pool.token_b_vault, false),
        AccountMeta::new_readonly(pool.token_a_mint, false),
        AccountMeta::new_readonly(pool.token_b_mint, false),
        AccountMeta::new_readonly(*user, true),
        AccountMeta::new_readonly(pool.token_a_program, false),
        AccountMeta::new_readonly(pool.token_b_program, false),
    ]);
    // IDL optional `referral_token_account`: mainnet always passes either a real
    // referral ATA or the program id as a readonly placeholder (14 fixed accounts).
    // Omitting the slot shifts event_authority → AccountDiscriminatorMismatch.
    match pool.referral_token_account {
        Some(referral)
            if referral != Pubkey::default() && referral != METEORA_DAMM_V2_PROGRAM =>
        {
            accounts.push(AccountMeta::new(referral, false));
        }
        _ => accounts.push(METEORA_DAMM_V2_PROGRAM_META),
    }
    accounts.push(AccountMeta::new_readonly(event_authority, false));
    accounts.push(METEORA_DAMM_V2_PROGRAM_META);
    // Exact-in swaps on mainnet include the Instructions sysvar (rate limiter).
    if pool.include_rate_limiter_sysvar {
        accounts.push(SYSVAR_INSTRUCTIONS_META);
    }
    Ok(Leg {
        program_id: METEORA_DAMM_V2_PROGRAM,
        accounts,
        data: data.to_vec(),
    })
}

pub fn raydium_clmm_swap_leg(
    user: &Pubkey,
    pool: &RaydiumClmmPool,
    amount_in: u64,
    min_out: u64,
    input_mint: Pubkey,
) -> Result<Leg> {
    let bitmap_pda = crate::constants::raydium_clmm_tick_array_bitmap_extension(&pool.pool_state);
    let mut bitmap = pool
        .tick_array_bitmap_extension
        .filter(|b| *b == bitmap_pda);
    let mut tick_arrays = Vec::with_capacity(pool.tick_arrays.len());
    for &key in &pool.tick_arrays {
        if key == bitmap_pda {
            bitmap = Some(key);
        } else {
            tick_arrays.push(key);
        }
    }
    if tick_arrays.is_empty() {
        return Err(anyhow!("Raydium CLMM snapshot has no tick arrays"));
    }
    let (output_mint, input_vault, output_vault, input_program, output_program) =
        if input_mint == pool.token_0_mint {
            (
                pool.token_1_mint,
                pool.token_0_vault,
                pool.token_1_vault,
                pool.token_0_program,
                pool.token_1_program,
            )
        } else if input_mint == pool.token_1_mint {
            (
                pool.token_0_mint,
                pool.token_1_vault,
                pool.token_0_vault,
                pool.token_1_program,
                pool.token_0_program,
            )
        } else {
            return Err(anyhow!("Raydium CLMM input mint does not match pool"));
        };
    let zero_for_one = input_mint == pool.token_0_mint;
    // Raydium accepts 0 (= full-range), but set explicit bounds for clarity / parity with Whirlpool.
    let sqrt_limit = if zero_for_one {
        MIN_SQRT_PRICE_X64 + 1
    } else {
        MAX_SQRT_PRICE_X64 - 1
    };
    let mut data = [0u8; 41];
    data[..8].copy_from_slice(&CLMM_SWAP_V2);
    data[8..16].copy_from_slice(&amount_in.to_le_bytes());
    data[16..24].copy_from_slice(&min_out.to_le_bytes());
    data[24..40].copy_from_slice(&sqrt_limit.to_le_bytes());
    data[40] = 1; // is_base_input (exact-in)
    let mut accounts = Vec::with_capacity(14 + tick_arrays.len());
    accounts.extend([
        AccountMeta::new_readonly(*user, true), // IDL: payer signer, not writable
        AccountMeta::new_readonly(pool.amm_config, false),
        AccountMeta::new(pool.pool_state, false),
        AccountMeta::new(ata(user, &input_mint, &input_program), false),
        AccountMeta::new(ata(user, &output_mint, &output_program), false),
        AccountMeta::new(input_vault, false),
        AccountMeta::new(output_vault, false),
        AccountMeta::new(pool.observation_state, false),
        TOKEN_PROGRAM_META,
        TOKEN_2022_PROGRAM_META,
        MEMO_PROGRAM_META,
        AccountMeta::new_readonly(input_mint, false),
        AccountMeta::new_readonly(output_mint, false),
    ]);
    if let Some(ext) = bitmap {
        accounts.push(AccountMeta::new(ext, false));
    }
    accounts.extend(tick_arrays.iter().map(|key| AccountMeta::new(*key, false)));
    ensure_leg_account_budget(accounts.len())?;
    Ok(Leg {
        program_id: RAYDIUM_CLMM_PROGRAM,
        accounts,
        data: data.to_vec(),
    })
}

pub fn whirlpool_swap_leg(
    user: &Pubkey,
    pool: &WhirlpoolPool,
    amount_in: u64,
    min_out: u64,
    input_mint: Pubkey,
) -> Result<Leg> {
    if pool.tick_arrays.len() < 3 {
        return Err(anyhow!(
            "Whirlpool swap_v2 requires 3 tick arrays (got {})",
            pool.tick_arrays.len()
        ));
    }
    let a_to_b = if input_mint == pool.mint_a {
        true
    } else if input_mint == pool.mint_b {
        false
    } else {
        return Err(anyhow!("Whirlpool input mint does not match pool"));
    };
    let ticks = [pool.tick_arrays[0], pool.tick_arrays[1], pool.tick_arrays[2]];
    // Orca accepts sqrt_price_limit==0 as "no explicit limit" (maps to MIN/MAX
    // on-chain). Encode official Whirlpool full-range bounds (≠ Raydium CLMM MAX).
    let sqrt_limit = if a_to_b {
        WHIRLPOOL_MIN_SQRT_PRICE_X64
    } else {
        WHIRLPOOL_MAX_SQRT_PRICE_X64
    };
    let mut data = Vec::with_capacity(43);
    data.extend_from_slice(&WHIRLPOOL_SWAP_V2);
    data.extend_from_slice(&amount_in.to_le_bytes());
    data.extend_from_slice(&min_out.to_le_bytes());
    data.extend_from_slice(&sqrt_limit.to_le_bytes());
    data.push(1); // amount_specified_is_input
    data.push(u8::from(a_to_b));
    data.push(0); // remaining_accounts_info: None
    let owner_a = ata(user, &pool.mint_a, &pool.token_program_a);
    let owner_b = ata(user, &pool.mint_b, &pool.token_program_b);
    let oracle = Pubkey::find_program_address(
        &[b"oracle", pool.whirlpool.as_ref()],
        &ORCA_WHIRLPOOL_PROGRAM,
    )
    .0;
    let accounts = vec![
        AccountMeta::new_readonly(pool.token_program_a, false),
        AccountMeta::new_readonly(pool.token_program_b, false),
        MEMO_PROGRAM_META,
        AccountMeta::new_readonly(*user, true),
        AccountMeta::new(pool.whirlpool, false),
        AccountMeta::new_readonly(pool.mint_a, false),
        AccountMeta::new_readonly(pool.mint_b, false),
        AccountMeta::new(owner_a, false),
        AccountMeta::new(pool.vault_a, false),
        AccountMeta::new(owner_b, false),
        AccountMeta::new(pool.vault_b, false),
        AccountMeta::new(ticks[0], false),
        AccountMeta::new(ticks[1], false),
        AccountMeta::new(ticks[2], false),
        AccountMeta::new(oracle, false),
    ];
    Ok(Leg {
        program_id: ORCA_WHIRLPOOL_PROGRAM,
        accounts,
        data,
    })
}

pub fn meteora_dlmm_swap_leg(
    user: &Pubkey,
    pool: &MeteoraDlmmPool,
    amount_in: u64,
    min_out: u64,
    input_mint: Pubkey,
) -> Result<Leg> {
    if pool.bin_arrays.is_empty() {
        return Err(anyhow!("Meteora DLMM snapshot has no bin arrays"));
    }
    let output_mint = if input_mint == pool.token_x_mint {
        pool.token_y_mint
    } else if input_mint == pool.token_y_mint {
        pool.token_x_mint
    } else {
        return Err(anyhow!("Meteora DLMM input mint does not match pool"));
    };
    let input_program = if input_mint == pool.token_x_mint {
        pool.token_x_program
    } else {
        pool.token_y_program
    };
    let output_program = if output_mint == pool.token_x_mint {
        pool.token_x_program
    } else {
        pool.token_y_program
    };
    let mut data = [0u8; 28];
    data[..8].copy_from_slice(&METEORA_DLMM_SWAP2);
    data[8..16].copy_from_slice(&amount_in.to_le_bytes());
    data[16..24].copy_from_slice(&min_out.to_le_bytes());
    // remaining_accounts_info: empty slice (u32 length 0).
    let mut accounts = Vec::with_capacity(16 + pool.bin_arrays.len());
    // Official SDK: missing bitmap extension → program-id sentinel (readonly).
    let bitmap_meta = match pool.bitmap_extension {
        Some(key) => AccountMeta::new(key, false),
        None => AccountMeta::new_readonly(METEORA_DLMM_PROGRAM, false),
    };
    accounts.extend([
        AccountMeta::new(pool.lb_pair, false),
        bitmap_meta,
        AccountMeta::new(pool.reserve_x, false),
        AccountMeta::new(pool.reserve_y, false),
        AccountMeta::new(ata(user, &input_mint, &input_program), false),
        AccountMeta::new(ata(user, &output_mint, &output_program), false),
        AccountMeta::new_readonly(pool.token_x_mint, false),
        AccountMeta::new_readonly(pool.token_y_mint, false),
        AccountMeta::new(pool.oracle, false),
        AccountMeta::new_readonly(METEORA_DLMM_PROGRAM, false), // host_fee_in None sentinel
        AccountMeta::new_readonly(*user, true),
        AccountMeta::new_readonly(pool.token_x_program, false),
        AccountMeta::new_readonly(pool.token_y_program, false),
        MEMO_PROGRAM_META,
        METEORA_DLMM_EVENT_AUTHORITY_META,
        METEORA_DLMM_PROGRAM_META,
    ]);
    accounts.extend(
        pool.bin_arrays
            .iter()
            .map(|key| AccountMeta::new(*key, false)),
    );
    ensure_leg_account_budget(accounts.len())?;
    Ok(Leg {
        program_id: METEORA_DLMM_PROGRAM,
        accounts,
        data: data.to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(seed: u8) -> Pubkey {
        Pubkey::new_from_array([seed; 32])
    }

    fn pumpfun_v2_pool() -> PumpFunPool {
        PumpFunPool {
            mint: key(1),
            mint_token_program: TOKEN_2022_PROGRAM,
            quote_mint: USDC_MINT,
            quote_token_program: TOKEN_PROGRAM,
            use_v2: true,
            bonding_curve: key(2),
            associated_bonding_curve: key(3),
            creator_vault: key(4),
            fee_recipient: key(5),
            buyback_fee_recipient: PUMPFUN_BUYBACK_FEE_RECIPIENT,
            global: key(6),
            event_authority: key(7),
            global_volume_accumulator: key(8),
            user_volume_accumulator: key(9),
            fee_config: key(10),
            fee_program: PUMPSWAP_FEE_PROGRAM,
            bonding_curve_v2: key(11),
            protocol_fee_recipient: key(12),
            virtual_token_reserves: 1_000_000,
            virtual_sol_reserves: 2_000_000,
            real_token_reserves: 900_000,
            protocol_fee_bps: 95,
            has_creator: true,
            is_cashback_coin: false,
        }
    }

    #[test]
    fn pumpfun_v2_account_counts_and_discriminators_match_idl() {
        let user = key(20);
        let pool = pumpfun_v2_pool();
        let buy = pumpfun_buy_v2_leg(&user, &pool, 100, 90);
        let sell = pumpfun_sell_v2_leg(&user, &pool, 100, 90);
        assert_eq!(buy.accounts.len(), 27);
        assert_eq!(sell.accounts.len(), 26);
        assert_eq!(&buy.data[..8], &PUMPFUN_BUY_EXACT_QUOTE_IN_V2);
        assert_eq!(&sell.data[..8], &PUMPFUN_SELL_V2);
        assert!(buy.accounts[8].is_writable);
        assert_eq!(
            buy.accounts[18].pubkey,
            Pubkey::find_program_address(
                &[b"sharing-config", pool.mint.as_ref()],
                &pool.fee_program
            )
            .0
        );
        assert_eq!(buy.accounts[19].pubkey, pool.global_volume_accumulator);
        let expected_uva = pumpfun_user_volume_accumulator(&user);
        assert_eq!(buy.accounts[20].pubkey, expected_uva);
        assert_eq!(sell.accounts[19].pubkey, expected_uva);
    }

    #[test]
    fn concentrated_swap_layouts_match_idls() {
        let user = key(20);
        let raydium = RaydiumClmmPool {
            amm_config: key(1),
            pool_state: key(2),
            observation_state: key(3),
            token_0_mint: key(4),
            token_1_mint: key(5),
            token_0_vault: key(6),
            token_1_vault: key(7),
            token_0_program: TOKEN_PROGRAM,
            token_1_program: TOKEN_2022_PROGRAM,
            tick_arrays: vec![key(8), key(9), key(10)],
            tick_array_bitmap_extension: Some(crate::constants::raydium_clmm_tick_array_bitmap_extension(
                &key(2),
            )),
            quoted_amount_in: Some(100),
            expected_out: Some(90),
            fee_bps: 25,
        };
        let ix = raydium_clmm_swap_leg(&user, &raydium, 100, 90, key(4)).unwrap();
        assert_eq!(ix.accounts.len(), 17);
        assert_eq!(
            ix.accounts[13].pubkey,
            crate::constants::raydium_clmm_tick_array_bitmap_extension(&key(2))
        );
        assert_eq!(ix.data.len(), 41);
        assert_eq!(&ix.data[..8], &CLMM_SWAP_V2);

        let whirlpool = WhirlpoolPool {
            whirlpool: key(1),
            mint_a: key(2),
            mint_b: key(3),
            vault_a: key(4),
            vault_b: key(5),
            token_program_a: TOKEN_PROGRAM,
            token_program_b: TOKEN_PROGRAM,
            tick_arrays: vec![key(6), key(7), key(8)],
            quoted_amount_in: Some(100),
            expected_out: Some(90),
            fee_bps: 30,
        };
        let ix = whirlpool_swap_leg(&user, &whirlpool, 100, 90, key(2)).unwrap();
        assert_eq!(ix.accounts.len(), 15);
        assert_eq!(ix.data.len(), 43);
        assert_eq!(&ix.data[..8], &WHIRLPOOL_SWAP_V2);
        let mut lim = [0u8; 16];
        lim.copy_from_slice(&ix.data[24..40]);
        assert_eq!(u128::from_le_bytes(lim), WHIRLPOOL_MIN_SQRT_PRICE_X64);
        // Must use Orca MAX — Raydium CLMM MAX is larger and reverts on-chain.
        let ix_b2a = whirlpool_swap_leg(&user, &whirlpool, 100, 90, key(3)).unwrap();
        lim.copy_from_slice(&ix_b2a.data[24..40]);
        assert_eq!(u128::from_le_bytes(lim), WHIRLPOOL_MAX_SQRT_PRICE_X64);
        assert_ne!(WHIRLPOOL_MAX_SQRT_PRICE_X64, MAX_SQRT_PRICE_X64);

        let dlmm = MeteoraDlmmPool {
            lb_pair: key(1),
            bitmap_extension: None,
            reserve_x: key(2),
            reserve_y: key(3),
            token_x_mint: key(4),
            token_y_mint: key(5),
            token_x_program: TOKEN_PROGRAM,
            token_y_program: TOKEN_PROGRAM,
            oracle: key(6),
            bin_arrays: vec![key(7), key(8)],
            quoted_amount_in: Some(100),
            expected_out: Some(90),
            fee_bps: 20,
        };
        let ix = meteora_dlmm_swap_leg(&user, &dlmm, 100, 90, key(4)).unwrap();
        assert_eq!(ix.accounts.len(), 18);
        assert_eq!(ix.data.len(), 28);
        assert_eq!(&ix.data[..8], &METEORA_DLMM_SWAP2);
        assert!(!ix.accounts[1].is_writable);
        assert_eq!(ix.accounts[1].pubkey, METEORA_DLMM_PROGRAM);
    }

    #[test]
    fn amm_v4_uses_swap_v2_without_openbook() {
        let user = key(20);
        let pool = RaydiumAmmV4Pool {
            amm: key(1),
            coin_mint: key(2),
            pc_mint: WSOL_MINT,
            token_coin: key(3),
            token_pc: key(4),
            amm_open_orders: Pubkey::default(),
            amm_target_orders: Pubkey::default(),
            serum_program: Pubkey::default(),
            serum_market: Pubkey::default(),
            serum_bids: Pubkey::default(),
            serum_asks: Pubkey::default(),
            serum_event_queue: Pubkey::default(),
            serum_coin_vault_account: Pubkey::default(),
            serum_pc_vault_account: Pubkey::default(),
            serum_vault_signer: Pubkey::default(),
            coin_reserve: 1_000,
            pc_reserve: 2_000,
            trade_fee_numerator: 25,
            swap_fee_numerator: 25,
        };
        assert!(!pool.uses_openbook_market());
        let ix = raydium_amm_v4_swap_leg(&user, &pool, 100, 90, key(2)).unwrap();
        assert_eq!(ix.accounts.len(), 8);
        assert_eq!(ix.data[0], RAYDIUM_AMM_V4_SWAP_BASE_IN_V2);
        assert!(ix.accounts[7].is_signer);
        assert!(!ix.accounts[7].is_writable); // official SDK: owner readonly signer
    }

    #[test]
    fn damm_v2_includes_program_placeholder_and_sysvar() {
        let user = key(20);
        let pool = MeteoraDammV2Pool {
            pool: key(1),
            token_a_vault: key(2),
            token_b_vault: key(3),
            token_a_mint: WSOL_MINT,
            token_b_mint: key(4),
            token_a_program: TOKEN_PROGRAM,
            token_b_program: TOKEN_PROGRAM,
            token_a_reserve: 1_000_000,
            token_b_reserve: 1_000_000,
            fee_bps: 25,
            quoted_amount_in: Some(100),
            expected_out: Some(90),
            swap_mode: METEORA_DAMM_V2_EXACT_IN,
            referral_token_account: None,
            include_rate_limiter_sysvar: true,
        };
        let ix = meteora_damm_v2_swap_leg(&user, &pool, 100, 90, WSOL_MINT).unwrap();
        assert_eq!(ix.accounts.len(), 15);
        assert_eq!(ix.accounts[11].pubkey, METEORA_DAMM_V2_PROGRAM);
        assert!(!ix.accounts[11].is_writable);
        assert_eq!(ix.accounts[14].pubkey, SYSVAR_INSTRUCTIONS);
        assert_eq!(&ix.data[..8], &METEORA_DAMM_V2_SWAP2);
    }

    #[test]
    fn pumpfun_v1_trailing_buyback_is_writable() {
        let user = key(20);
        let mut pool = pumpfun_v2_pool();
        pool.quote_mint = WSOL_MINT;
        pool.use_v2 = false;
        pool.buyback_fee_recipient = PUMPFUN_BUYBACK_FEE_RECIPIENT;
        // Foreign UVA must be ignored — derive from signer.
        pool.user_volume_accumulator = key(99);
        let buy = pumpfun_buy_leg(&user, &pool, 100, 90, ata(&user, &pool.mint, &TOKEN_PROGRAM));
        let sell = pumpfun_sell_leg(&user, &pool, 100, 90, ata(&user, &pool.mint, &TOKEN_PROGRAM));
        let expected_uva = pumpfun_user_volume_accumulator(&user);
        assert_eq!(buy.accounts[13].pubkey, expected_uva);
        assert_ne!(buy.accounts[13].pubkey, key(99));
        // Official: accounts = IDL + bonding_curve_v2 + buybackFeeRecipient(writable)
        assert_eq!(buy.accounts.len(), 18);
        assert_eq!(buy.accounts[17].pubkey, pool.buyback_fee_recipient);
        assert!(buy.accounts[17].is_writable);
        assert!(sell.accounts.last().unwrap().is_writable);
        assert_eq!(sell.accounts.last().unwrap().pubkey, pool.buyback_fee_recipient);
    }

    #[test]
    fn pumpswap_uses_observed_protocol_fee_recipient() {
        let user = key(20);
        let mut pool = PumpSwapPool {
            pool: key(1),
            base_mint: key(2),
            quote_mint: WSOL_MINT,
            pool_base_token_account: key(3),
            pool_quote_token_account: key(4),
            base_token_program: TOKEN_PROGRAM,
            quote_token_program: TOKEN_PROGRAM,
            coin_creator_vault_ata: key(5),
            coin_creator_vault_authority: key(6),
            coin_creator: Pubkey::default(),
            base_reserve: 1,
            quote_reserve: 1,
            virtual_quote_reserves: 0,
            lp_fee_bps: 20,
            protocol_fee_bps: 5,
            creator_fee_bps: 0,
            is_cashback_coin: false,
            protocol_fee_recipient: PUMP_MAYHEM_FEE_RECIPIENT,
            buyback_fee_recipient: PUMPSWAP_BUYBACK_FEE_RECIPIENT,
        };
        let ix = pumpswap_buy_leg(&user, &pool, 100, 90).unwrap();
        assert_eq!(ix.accounts[9].pubkey, PUMP_MAYHEM_FEE_RECIPIENT);
        assert!(!ix.accounts[9].is_writable);
        pool.protocol_fee_recipient = Pubkey::default();
        let ix2 = pumpswap_buy_leg(&user, &pool, 100, 90).unwrap();
        assert_eq!(ix2.accounts[9].pubkey, PUMPSWAP_PROTOCOL_FEE_RECIPIENT);
    }
}
