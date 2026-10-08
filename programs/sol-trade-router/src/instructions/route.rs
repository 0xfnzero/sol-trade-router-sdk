//! Multi-hop route: take platform fee, then CPI each DEX leg.
//!
//! # Accounts
//! 0. `[signer]` user
//! 1. `[]` config PDA (must be this program's PDA, owned by this program)
//! 2. `[writable]` fee_destination
//! 3. `[writable]` fee_source (SOL: user; Token: user ATA owned by user)
//! 4. `[writable]` user_output_token (token account, or user for native SOL)
//! 5. `[]` fee_program (System for SOL fee; SPL Token / Token-2022 for token fee)
//!    6.. : remaining — concatenated per-leg AccountMetas, then optional DEX program ids
//!
//! # Data (after 1-byte tag stripped by entrypoint)
//! ```text
//! amount_in: u64       // total input from fee_source (fee + swap spend)
//! min_amount_out: u64
//! fee_asset: u8        // 0=SOL 1=SPL
//! num_legs: u8
//! output_mint: [u8;32] // SystemProgram (=0) means native SOL on user
//! for each leg:
//!   program_id: [u8;32]
//!   num_accounts: u8
//!   data_len: u16
//!   data: [u8; data_len]
//! ```
//!
//! # Fee integrity
//! Fee = amount_in * fee_bps / 10_000 is taken from `fee_source` first.
//! Exact-in requires `fee_source` spend == `amount_in` (fee + swap).
//! Exact-out requires fee <= spend <= amount_in (the declared max budget).
//!
//! The separate `ROUTE_DYNAMIC` tag keeps this legacy format intact. It adds
//! `intermediate_min_out: u64` after `output_mint` and one writable intermediate
//! token account after the six fixed accounts. It requires exactly two legs;
//! the second must be LaunchLab buyExactIn or a supported exact-input pool swap.
//!
//! `ROUTE_DYNAMIC_THREE` keeps both earlier formats intact. It adds two
//! intermediate minimum outputs and two writable intermediate token accounts.
//! Its middle leg is a supported exact-input pool swap; its final leg is
//! LaunchLab buyExactIn or a supported exact-input pool swap. A zero input in
//! either later leg is replaced with the previous leg's *newly credited*
//! token balance before CPI.

use core::mem::MaybeUninit;

use pinocchio::{
    cpi::invoke_with_slice,
    error::ProgramError,
    instruction::{InstructionAccount, InstructionView},
    AccountView, Address, ProgramResult,
};
use pinocchio_system::instructions::Transfer as SystemTransfer;
use pinocchio_token::instructions::Transfer as TokenTransfer;

use crate::{
    state::{
        is_token_program, token_amount, token_mint, token_owner, RouterConfig, CONFIG_SEED,
        SYSTEM_PROGRAM_ID, TOKEN_2022_PROGRAM_ID, TOKEN_PROGRAM_ID,
    },
    RouterError,
};

const MAX_LEGS: usize = 4;
/// Must stay in sync with SDK `legs::MAX_LEG_ACCOUNTS`.
const MAX_LEG_ACCOUNTS: usize = 64;
/// Legacy tag-2 header: input(8), minimum(8), fee asset(1), legs(1), output mint(32).
const ROUTE_HEADER_LEN: usize = 50;
const LAUNCHLAB_PROGRAM_ID: [u8; 32] = [
    5, 4, 59, 149, 77, 202, 38, 225, 239, 145, 181, 44, 79, 143, 137, 175, 138, 111, 90, 200, 198,
    33, 86, 241, 113, 207, 15, 33, 172, 81, 201, 34,
];
const RAYDIUM_CPMM_PROGRAM_ID: [u8; 32] = [
    169, 42, 90, 139, 79, 41, 89, 82, 132, 37, 80, 170, 147, 253, 91, 149, 181, 172, 230, 168, 235,
    146, 12, 147, 148, 46, 67, 105, 12, 32, 236, 115,
];
const METEORA_DLMM_PROGRAM_ID: [u8; 32] = [
    4, 233, 225, 47, 188, 132, 232, 38, 201, 50, 204, 233, 226, 100, 12, 206, 21, 89, 12, 28, 98,
    115, 176, 146, 87, 8, 186, 59, 133, 32, 176, 188,
];
const RAYDIUM_AMM_V4_PROGRAM_ID: [u8; 32] = [
    75, 217, 73, 196, 54, 2, 195, 63, 32, 119, 144, 237, 22, 163, 82, 76, 161, 185, 151, 92, 241,
    33, 162, 169, 12, 255, 236, 125, 248, 182, 138, 205,
];
const RAYDIUM_CLMM_PROGRAM_ID: [u8; 32] = [
    165, 213, 202, 158, 4, 207, 93, 181, 144, 183, 20, 186, 47, 227, 44, 177, 89, 19, 63, 193, 193,
    146, 183, 34, 87, 253, 7, 211, 156, 176, 64, 30,
];
const METEORA_DAMM_V2_PROGRAM_ID: [u8; 32] = [
    9, 45, 33, 53, 101, 122, 21, 156, 43, 135, 212, 182, 106, 112, 219, 142, 151, 82, 56, 159, 247,
    106, 175, 32, 108, 237, 6, 58, 56, 249, 90, 237,
];
const ORCA_WHIRLPOOL_PROGRAM_ID: [u8; 32] = [
    14, 3, 104, 95, 142, 144, 144, 83, 228, 88, 18, 28, 102, 245, 167, 106, 237, 199, 112, 106,
    161, 28, 130, 248, 170, 149, 42, 143, 43, 120, 121, 169,
];
const PUMPSWAP_PROGRAM_ID: [u8; 32] = [
    12, 20, 222, 252, 130, 94, 198, 118, 148, 37, 8, 24, 187, 101, 64, 101, 244, 41, 141, 49, 86,
    213, 113, 180, 212, 248, 9, 12, 24, 233, 168, 99,
];
const LAUNCHLAB_BUY_EXACT_IN: [u8; 8] = [250, 234, 13, 123, 213, 156, 19, 236];
const CPMM_SWAP_BASE_IN: [u8; 8] = [143, 190, 90, 218, 196, 30, 51, 222];
const PUMPSWAP_SELL: [u8; 8] = [51, 230, 133, 164, 1, 127, 131, 173];
const METEORA_DLMM_SWAP2: [u8; 8] = [65, 75, 63, 76, 235, 91, 91, 136];
const CLMM_SWAP_V2: [u8; 8] = [43, 4, 237, 11, 26, 201, 30, 98];
const NO_ACCOUNT: usize = usize::MAX;
const MAX_DYNAMIC_LEG_DATA: usize = 49;

#[derive(Clone, Copy)]
struct DynamicSecondLeg {
    input_account: usize,
    output_account: usize,
    signer: usize,
    amount_offset: usize,
    input_mint: usize,
    input_program: usize,
    output_mint: usize,
    output_program: usize,
    paired_mints: bool,
}

fn valid_whirlpool_remaining_accounts(data: &[u8]) -> bool {
    (data.len() == 43 && data[42] == 0)
        || (data.len() == 49 && data[42..48] == [1, 1, 0, 0, 0, 6]
            && (1..=3).contains(&data[48]))
}

// Both dynamic hops use this DEX switch; keep one copy in the SBF binary.
#[inline(never)]
fn dynamic_second_leg(program_id: &Address, data: &[u8]) -> Result<DynamicSecondLeg, ProgramError> {
    if addr_eq(program_id, &LAUNCHLAB_PROGRAM_ID)
        && data.len() == 32
        && data[..8] == LAUNCHLAB_BUY_EXACT_IN
        && data[24..32] == [0u8; 8]
    {
        Ok(DynamicSecondLeg {
            input_account: 6,
            output_account: 5,
            signer: 0,
            amount_offset: 8,
            input_mint: 10,
            input_program: 12,
            output_mint: 9,
            output_program: 11,
            paired_mints: false,
        })
    } else if addr_eq(program_id, &RAYDIUM_CPMM_PROGRAM_ID)
        && data.len() == 24
        && data[..8] == CPMM_SWAP_BASE_IN
    {
        Ok(DynamicSecondLeg {
            input_account: 4,
            output_account: 5,
            signer: 0,
            amount_offset: 8,
            input_mint: 10,
            input_program: 8,
            output_mint: 11,
            output_program: 9,
            paired_mints: false,
        })
    } else if addr_eq(program_id, &PUMPSWAP_PROGRAM_ID)
        && data.len() == 24
        && data[..8] == PUMPSWAP_SELL
    {
        // PumpSwap sell spends base exactly and receives quote.
        Ok(DynamicSecondLeg {
            input_account: 5,
            output_account: 6,
            signer: 1,
            amount_offset: 8,
            input_mint: 3,
            input_program: 11,
            output_mint: 4,
            output_program: 12,
            paired_mints: false,
        })
    } else if addr_eq(program_id, &RAYDIUM_AMM_V4_PROGRAM_ID) && data.len() == 17 && data[0] == 16 {
        Ok(DynamicSecondLeg {
            input_account: 5,
            output_account: 6,
            signer: 7,
            amount_offset: 1,
            input_mint: NO_ACCOUNT,
            input_program: 0,
            output_mint: NO_ACCOUNT,
            output_program: 0,
            paired_mints: false,
        })
    } else if addr_eq(program_id, &RAYDIUM_CLMM_PROGRAM_ID)
        && data.len() == 41
        && data[..8] == CLMM_SWAP_V2
        && data[40] == 1
    {
        Ok(DynamicSecondLeg {
            input_account: 3,
            output_account: 4,
            signer: 0,
            amount_offset: 8,
            input_mint: 11,
            input_program: NO_ACCOUNT,
            output_mint: 12,
            output_program: NO_ACCOUNT,
            paired_mints: false,
        })
    } else if addr_eq(program_id, &METEORA_DLMM_PROGRAM_ID)
        && data.len() == 28
        && data[..8] == METEORA_DLMM_SWAP2
        && data[24..28] == [0u8; 4]
    {
        Ok(DynamicSecondLeg {
            input_account: 4,
            output_account: 5,
            signer: 10,
            amount_offset: 8,
            input_mint: 6,
            input_program: 11,
            output_mint: 7,
            output_program: 12,
            paired_mints: true,
        })
    } else if addr_eq(program_id, &METEORA_DAMM_V2_PROGRAM_ID)
        && data.len() == 25
        && data[..8] == METEORA_DLMM_SWAP2
        && data[24] == 0
    {
        Ok(DynamicSecondLeg {
            input_account: 2,
            output_account: 3,
            signer: 8,
            amount_offset: 8,
            input_mint: 6,
            input_program: 9,
            output_mint: 7,
            output_program: 10,
            paired_mints: true,
        })
    } else if addr_eq(program_id, &ORCA_WHIRLPOOL_PROGRAM_ID)
        && valid_whirlpool_remaining_accounts(data)
        && data[..8] == CLMM_SWAP_V2
        && data[40] == 1
        && data[41] <= 1
    {
        let a_to_b = data[41] == 1;
        Ok(DynamicSecondLeg {
            input_account: if a_to_b { 7 } else { 9 },
            output_account: if a_to_b { 9 } else { 7 },
            signer: 3,
            amount_offset: 8,
            input_mint: if a_to_b { 5 } else { 6 },
            input_program: if a_to_b { 0 } else { 1 },
            output_mint: if a_to_b { 6 } else { 5 },
            output_program: if a_to_b { 1 } else { 0 },
            paired_mints: false,
        })
    } else {
        Err(RouterError::InvalidDynamicLeg.into())
    }
}

#[inline(always)]
fn validate_dynamic_accounts(
    layout: DynamicSecondLeg,
    leg_accounts: &[AccountView],
    user: &AccountView,
    input: &AccountView,
    output: &AccountView,
) -> ProgramResult {
    let required = [
        layout.input_account,
        layout.output_account,
        layout.signer,
        layout.input_mint,
        layout.input_program,
        layout.output_mint,
        layout.output_program,
    ];
    if required
        .iter()
        .any(|&i| i != NO_ACCOUNT && i >= leg_accounts.len())
        || leg_accounts[layout.signer].address() != user.address()
        || !leg_accounts[layout.signer].is_signer()
        || leg_accounts[layout.input_account].address() != input.address()
        || !leg_accounts[layout.input_account].is_writable()
        || leg_accounts[layout.output_account].address() != output.address()
        || !leg_accounts[layout.output_account].is_writable()
    {
        return Err(RouterError::InvalidDynamicLeg.into());
    }
    let input_mint = token_mint(input)?;
    let output_mint = token_mint(output)?;
    if layout.paired_mints {
        let forward = leg_accounts[layout.input_mint].address() == &input_mint
            && leg_accounts[layout.output_mint].address() == &output_mint
            && leg_accounts[layout.input_program].address() == input.owner()
            && leg_accounts[layout.output_program].address() == output.owner();
        let reverse = leg_accounts[layout.output_mint].address() == &input_mint
            && leg_accounts[layout.input_mint].address() == &output_mint
            && leg_accounts[layout.output_program].address() == input.owner()
            && leg_accounts[layout.input_program].address() == output.owner();
        if !forward && !reverse {
            return Err(RouterError::InvalidDynamicLeg.into());
        }
    } else if (layout.input_mint != NO_ACCOUNT
        && leg_accounts[layout.input_mint].address() != &input_mint)
        || (layout.output_mint != NO_ACCOUNT
            && leg_accounts[layout.output_mint].address() != &output_mint)
        || (layout.input_program != NO_ACCOUNT
            && leg_accounts[layout.input_program].address() != input.owner())
        || (layout.output_program != NO_ACCOUNT
            && leg_accounts[layout.output_program].address() != output.owner())
    {
        return Err(RouterError::InvalidDynamicLeg.into());
    }
    Ok(())
}

#[inline(always)]
fn received_quote(before: u64, after: u64, minimum: u64) -> Result<u64, ProgramError> {
    let received = after
        .checked_sub(before)
        .ok_or(RouterError::InvalidIntermediateAccount)?;
    if received < minimum {
        return Err(RouterError::SlippageExceeded.into());
    }
    Ok(received)
}

#[inline(always)]
fn patch_second_leg_amount<'a>(
    data: &[u8],
    amount: u64,
    offset: usize,
    patched: &'a mut [u8; MAX_DYNAMIC_LEG_DATA],
) -> &'a [u8] {
    patched[..data.len()].copy_from_slice(data);
    patched[offset..offset + 8].copy_from_slice(&amount.to_le_bytes());
    &patched[..data.len()]
}

#[inline(always)]
fn read_u16(data: &[u8], offset: usize) -> Result<u16, ProgramError> {
    let bytes = data
        .get(offset..offset + 2)
        .ok_or(RouterError::InvalidInstructionData)?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

#[inline(always)]
fn read_u64(data: &[u8], offset: usize) -> Result<u64, ProgramError> {
    let bytes = data
        .get(offset..offset + 8)
        .ok_or(RouterError::InvalidInstructionData)?;
    let mut buf = [0u8; 8];
    buf.copy_from_slice(bytes);
    Ok(u64::from_le_bytes(buf))
}

#[inline(always)]
fn checked_fee(amount_in: u64, fee_bps: u16) -> Result<u64, ProgramError> {
    if fee_bps == 0 {
        return Ok(0);
    }
    amount_in
        .checked_mul(fee_bps as u64)
        .and_then(|v| v.checked_div(10_000))
        .ok_or_else(|| RouterError::ArithmeticOverflow.into())
}

#[inline(always)]
fn addr_eq(a: &Address, b: &[u8; 32]) -> bool {
    a.as_array() == b
}

#[inline(always)]
fn validate_output(
    user: &AccountView,
    output: &AccountView,
    expected_mint: &[u8; 32],
    dynamic: bool,
) -> Result<bool, ProgramError> {
    let native_sol = *expected_mint == SYSTEM_PROGRAM_ID;
    if native_sol {
        // Dynamic routes settle only in tokens; native SOL must belong to the signer.
        if dynamic || output.address() != user.address() {
            return Err(RouterError::InvalidOutputAccount.into());
        }
    } else {
        if !is_token_program(output.owner()) {
            return Err(RouterError::InvalidOutputAccount.into());
        }
        if token_mint(output)?.as_array() != expected_mint {
            return Err(RouterError::InvalidOutputMint.into());
        }
        if dynamic && token_owner(output)? != *user.address() {
            return Err(RouterError::InvalidOutputAccount.into());
        }
    }
    Ok(native_sol)
}

#[inline(always)]
fn output_balance(output: &AccountView, native_sol: bool) -> Result<u64, ProgramError> {
    if native_sol {
        Ok(output.lamports())
    } else {
        token_amount(output)
    }
}

#[inline(always)]
fn validate_spend(
    before: u64,
    after: u64,
    amount_in: u64,
    fee: u64,
    exact_out: bool,
) -> ProgramResult {
    let spent = before
        .checked_sub(after)
        .ok_or(RouterError::FeeSourceMismatch)?;
    if (exact_out && (spent > amount_in || spent < fee)) || (!exact_out && spent != amount_in) {
        return Err(RouterError::FeeSourceMismatch.into());
    }
    Ok(())
}

pub fn process(program_id: &Address, accounts: &mut [AccountView], data: &[u8]) -> ProgramResult {
    process_inner(program_id, accounts, data, 0)
}

pub fn process_dynamic(
    program_id: &Address,
    accounts: &mut [AccountView],
    data: &[u8],
) -> ProgramResult {
    process_inner(program_id, accounts, data, 1)
}

pub fn process_dynamic_three(
    program_id: &Address,
    accounts: &mut [AccountView],
    data: &[u8],
) -> ProgramResult {
    process_inner(program_id, accounts, data, 2)
}

fn process_inner(
    program_id: &Address,
    accounts: &mut [AccountView],
    data: &[u8],
    dynamic_intermediates: usize,
) -> ProgramResult {
    let dynamic = dynamic_intermediates != 0;
    let three_hop = dynamic_intermediates == 2;
    if data.len() < ROUTE_HEADER_LEN + 8 * dynamic_intermediates {
        return Err(RouterError::InvalidInstructionData.into());
    }

    let amount_in = read_u64(data, 0)?;
    let min_amount_out = read_u64(data, 8)?;
    let fee_asset = data[16];
    let num_legs = data[17] as usize;
    let mut expected_output_mint = [0u8; 32];
    expected_output_mint.copy_from_slice(&data[18..ROUTE_HEADER_LEN]);
    let intermediate_min_out = if dynamic {
        read_u64(data, ROUTE_HEADER_LEN)?
    } else {
        0
    };
    let second_intermediate_min_out = if three_hop {
        read_u64(data, ROUTE_HEADER_LEN + 8)?
    } else {
        0
    };

    if amount_in == 0 {
        return Err(RouterError::InvalidInstructionData.into());
    }
    if !(1..=MAX_LEGS).contains(&num_legs) {
        return Err(RouterError::TooManyLegs.into());
    }
    if dynamic
        && (num_legs != dynamic_intermediates + 1
            || data[16] != 1
            || intermediate_min_out == 0
            || (three_hop && second_intermediate_min_out == 0)
            || min_amount_out == 0)
    {
        return Err(RouterError::InvalidInstructionData.into());
    }
    // Low bit = asset (0 SOL / 1 SPL). Bit 7 = exact-out (amount_in is max budget).
    let exact_out = (fee_asset & 0x80) != 0;
    let fee_asset = fee_asset & 0x01;

    let (user, rest) = accounts
        .split_first_mut()
        .ok_or(RouterError::InsufficientAccounts)?;
    let (config_acc, rest) = rest
        .split_first_mut()
        .ok_or(RouterError::InsufficientAccounts)?;
    let (fee_destination, rest) = rest
        .split_first_mut()
        .ok_or(RouterError::InsufficientAccounts)?;
    let (fee_source, rest) = rest
        .split_first_mut()
        .ok_or(RouterError::InsufficientAccounts)?;
    let (user_output, rest) = rest
        .split_first_mut()
        .ok_or(RouterError::InsufficientAccounts)?;
    let (fee_program, remaining) = rest
        .split_first_mut()
        .ok_or(RouterError::InsufficientAccounts)?;
    let (intermediate_acc, remaining) = if dynamic {
        let (intermediate, remaining) = remaining
            .split_first_mut()
            .ok_or(RouterError::InsufficientAccounts)?;
        (Some(intermediate), remaining)
    } else {
        (None, remaining)
    };
    let (second_intermediate_acc, remaining) = if three_hop {
        let (intermediate, remaining) = remaining
            .split_first_mut()
            .ok_or(RouterError::InsufficientAccounts)?;
        (Some(intermediate), remaining)
    } else {
        (None, remaining)
    };

    if !user.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // C1: config must be this program's PDA and owned by this program.
    if config_acc.owner() != program_id {
        return Err(RouterError::InvalidConfig.into());
    }

    let cfg = RouterConfig::read(config_acc)?;
    // The bump was fixed when initialize created the config. Checking that
    // single candidate avoids searching for a bump on every swap.
    let bump_seed = [cfg.bump];
    let expected_config = Address::create_program_address(&[CONFIG_SEED, &bump_seed], program_id)
        .map_err(|_| RouterError::InvalidConfig)?;
    if config_acc.address() != &expected_config {
        return Err(RouterError::InvalidConfig.into());
    }
    if cfg.paused != 0 {
        return Err(RouterError::Paused.into());
    }

    let native_sol_out = validate_output(user, user_output, &expected_output_mint, dynamic)?;

    let fee = checked_fee(amount_in, cfg.fee_bps)?;

    // Snapshot fee_source before fee + legs (C2).
    let fee_source_before = match fee_asset {
        0 => {
            // This binding also applies when rounding or a zero fee skips the fee CPI.
            if fee_source.address() != user.address() {
                return Err(RouterError::InvalidFeeAsset.into());
            }
            fee_source.lamports()
        }
        1 => {
            if !is_token_program(fee_source.owner()) {
                return Err(RouterError::InvalidFeeAsset.into());
            }
            // fee_source must be owned by the signer
            if token_owner(fee_source)? != *user.address() {
                return Err(RouterError::InvalidFeeAsset.into());
            }
            token_amount(fee_source)?
        }
        _ => return Err(RouterError::InvalidFeeAsset.into()),
    };

    if fee > 0 {
        match fee_asset {
            0 => {
                // C4: SOL fee uses System Program
                if !addr_eq(fee_program.address(), &SYSTEM_PROGRAM_ID) {
                    return Err(RouterError::InvalidProgramId.into());
                }
                if fee_destination.address() != &cfg.fee_recipient {
                    return Err(RouterError::InvalidFeeAsset.into());
                }
                // fee_source for SOL must be the signer
                if fee_source.address() != user.address() {
                    return Err(RouterError::InvalidFeeAsset.into());
                }
                SystemTransfer {
                    from: fee_source,
                    to: fee_destination,
                    lamports: fee,
                }
                .invoke()?;
            }
            1 => {
                // C4: SPL fee program must be Token or Token-2022
                let fp = fee_program.address();
                if !addr_eq(fp, &TOKEN_PROGRAM_ID) && !addr_eq(fp, &TOKEN_2022_PROGRAM_ID) {
                    return Err(RouterError::InvalidProgramId.into());
                }
                if fee_program.address() != fee_source.owner() {
                    return Err(RouterError::InvalidProgramId.into());
                }
                // C3: fee_destination must be a token account owned by fee_recipient,
                // same mint as fee_source.
                if !is_token_program(fee_destination.owner()) {
                    return Err(RouterError::InvalidFeeAsset.into());
                }
                if fee_destination.owner() != fee_source.owner() {
                    return Err(RouterError::InvalidFeeAsset.into());
                }
                if token_owner(fee_destination)? != cfg.fee_recipient {
                    return Err(RouterError::InvalidFeeAsset.into());
                }
                if token_mint(fee_destination)? != token_mint(fee_source)? {
                    return Err(RouterError::InvalidFeeAsset.into());
                }
                // Program id already verified above (Token / Token-2022).
                TokenTransfer::new(fee_source, fee_destination, user, fee)
                    .invoke_with_unverified_program(fee_program.address())?;
            }
            _ => return Err(RouterError::InvalidFeeAsset.into()),
        }
    }

    let output_before = output_balance(user_output, native_sol_out)?;
    let quote_before = if let Some(intermediate) = intermediate_acc.as_ref() {
        if !intermediate.is_writable()
            || !is_token_program(intermediate.owner())
            || token_owner(intermediate)? != *user.address()
            || intermediate.address() == fee_source.address()
            || intermediate.address() == user_output.address()
        {
            return Err(RouterError::InvalidIntermediateAccount.into());
        }
        token_amount(intermediate)?
    } else {
        0
    };
    let second_intermediate_before = if let Some(second) = second_intermediate_acc.as_ref() {
        if !second.is_writable()
            || !is_token_program(second.owner())
            || token_owner(second)? != *user.address()
            || second.address() == fee_source.address()
            || second.address() == user_output.address()
            || intermediate_acc
                .as_ref()
                .is_some_and(|first| second.address() == first.address())
        {
            return Err(RouterError::InvalidIntermediateAccount.into());
        }
        token_amount(second)?
    } else {
        0
    };
    let mut quote_received = 0u64;

    let mut cursor = ROUTE_HEADER_LEN + 8 * dynamic_intermediates;
    let mut remaining_offset = 0usize;

    for leg_index in 0..num_legs {
        let program_bytes = data
            .get(cursor..cursor + 32)
            .ok_or(RouterError::InvalidLeg)?;
        let mut pid_bytes = [0u8; 32];
        pid_bytes.copy_from_slice(program_bytes);
        let program_id_leg = Address::new_from_array(pid_bytes);
        cursor += 32;

        let num_accounts = *data.get(cursor).ok_or(RouterError::InvalidLeg)? as usize;
        cursor += 1;
        let data_len = read_u16(data, cursor)? as usize;
        cursor += 2;

        if num_accounts == 0 || num_accounts > MAX_LEG_ACCOUNTS {
            return Err(RouterError::InvalidLeg.into());
        }
        let leg_data = data
            .get(cursor..cursor + data_len)
            .ok_or(RouterError::InvalidLeg)?;
        cursor += data_len;

        let end = remaining_offset
            .checked_add(num_accounts)
            .ok_or(RouterError::ArithmeticOverflow)?;
        if end > remaining.len() {
            return Err(RouterError::InsufficientAccounts.into());
        }
        let leg_accounts = &remaining[remaining_offset..end];
        remaining_offset = end;

        let mut patched_data = [0u8; MAX_DYNAMIC_LEG_DATA];
        let invoke_data = if three_hop && leg_index == 1 {
            let input = intermediate_acc
                .as_ref()
                .ok_or(RouterError::InvalidIntermediateAccount)?;
            let output = second_intermediate_acc
                .as_ref()
                .ok_or(RouterError::InvalidIntermediateAccount)?;
            if addr_eq(&program_id_leg, &LAUNCHLAB_PROGRAM_ID) {
                return Err(RouterError::InvalidDynamicLeg.into());
            }
            let layout = dynamic_second_leg(&program_id_leg, leg_data)?;
            if leg_data[layout.amount_offset..layout.amount_offset + 8] != [0u8; 8] {
                return Err(RouterError::InvalidDynamicLeg.into());
            }
            validate_dynamic_accounts(layout, leg_accounts, user, input, output)?;
            patch_second_leg_amount(
                leg_data,
                quote_received,
                layout.amount_offset,
                &mut patched_data,
            )
        } else if dynamic && leg_index == num_legs - 1 {
            let intermediate = if three_hop {
                second_intermediate_acc.as_ref()
            } else {
                intermediate_acc.as_ref()
            }
            .ok_or(RouterError::InvalidIntermediateAccount)?;
            let layout = dynamic_second_leg(&program_id_leg, leg_data)?;
            if three_hop && leg_data[layout.amount_offset..layout.amount_offset + 8] != [0u8; 8] {
                return Err(RouterError::InvalidDynamicLeg.into());
            }
            validate_dynamic_accounts(layout, leg_accounts, user, intermediate, user_output)?;
            patch_second_leg_amount(
                leg_data,
                quote_received,
                layout.amount_offset,
                &mut patched_data,
            )
        } else {
            if dynamic && leg_index == 0 {
                let intermediate = intermediate_acc
                    .as_ref()
                    .ok_or(RouterError::InvalidIntermediateAccount)?;
                if !leg_accounts
                    .iter()
                    .any(|acc| acc.address() == intermediate.address() && acc.is_writable())
                    || (three_hop
                        && second_intermediate_acc.as_ref().is_some_and(|second| {
                            leg_accounts
                                .iter()
                                .any(|acc| acc.address() == second.address() && acc.is_writable())
                        }))
                {
                    return Err(RouterError::InvalidIntermediateAccount.into());
                }
            }
            leg_data
        };

        let mut metas = [const { MaybeUninit::<InstructionAccount>::uninit() }; MAX_LEG_ACCOUNTS];
        for (i, acc) in leg_accounts.iter().enumerate() {
            metas[i].write(InstructionAccount::new(
                acc.address(),
                acc.is_writable(),
                acc.is_signer(),
            ));
        }
        let metas_slice = unsafe {
            core::slice::from_raw_parts(metas.as_ptr() as *const InstructionAccount, num_accounts)
        };

        let ix = InstructionView {
            program_id: &program_id_leg,
            accounts: metas_slice,
            data: invoke_data,
        };
        invoke_with_slice(&ix, leg_accounts)?;

        if dynamic && leg_index == 0 {
            let intermediate = intermediate_acc
                .as_ref()
                .ok_or(RouterError::InvalidIntermediateAccount)?;
            quote_received = received_quote(
                quote_before,
                token_amount(intermediate)?,
                intermediate_min_out,
            )?;
        } else if three_hop && leg_index == 1 {
            let intermediate = second_intermediate_acc
                .as_ref()
                .ok_or(RouterError::InvalidIntermediateAccount)?;
            quote_received = received_quote(
                second_intermediate_before,
                token_amount(intermediate)?,
                second_intermediate_min_out,
            )?;
        }
    }

    if dynamic && cursor != data.len() {
        return Err(RouterError::InvalidInstructionData.into());
    }

    // Leftover remaining accounts are DEX program metas (SDK appends them after legs).
    let _ = remaining_offset;

    if let Some(intermediate) = intermediate_acc.as_ref() {
        if token_amount(intermediate)? != quote_before {
            return Err(RouterError::IntermediateNotSpent.into());
        }
    }
    if let Some(intermediate) = second_intermediate_acc.as_ref() {
        if token_amount(intermediate)? != second_intermediate_before {
            return Err(RouterError::IntermediateNotSpent.into());
        }
    }

    // C2 fee integrity:
    // - exact-in:  spent == amount_in (no overspend or understated fees)
    // - exact-out: spent <= amount_in (amount_in is max budget; DEX enforces exact out)
    let fee_source_after = match fee_asset {
        0 => fee_source.lamports(),
        1 => token_amount(fee_source)?,
        _ => return Err(RouterError::InvalidFeeAsset.into()),
    };
    validate_spend(
        fee_source_before,
        fee_source_after,
        amount_in,
        fee,
        exact_out,
    )?;

    let output_after = output_balance(user_output, native_sol_out)?;
    let received = output_after.saturating_sub(output_before);
    if received < min_amount_out {
        return Err(RouterError::SlippageExceeded.into());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::str::FromStr;
    use pinocchio::account::{RuntimeAccount, NOT_BORROWED};

    // RuntimeAccount is followed immediately by its data, as in the Solana ABI.
    #[repr(C)]
    struct Fixture {
        raw: RuntimeAccount,
        data: [u8; 80],
    }

    impl Fixture {
        fn new(address: Address, owner: Address, signer: bool, data_len: u64) -> Self {
            Self {
                raw: RuntimeAccount {
                    borrow_state: NOT_BORROWED,
                    address,
                    owner,
                    is_signer: signer as u8,
                    is_writable: 1,
                    data_len,
                    lamports: 1_000_000,
                    ..Default::default()
                },
                data: [0; 80],
            }
        }

        fn view(&mut self) -> AccountView {
            // SAFETY: repr(C) ensures aligned metadata immediately followed by
            // the data buffer; the fixture stays alive during every view use.
            assert!(self.raw.data_len <= self.data.len() as u64);
            unsafe { AccountView::new_unchecked(&mut self.raw) }
        }
    }

    #[test]
    fn exact_in_rejects_overspending_understatement_and_unspent_budget() {
        let mismatch = Err(RouterError::FeeSourceMismatch.into());
        // A 10,000-unit swap cannot be declared as 1 to round its 100-bps fee to zero.
        let understated_fee = checked_fee(1, 100).unwrap();
        assert_eq!(understated_fee, 0);
        assert_eq!(
            validate_spend(20_000, 10_000, 1, understated_fee, false),
            mismatch
        );
        // Legitimate 100-bps fee and input spend total exactly 10,000.
        assert_eq!(validate_spend(20_000, 10_000, 10_000, 100, false), Ok(()));
        assert_eq!(validate_spend(20_000, 9_999, 10_000, 100, false), mismatch);
        assert_eq!(validate_spend(20_000, 10_001, 10_000, 100, false), mismatch);
        assert_eq!(validate_spend(20_000, 20_001, 10_000, 100, false), mismatch);
    }

    #[test]
    fn exact_out_preserves_the_fee_floor_and_budget_ceiling() {
        let mismatch = Err(RouterError::FeeSourceMismatch.into());
        assert_eq!(validate_spend(20_000, 19_900, 10_000, 100, true), Ok(()));
        assert_eq!(validate_spend(20_000, 10_000, 10_000, 100, true), Ok(()));
        assert_eq!(validate_spend(20_000, 19_901, 10_000, 100, true), mismatch);
        assert_eq!(validate_spend(20_000, 9_999, 10_000, 100, true), mismatch);
    }

    #[test]
    fn token_output_is_bound_to_the_expected_mint_in_every_route_mode() {
        let user = Address::new_from_array([1; 32]);
        let mint = [7; 32];
        let mut payer = Fixture::new(user, Address::new_from_array(SYSTEM_PROGRAM_ID), true, 0);
        for program in [TOKEN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID] {
            let mut output = Fixture::new(
                Address::new_from_array([2; 32]),
                Address::new_from_array(program),
                false,
                72,
            );
            output.data[..32].copy_from_slice(&mint);
            output.data[32..64].copy_from_slice(user.as_array());
            for dynamic in [false, true] {
                assert_eq!(
                    validate_output(&payer.view(), &output.view(), &mint, dynamic),
                    Ok(false)
                );
                assert_eq!(
                    validate_output(&payer.view(), &output.view(), &[8; 32], dynamic),
                    Err(RouterError::InvalidOutputMint.into())
                );
            }
            output.data[32..64].fill(9);
            assert_eq!(
                validate_output(&payer.view(), &output.view(), &mint, true),
                Err(RouterError::InvalidOutputAccount.into())
            );
        }
        // Keep the pre-dynamic error code stable for existing integrations.
        assert_eq!(RouterError::InvalidOutputMint as u32, 18);
    }

    #[test]
    fn native_sol_settlement_reads_signer_lamports_and_rejects_other_recipients() {
        let user = Address::new_from_array([1; 32]);
        let system = Address::new_from_array(SYSTEM_PROGRAM_ID);
        let mut payer = Fixture::new(user, system, true, 0);
        let mut other = Fixture::new(Address::new_from_array([2; 32]), system, false, 0);
        assert_eq!(
            validate_output(&payer.view(), &payer.view(), &SYSTEM_PROGRAM_ID, false),
            Ok(true)
        );
        assert_eq!(
            validate_output(&payer.view(), &other.view(), &SYSTEM_PROGRAM_ID, false),
            Err(RouterError::InvalidOutputAccount.into())
        );
        assert_eq!(
            validate_output(&payer.view(), &payer.view(), &SYSTEM_PROGRAM_ID, true),
            Err(RouterError::InvalidOutputAccount.into())
        );
        let before = output_balance(&payer.view(), true).unwrap();
        payer.raw.lamports += 500;
        assert_eq!(output_balance(&payer.view(), true).unwrap() - before, 500);
    }

    #[test]
    fn legacy_native_sol_header_reaches_leg_parsing() {
        let user = Address::new_from_array([1; 32]);
        let system = Address::new_from_array(SYSTEM_PROGRAM_ID);
        let (config, bump) = crate::state::config_pda(&crate::ID);
        let mut payer = Fixture::new(user, system, true, 0);
        let mut cfg = Fixture::new(config, crate::ID, false, RouterConfig::LEN as u64);
        RouterConfig::write_new(&mut cfg.view(), user, user, 0, bump).unwrap();
        let mut destination = Fixture::new(Address::new_from_array([2; 32]), system, false, 0);
        let mut source = Fixture::new(
            Address::new_from_array([3; 32]),
            Address::new_from_array(TOKEN_PROGRAM_ID),
            false,
            72,
        );
        source.data[..32].fill(7);
        source.data[32..64].copy_from_slice(user.as_array());
        let mut fee_program =
            Fixture::new(Address::new_from_array(TOKEN_PROGRAM_ID), system, false, 0);
        let mut accounts = [
            payer.view(),
            cfg.view(),
            destination.view(),
            source.view(),
            payer.view(),
            fee_program.view(),
        ];
        let mut data = [0u8; ROUTE_HEADER_LEN];
        data[..8].copy_from_slice(&1u64.to_le_bytes());
        data[8..16].copy_from_slice(&1u64.to_le_bytes());
        data[16] = 1;
        data[17] = 1;
        // Deliberately omit the CPI leg: InvalidLeg proves native output passed validation.
        assert_eq!(
            process(&crate::ID, &mut accounts, &data),
            Err(RouterError::InvalidLeg.into())
        );
        // A SOL fee source must remain the signer, even with fee_bps=0.
        data[16] = 0;
        assert_eq!(
            process(&crate::ID, &mut accounts, &data),
            Err(RouterError::InvalidFeeAsset.into())
        );
    }

    #[test]
    fn every_route_header_rejects_a_substituted_output_mint_before_cpi() {
        let user = Address::new_from_array([1; 32]);
        let system = Address::new_from_array(SYSTEM_PROGRAM_ID);
        let token = Address::new_from_array(TOKEN_PROGRAM_ID);
        let (config, bump) = crate::state::config_pda(&crate::ID);
        let mut payer = Fixture::new(user, system, true, 0);
        let mut cfg = Fixture::new(config, crate::ID, false, RouterConfig::LEN as u64);
        RouterConfig::write_new(&mut cfg.view(), user, user, 0, bump).unwrap();
        let mut destination = Fixture::new(Address::new_from_array([2; 32]), token, false, 72);
        let mut source = Fixture::new(Address::new_from_array([3; 32]), token, false, 72);
        let mut output = Fixture::new(Address::new_from_array([4; 32]), token, false, 72);
        let mut first = Fixture::new(Address::new_from_array([5; 32]), token, false, 72);
        let mut second = Fixture::new(Address::new_from_array([6; 32]), token, false, 72);
        for fixture in [&mut source, &mut output, &mut first, &mut second] {
            fixture.data[..32].fill(7);
            fixture.data[32..64].copy_from_slice(user.as_array());
        }
        let mut fee_program = Fixture::new(token, system, false, 0);
        for intermediates in 0..=2 {
            let mut accounts = [
                payer.view(),
                cfg.view(),
                destination.view(),
                source.view(),
                output.view(),
                fee_program.view(),
                first.view(),
                second.view(),
            ];
            let mut data = [0u8; ROUTE_HEADER_LEN + 16];
            data[..8].copy_from_slice(&1u64.to_le_bytes());
            data[8..16].copy_from_slice(&1u64.to_le_bytes());
            data[16] = 1;
            data[17] = (intermediates + 1) as u8;
            data[18..50].fill(8); // Declared mint differs from the actual mint [7;32].
            data[50..58].copy_from_slice(&1u64.to_le_bytes());
            data[58..66].copy_from_slice(&1u64.to_le_bytes());
            let len = ROUTE_HEADER_LEN + 8 * intermediates;
            assert_eq!(
                process_inner(&crate::ID, &mut accounts, &data[..len], intermediates),
                Err(RouterError::InvalidOutputMint.into())
            );
            data[18..50].fill(7);
            // No CPI leg is provided; reaching its parser proves header/account acceptance.
            assert_eq!(
                process_inner(&crate::ID, &mut accounts, &data[..len], intermediates),
                Err(RouterError::InvalidLeg.into())
            );
        }
    }

    #[test]
    fn stored_config_bump_recreates_the_canonical_pda() {
        let program_id = Address::from_str("CmNFUmRJL7YcnVn22oZzwG5Xg5WJqbcHEc6BK5mzDNR8").unwrap();
        let (canonical, bump) = crate::state::config_pda(&program_id);
        let derived =
            Address::create_program_address(&[CONFIG_SEED, &[bump]], &program_id).unwrap();
        assert_eq!(derived, canonical);
        assert_ne!(
            Address::create_program_address(&[CONFIG_SEED, &[bump.wrapping_sub(1)]], &program_id)
                .ok(),
            Some(canonical)
        );
    }

    #[test]
    fn dynamic_target_program_ids_match_mainnet_addresses() {
        assert_eq!(
            Address::from_str("LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj")
                .unwrap()
                .as_array(),
            &LAUNCHLAB_PROGRAM_ID
        );
        assert_eq!(
            Address::from_str("CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C")
                .unwrap()
                .as_array(),
            &RAYDIUM_CPMM_PROGRAM_ID
        );
        assert_eq!(
            Address::from_str("LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo")
                .unwrap()
                .as_array(),
            &METEORA_DLMM_PROGRAM_ID
        );
        for (name, expected) in [
            (
                "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8",
                &RAYDIUM_AMM_V4_PROGRAM_ID,
            ),
            (
                "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK",
                &RAYDIUM_CLMM_PROGRAM_ID,
            ),
            (
                "cpamdpZCGKUy5JxQXB4dcpGPiikHawvSWAd6mEn1sGG",
                &METEORA_DAMM_V2_PROGRAM_ID,
            ),
            (
                "whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc",
                &ORCA_WHIRLPOOL_PROGRAM_ID,
            ),
            (
                "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA",
                &PUMPSWAP_PROGRAM_ID,
            ),
        ] {
            assert_eq!(Address::from_str(name).unwrap().as_array(), expected);
        }
    }

    #[test]
    fn dynamic_whirlpool_preserves_and_validates_supplemental_slice() {
        let program = Address::new_from_array(ORCA_WHIRLPOOL_PROGRAM_ID);
        let mut data = [0u8; 49];
        data[..8].copy_from_slice(&CLMM_SWAP_V2);
        data[40] = 1;
        data[41] = 1;
        data[42..48].copy_from_slice(&[1, 1, 0, 0, 0, 6]);
        for count in 1..=3 {
            data[48] = count;
            let layout = dynamic_second_leg(&program, &data).unwrap();
            let mut buffer = [0u8; MAX_DYNAMIC_LEG_DATA];
            let patched = patch_second_leg_amount(&data, 123, layout.amount_offset, &mut buffer);
            assert_eq!(read_u64(patched, 8).unwrap(), 123);
            assert_eq!(&patched[16..], &data[16..]);
        }
        for (index, value) in [(42, 0), (43, 2), (47, 0), (48, 0), (48, 4)] {
            let mut invalid = data;
            invalid[index] = value;
            assert!(dynamic_second_leg(&program, &invalid).is_err());
        }
        assert!(dynamic_second_leg(&program, &data[..48]).is_err());
    }

    #[test]
    fn dynamic_target_only_accepts_exact_input_swaps() {
        let launch = Address::new_from_array(LAUNCHLAB_PROGRAM_ID);
        let cpmm = Address::new_from_array(RAYDIUM_CPMM_PROGRAM_ID);
        let mut launch_data = [0u8; 32];
        launch_data[..8].copy_from_slice(&LAUNCHLAB_BUY_EXACT_IN);
        assert_eq!(
            dynamic_second_leg(&launch, &launch_data)
                .unwrap()
                .input_account,
            6
        );
        launch_data[24] = 1; // share fee is not part of the supported path
        assert!(dynamic_second_leg(&launch, &launch_data).is_err());

        let mut cpmm_data = [0u8; 24];
        cpmm_data[..8].copy_from_slice(&CPMM_SWAP_BASE_IN);
        assert_eq!(
            dynamic_second_leg(&cpmm, &cpmm_data).unwrap().input_account,
            4
        );
        assert!(dynamic_second_leg(&cpmm, &cpmm_data[..23]).is_err());
        assert!(dynamic_second_leg(&Address::new_from_array([1; 32]), &cpmm_data).is_err());
    }

    #[test]
    fn dynamic_quote_delta_and_amount_patch_use_only_newly_received_tokens() {
        let amount = received_quote(9_000, 9_375, 300).unwrap();
        assert_eq!(amount, 375);
        assert!(received_quote(9_000, 9_299, 300).is_err());
        assert!(received_quote(9_000, 8_999, 1).is_err());
        let mut original = [0u8; 32];
        original[..8].copy_from_slice(&LAUNCHLAB_BUY_EXACT_IN);
        original[8..16].copy_from_slice(&1u64.to_le_bytes());
        original[16..24].copy_from_slice(&50u64.to_le_bytes());
        let mut patched = [0u8; MAX_DYNAMIC_LEG_DATA];
        let data = patch_second_leg_amount(&original, amount, 8, &mut patched);
        assert_eq!(u64::from_le_bytes(data[8..16].try_into().unwrap()), 375);
        assert_eq!(u64::from_le_bytes(data[16..24].try_into().unwrap()), 50);
        assert_eq!(u64::from_le_bytes(original[8..16].try_into().unwrap()), 1);
    }

    #[test]
    fn dynamic_middle_accepts_supported_exact_input_data_and_patches_amount() {
        let dlmm = Address::new_from_array(METEORA_DLMM_PROGRAM_ID);
        let mut data = [0u8; 28];
        data[..8].copy_from_slice(&METEORA_DLMM_SWAP2);
        data[16..24].copy_from_slice(&10u64.to_le_bytes());
        assert_eq!(dynamic_second_leg(&dlmm, &data).unwrap().amount_offset, 8);
        let received = received_quote(2_000, 2_375, 300).unwrap();
        let mut patched = [0u8; MAX_DYNAMIC_LEG_DATA];
        let invoke_data = patch_second_leg_amount(&data, received, 8, &mut patched);
        assert_eq!(read_u64(invoke_data, 8).unwrap(), 375);
        assert_eq!(read_u64(invoke_data, 16).unwrap(), 10);
        assert_eq!(read_u64(&data, 8).unwrap(), 0);
        data[24] = 1;
        assert!(dynamic_second_leg(&dlmm, &data).is_err());
        assert!(dynamic_second_leg(&Address::new_from_array([7; 32]), &data).is_err());

        let amm = Address::new_from_array(RAYDIUM_AMM_V4_PROGRAM_ID);
        let mut amm_data = [0u8; 17];
        amm_data[0] = 16;
        let layout = dynamic_second_leg(&amm, &amm_data).unwrap();
        assert_eq!(layout.amount_offset, 1);
        let invoke_data = patch_second_leg_amount(&amm_data, received, 1, &mut patched);
        assert_eq!(read_u64(invoke_data, 1).unwrap(), 375);
        amm_data[0] = 17;
        assert!(dynamic_second_leg(&amm, &amm_data).is_err());

        let orca = Address::new_from_array(ORCA_WHIRLPOOL_PROGRAM_ID);
        let mut orca_data = [0u8; 43];
        orca_data[..8].copy_from_slice(&CLMM_SWAP_V2);
        orca_data[40] = 1;
        orca_data[41] = 1;
        assert_eq!(
            dynamic_second_leg(&orca, &orca_data).unwrap().input_account,
            7
        );
        orca_data[41] = 0;
        assert_eq!(
            dynamic_second_leg(&orca, &orca_data).unwrap().input_account,
            9
        );
        orca_data[40] = 0;
        assert!(dynamic_second_leg(&orca, &orca_data).is_err());

        let clmm = Address::new_from_array(RAYDIUM_CLMM_PROGRAM_ID);
        let mut clmm_data = [0u8; 41];
        clmm_data[..8].copy_from_slice(&CLMM_SWAP_V2);
        clmm_data[40] = 1;
        let clmm_layout = dynamic_second_leg(&clmm, &clmm_data).unwrap();
        assert_eq!(
            (clmm_layout.input_account, clmm_layout.output_account),
            (3, 4)
        );
        clmm_data[40] = 0;
        assert!(dynamic_second_leg(&clmm, &clmm_data).is_err());

        let damm = Address::new_from_array(METEORA_DAMM_V2_PROGRAM_ID);
        let mut damm_data = [0u8; 25];
        damm_data[..8].copy_from_slice(&METEORA_DLMM_SWAP2);
        let damm_layout = dynamic_second_leg(&damm, &damm_data).unwrap();
        assert_eq!(
            (damm_layout.input_account, damm_layout.output_account),
            (2, 3)
        );
        damm_data[24] = 2; // exact-out cannot spend the previous hop's actual output
        assert!(dynamic_second_leg(&damm, &damm_data).is_err());
    }

    #[test]
    fn pumpswap_sell_is_a_dynamic_following_leg_only_with_exact_input_layout() {
        let pump = Address::new_from_array(PUMPSWAP_PROGRAM_ID);
        let mut data = [0u8; 24];
        data[..8].copy_from_slice(&PUMPSWAP_SELL);
        data[16..24].copy_from_slice(&17u64.to_le_bytes());
        let layout = dynamic_second_leg(&pump, &data).unwrap();
        assert_eq!(
            (layout.input_account, layout.output_account, layout.signer),
            (5, 6, 1)
        );
        assert_eq!((layout.input_mint, layout.output_mint), (3, 4));
        assert_eq!((layout.input_program, layout.output_program), (11, 12));
        let mut patched = [0u8; MAX_DYNAMIC_LEG_DATA];
        let invoke_data = patch_second_leg_amount(&data, 375, layout.amount_offset, &mut patched);
        assert_eq!(read_u64(invoke_data, 8).unwrap(), 375);
        assert_eq!(read_u64(invoke_data, 16).unwrap(), 17);
        assert_eq!(read_u64(&data, 8).unwrap(), 0);
        assert!(dynamic_second_leg(&pump, &data[..23]).is_err());
        data[0] ^= 1;
        assert!(dynamic_second_leg(&pump, &data).is_err());
        assert!(dynamic_second_leg(&Address::new_from_array([1; 32]), &data).is_err());
    }
}
