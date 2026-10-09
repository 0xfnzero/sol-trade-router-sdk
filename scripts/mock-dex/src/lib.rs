//! Local-only CPMM-layout fixture. It uses real token CPIs, not real DEX pricing.
#![no_std]
use pinocchio::{
    cpi::invoke_with_slice,
    default_allocator,
    error::ProgramError,
    instruction::{InstructionAccount, InstructionView},
    nostd_panic_handler, program_entrypoint, AccountView, Address, ProgramResult,
};

program_entrypoint!(process_instruction);
default_allocator!();
nostd_panic_handler!();

fn checked_transfer(
    from: &AccountView,
    mint: &AccountView,
    to: &AccountView,
    authority: &AccountView,
    program: &AccountView,
    amount: u64,
) -> ProgramResult {
    let decimals = *mint
        .try_borrow()?
        .get(44)
        .ok_or(ProgramError::InvalidAccountData)?;
    let mut data = [0u8; 10];
    data[0] = 12;
    data[1..9].copy_from_slice(&amount.to_le_bytes());
    data[9] = decimals;
    let metas = [
        InstructionAccount::new(from.address(), true, false),
        InstructionAccount::new(mint.address(), false, false),
        InstructionAccount::new(to.address(), true, false),
        InstructionAccount::new(authority.address(), false, true),
    ];
    invoke_with_slice(
        &InstructionView {
            program_id: program.address(),
            accounts: &metas,
            data: &data,
        },
        &[from.clone(), mint.clone(), to.clone(), authority.clone()],
    )
}

fn process_instruction(_id: &Address, accounts: &mut [AccountView], data: &[u8]) -> ProgramResult {
    if accounts.len() != 13
        || data.len() != 24
        || data[..8] != [143, 190, 90, 218, 196, 30, 51, 222]
    {
        return Err(ProgramError::InvalidInstructionData);
    }
    let amount = u64::from_le_bytes(
        data[8..16]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    // Deliberately misbehaving hops exercise the Router's post-CPI rollback.
    let mode = u64::from_le_bytes(
        data[16..24]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    let spent = match mode {
        1 => amount,
        2 => amount
            .checked_sub(1)
            .ok_or(ProgramError::ArithmeticOverflow)?,
        3 => amount
            .checked_add(1)
            .ok_or(ProgramError::ArithmeticOverflow)?,
        _ => return Err(ProgramError::InvalidInstructionData),
    };
    checked_transfer(
        &accounts[4],
        &accounts[10],
        &accounts[6],
        &accounts[0],
        &accounts[8],
        spent,
    )?;
    let out = amount
        .checked_mul(2)
        .ok_or(ProgramError::ArithmeticOverflow)?;
    checked_transfer(
        &accounts[7],
        &accounts[11],
        &accounts[5],
        &accounts[0],
        &accounts[9],
        out,
    )
}
