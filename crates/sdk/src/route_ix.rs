//! Encode `Route` instruction for the on-chain Pinocchio router.

use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
};

use crate::{
    config_pda,
    constants::{SYSTEM_PROGRAM, TOKEN_2022_PROGRAM, TOKEN_PROGRAM},
    legs::Leg,
};

pub const TAG_ROUTE: u8 = 2;
pub const FEE_ASSET_SOL: u8 = 0;
pub const FEE_ASSET_TOKEN: u8 = 1;
/// OR into `fee_asset`: `amount_in` is a max budget (exact-out). On-chain checks
/// `spent <= amount_in` instead of `spent >= amount_in`.
pub const FEE_ASSET_EXACT_OUT: u8 = 0x80;

pub struct RouteAccounts {
    pub payer: Pubkey,
    pub fee_destination: Pubkey,
    pub fee_source: Pubkey,
    /// Token account whose balance delta is checked against `min_amount_out`.
    pub output_token_account: Pubkey,
    /// System program for SOL fee, or SPL Token / Token-2022 for token fee.
    pub fee_program: Pubkey,
    /// Retained for API compatibility; not currently passed on-chain (Token
    /// transfer CPI does not require the mint account).
    pub fee_mint: Pubkey,
}

pub fn build_route_instruction(
    program_id: &Pubkey,
    accounts: RouteAccounts,
    amount_in: u64,
    min_amount_out: u64,
    fee_asset: u8,
    _expected_output_mint: &Pubkey,
    legs: &[Leg],
) -> Instruction {
    build_route_instruction_ex(
        program_id,
        accounts,
        amount_in,
        min_amount_out,
        fee_asset,
        false,
        legs,
    )
}

/// Same as [`build_route_instruction`] with an explicit exact-out fee check flag.
pub fn build_route_instruction_ex(
    program_id: &Pubkey,
    accounts: RouteAccounts,
    amount_in: u64,
    min_amount_out: u64,
    fee_asset: u8,
    exact_out: bool,
    legs: &[Leg],
) -> Instruction {
    let (config, _) = config_pda(program_id);
    let fee_asset_byte = if exact_out {
        (fee_asset & 0x01) | FEE_ASSET_EXACT_OUT
    } else {
        fee_asset & 0x01
    };

    let mut data = Vec::with_capacity(64 + legs.len() * 48);
    data.push(TAG_ROUTE);
    data.extend_from_slice(&amount_in.to_le_bytes());
    data.extend_from_slice(&min_amount_out.to_le_bytes());
    data.push(fee_asset_byte);
    data.push(legs.len() as u8);

    // Account layout must match on-chain `route::process` (6 fixed + remaining).
    let mut metas = Vec::with_capacity(6 + 32);
    metas.push(AccountMeta::new(accounts.payer, true));
    metas.push(AccountMeta::new_readonly(config, false));
    metas.push(AccountMeta::new(accounts.fee_destination, false));
    metas.push(AccountMeta::new(accounts.fee_source, false));
    metas.push(AccountMeta::new(accounts.output_token_account, false));
    metas.push(AccountMeta::new_readonly(accounts.fee_program, false));
    let _ = accounts.fee_mint;

    for leg in legs {
        data.extend_from_slice(leg.program_id.as_ref());
        data.push(leg.accounts.len() as u8);
        data.extend_from_slice(&(leg.data.len() as u16).to_le_bytes());
        data.extend_from_slice(&leg.data);
        metas.extend(leg.accounts.iter().cloned());
    }

    let mut seen = [Pubkey::default(); 4];
    let mut seen_n = 0usize;
    for leg in legs {
        let already = seen[..seen_n].iter().any(|p| p == &leg.program_id);
        if already {
            continue;
        }
        metas.push(AccountMeta::new_readonly(leg.program_id, false));
        if seen_n < seen.len() {
            seen[seen_n] = leg.program_id;
            seen_n += 1;
        }
    }

    Instruction {
        program_id: *program_id,
        accounts: metas,
        data,
    }
}

pub fn sol_fee_program() -> Pubkey {
    SYSTEM_PROGRAM
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
