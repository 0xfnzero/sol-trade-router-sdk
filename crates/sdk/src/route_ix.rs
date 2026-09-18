//! Encode `Route` instruction for the on-chain Pinocchio router.

use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    system_program,
};

use crate::{
    config_pda,
    constants::{TOKEN_2022_PROGRAM, TOKEN_PROGRAM},
    legs::Leg,
};

pub const TAG_ROUTE: u8 = 2;
pub const FEE_ASSET_SOL: u8 = 0;
pub const FEE_ASSET_TOKEN: u8 = 1;

pub struct RouteAccounts {
    pub payer: Pubkey,
    pub fee_destination: Pubkey,
    pub fee_source: Pubkey,
    /// Token account whose balance delta is checked against `min_amount_out`.
    pub output_token_account: Pubkey,
    /// System program for SOL fee, or SPL Token / Token-2022 for token fee.
    pub fee_program: Pubkey,
}

pub fn build_route_instruction(
    program_id: &Pubkey,
    accounts: RouteAccounts,
    amount_in: u64,
    min_amount_out: u64,
    fee_asset: u8,
    legs: &[Leg],
) -> Instruction {
    let (config, _) = config_pda(program_id);

    let mut data = Vec::with_capacity(64);
    data.push(TAG_ROUTE);
    data.extend_from_slice(&amount_in.to_le_bytes());
    data.extend_from_slice(&min_amount_out.to_le_bytes());
    data.push(fee_asset);
    data.push(legs.len() as u8);

    let mut metas = vec![
        AccountMeta::new(accounts.payer, true),
        AccountMeta::new_readonly(config, false),
        AccountMeta::new(accounts.fee_destination, false),
        AccountMeta::new(accounts.fee_source, false),
        AccountMeta::new(accounts.output_token_account, false),
        AccountMeta::new_readonly(accounts.fee_program, false),
    ];

    // Leg accounts first — on-chain consumes these by num_accounts per leg.
    for leg in legs {
        data.extend_from_slice(leg.program_id.as_ref());
        data.push(leg.accounts.len() as u8);
        data.extend_from_slice(&(leg.data.len() as u16).to_le_bytes());
        data.extend_from_slice(&leg.data);
        metas.extend(leg.accounts.iter().cloned());
    }

    // DEX programs after leg accounts (must appear in the tx for CPI; chain ignores leftover).
    let mut seen_programs = Vec::new();
    for leg in legs {
        if !seen_programs.contains(&leg.program_id) {
            metas.push(AccountMeta::new_readonly(leg.program_id, false));
            seen_programs.push(leg.program_id);
        }
    }

    Instruction {
        program_id: *program_id,
        accounts: metas,
        data,
    }
}

pub fn sol_fee_program() -> Pubkey {
    system_program::id()
}

pub fn spl_fee_program() -> Pubkey {
    TOKEN_PROGRAM
}

/// Pick Token vs Token-2022 for fee CPI based on the fee_source ATA's token program.
pub fn token_fee_program(token_program: &Pubkey) -> Pubkey {
    if *token_program == TOKEN_2022_PROGRAM {
        TOKEN_2022_PROGRAM
    } else {
        TOKEN_PROGRAM
    }
}
