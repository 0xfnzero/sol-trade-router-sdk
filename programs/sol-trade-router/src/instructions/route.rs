//! Multi-hop route: take platform fee, then CPI each DEX leg.
//!
//! # Accounts
//! 0. `[signer]` user
//! 1. `[]` config PDA (must be this program's PDA, owned by this program)
//! 2. `[writable]` fee_destination
//! 3. `[writable]` fee_source (SOL: user; Token: user ATA owned by user)
//! 4. `[writable]` user_output_token (min_out check; token ATA, or user for native SOL)
//! 5. `[]` fee_program (System for SOL fee; SPL Token / Token-2022 for token fee)
//! 6.. : remaining — concatenated per-leg AccountMetas, then optional DEX program ids
//!
//! # Data (after 1-byte tag stripped by entrypoint)
//! ```text
//! amount_in: u64       // total input from fee_source (fee + swap spend)
//! min_amount_out: u64
//! fee_asset: u8        // 0=SOL 1=SPL; bit7=exact-out
//! num_legs: u8
//! output_mint: [u8;32] // expected output mint; SystemProgram (=0) => native SOL on user
//! for each leg:
//!   program_id: [u8;32]
//!   num_accounts: u8
//!   data_len: u16
//!   data: [u8; data_len]
//! ```
//!
//! # Spend integrity (`fee_source`)
//! Fee = amount_in * fee_bps / 10_000 is taken from `fee_source` first (skipped if 0).
//! - exact-in:  `spent == amount_in` (floor and ceiling — no overspend / understate)
//! - exact-out: `fee <= spent <= amount_in` (`amount_in` is max budget)

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
        config_pda, is_token_program, token_amount, token_mint, token_owner, RouterConfig,
        SYSTEM_PROGRAM_ID, TOKEN_2022_PROGRAM_ID, TOKEN_PROGRAM_ID,
    },
    RouterError,
};

const MAX_LEGS: usize = 4;
/// Must stay in sync with SDK `legs::MAX_LEG_ACCOUNTS`.
const MAX_LEG_ACCOUNTS: usize = 64;
/// Header after tag strip: amount_in(8) + min_out(8) + fee_asset(1) + num_legs(1) + mint(32).
const ROUTE_HEADER_LEN: usize = 50;

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
fn is_native_sol_output(mint: &[u8; 32]) -> bool {
    *mint == SYSTEM_PROGRAM_ID
}

pub fn process(
    program_id: &Address,
    accounts: &mut [AccountView],
    data: &[u8],
) -> ProgramResult {
    if data.len() < ROUTE_HEADER_LEN {
        return Err(RouterError::InvalidInstructionData.into());
    }

    let amount_in = read_u64(data, 0)?;
    let min_amount_out = read_u64(data, 8)?;
    let fee_asset = data[16];
    let num_legs = data[17] as usize;
    let mut output_mint = [0u8; 32];
    output_mint.copy_from_slice(&data[18..50]);

    if amount_in == 0 {
        return Err(RouterError::InvalidInstructionData.into());
    }
    if !(1..=MAX_LEGS).contains(&num_legs) {
        return Err(RouterError::TooManyLegs.into());
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

    if !user.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // C1: config must be this program's PDA and owned by this program.
    let (expected_config, _) = config_pda(program_id);
    if config_acc.address() != &expected_config {
        return Err(RouterError::InvalidConfig.into());
    }
    if config_acc.owner() != program_id {
        return Err(RouterError::InvalidConfig.into());
    }

    let cfg = RouterConfig::read(config_acc)?;
    if cfg.paused != 0 {
        return Err(RouterError::Paused.into());
    }

    let native_sol_out = is_native_sol_output(&output_mint);
    if native_sol_out {
        // Native SOL min_out: measure signer lamports; output account must be the user.
        if user_output.address() != user.address() {
            return Err(RouterError::InvalidOutputAccount.into());
        }
    } else if !is_token_program(user_output.owner()) {
        return Err(RouterError::InvalidOutputAccount.into());
    } else if token_mint(user_output)?.as_array() != &output_mint {
        return Err(RouterError::InvalidOutputMint.into());
    }

    let fee = checked_fee(amount_in, cfg.fee_bps)?;

    // Snapshot fee_source before fee + legs (C2).
    let fee_source_before = match fee_asset {
        0 => {
            // SOL fee_source must always be the signer (even when fee_bps == 0).
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

    let output_before = if native_sol_out {
        user.lamports()
    } else {
        token_amount(user_output)?
    };

    let mut cursor = ROUTE_HEADER_LEN;
    let mut remaining_offset = 0usize;

    for _ in 0..num_legs {
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
            data: leg_data,
        };
        invoke_with_slice(&ix, leg_accounts)?;
    }

    // Leftover remaining accounts are DEX program metas (SDK appends them after legs).
    let _ = remaining_offset;

    // C2 spend integrity on fee_source.
    let fee_source_after = match fee_asset {
        0 => fee_source.lamports(),
        1 => token_amount(fee_source)?,
        _ => return Err(RouterError::InvalidFeeAsset.into()),
    };
    let spent = fee_source_before
        .checked_sub(fee_source_after)
        .ok_or(RouterError::FeeSourceMismatch)?;
    if exact_out {
        if spent > amount_in || spent < fee {
            return Err(RouterError::FeeSourceMismatch.into());
        }
    } else if spent != amount_in {
        // Exact-in: amount_in is both floor and ceiling (blocks overspend + fee understate).
        return Err(RouterError::FeeSourceMismatch.into());
    }

    let output_after = if native_sol_out {
        user.lamports()
    } else {
        token_amount(user_output)?
    };
    let received = output_after.saturating_sub(output_before);
    if received < min_amount_out {
        return Err(RouterError::SlippageExceeded.into());
    }

    Ok(())
}
