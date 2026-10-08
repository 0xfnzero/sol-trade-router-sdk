//! Idempotently top up only the canonical Pump creator vault's rent floor.
//! Accounts: signer/writable payer, readonly Pump curve, writable creator vault,
//! readonly System Program. No data. Run before ROUTE so setup rent is not input.
use crate::{state::SYSTEM_PROGRAM_ID, RouterError};
use pinocchio::{
    error::ProgramError,
    sysvars::{rent::Rent, Sysvar},
    AccountView, Address, ProgramResult,
};
use pinocchio_system::instructions::Transfer;

const PUMP: Address = Address::from_str_const("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P");

pub fn process(_program_id: &Address, accounts: &mut [AccountView], data: &[u8]) -> ProgramResult {
    if !data.is_empty() {
        return Err(RouterError::InvalidInstructionData.into());
    }
    let [payer, curve, vault, system] = accounts else {
        return Err(RouterError::InsufficientAccounts.into());
    };
    if !payer.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if !payer.is_writable() || !vault.is_writable() {
        return Err(ProgramError::InvalidAccountData);
    }
    if system.address().as_ref() != SYSTEM_PROGRAM_ID.as_slice() || curve.owner() != &PUMP {
        return Err(RouterError::InvalidProgramId.into());
    }
    let curve_data = curve.try_borrow()?;
    if curve_data.len() < 81 || curve_data[..8] != [23, 183, 248, 55, 96, 216, 172, 96] {
        return Err(ProgramError::InvalidAccountData);
    }
    let expected = Address::find_program_address(&[b"creator-vault", &curve_data[49..81]], &PUMP).0;
    if vault.address() != &expected
        || vault.owner().as_ref() != SYSTEM_PROGRAM_ID.as_slice()
        || vault.data_len() != 0
    {
        return Err(ProgramError::InvalidAccountData);
    }
    drop(curve_data);
    let needed = Rent::get()?
        .try_minimum_balance(0)?
        .saturating_sub(vault.lamports());
    if needed > 0 {
        Transfer {
            from: payer,
            to: vault,
            lamports: needed,
        }
        .invoke()?;
    }
    Ok(())
}
