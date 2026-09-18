//! Build individual DEX legs (AccountMeta + data) for the router CPI payload.
//! Hot path: stack buffers + const AccountMetas (aligned with sol-trade-sdk).

use anyhow::{anyhow, Result};
use solana_sdk::{instruction::AccountMeta, pubkey::Pubkey};

use crate::{
    constants::*,
    market::{CpmmPool, LaunchLabPool, PumpFunPool},
};

#[derive(Clone, Debug)]
pub struct Leg {
    pub program_id: Pubkey,
    pub accounts: Vec<AccountMeta>,
    pub data: Vec<u8>,
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
    let (input_vault, output_vault, input_tp, output_tp) = if input_mint == pool.base_mint
        && output_mint == pool.quote_mint
    {
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
        AccountMeta::new(*user, true),
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
        AccountMeta::new(pool.global_volume_accumulator, false),
        AccountMeta::new(pool.user_volume_accumulator, false),
        AccountMeta::new_readonly(pool.fee_config, false),
        AccountMeta::new_readonly(pool.fee_program, false),
        AccountMeta::new_readonly(pool.bonding_curve_v2, false),
        AccountMeta::new(pool.protocol_fee_recipient, false),
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
        accounts.push(AccountMeta::new(pool.user_volume_accumulator, false));
    }
    accounts.push(AccountMeta::new_readonly(pool.bonding_curve_v2, false));
    accounts.push(AccountMeta::new(pool.protocol_fee_recipient, false));

    Leg {
        program_id: PUMPFUN_PROGRAM,
        accounts,
        data: data.to_vec(),
    }
}
