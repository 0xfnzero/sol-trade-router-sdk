//! Initialize the router config PDA.
//!
//! Accounts:
//! 0. `[signer, writable]` authority / payer
//! 1. `[writable]` config PDA (seeds = ["config"])
//! 2. `[]` fee_recipient
//! 3. `[]` system_program
//!
//! Data: `fee_bps: u16` (le) + `bump: u8`

use pinocchio::{
    cpi::{Seed, Signer},
    error::ProgramError,
    AccountView, Address, ProgramResult,
};
use pinocchio_system::create_account_with_minimum_balance_signed;

use crate::{
    state::{config_pda, RouterConfig, CONFIG_SEED, MAX_FEE_BPS},
    RouterError,
};

pub fn process(
    program_id: &Address,
    accounts: &mut [AccountView],
    data: &[u8],
) -> ProgramResult {
    if data.len() < 3 {
        return Err(RouterError::InvalidInstructionData.into());
    }
    let fee_bps = u16::from_le_bytes([data[0], data[1]]);
    let bump = data[2];
    if fee_bps > MAX_FEE_BPS {
        return Err(RouterError::InvalidFeeBps.into());
    }

    let (payer, rest) = accounts
        .split_first_mut()
        .ok_or(RouterError::InsufficientAccounts)?;
    let (config, rest) = rest
        .split_first_mut()
        .ok_or(RouterError::InsufficientAccounts)?;
    let (fee_recipient, _rest) = rest
        .split_first_mut()
        .ok_or(RouterError::InsufficientAccounts)?;

    if !payer.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let (expected, expected_bump) = config_pda(program_id);
    if bump != expected_bump || config.address() != &expected {
        return Err(RouterError::InvalidConfig.into());
    }

    if config.data_len() >= RouterConfig::LEN && RouterConfig::read(config).is_ok() {
        return Err(RouterError::AlreadyInitialized.into());
    }

    let bump_seed = [bump];
    let seed_array: [Seed; 2] = [Seed::from(CONFIG_SEED), Seed::from(bump_seed.as_ref())];
    let signer = Signer::from(&seed_array);

    create_account_with_minimum_balance_signed(
        config,
        RouterConfig::LEN,
        program_id,
        payer,
        None,
        &[signer],
    )?;

    RouterConfig::write_new(
        config,
        *payer.address(),
        *fee_recipient.address(),
        fee_bps,
        bump,
    )
}
