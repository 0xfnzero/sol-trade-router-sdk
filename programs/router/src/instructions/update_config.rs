//! Update router config (authority only).
//!
//! Accounts:
//! 0. `[signer]` authority
//! 1. `[writable]` config PDA
//! 2. `[]` new fee_recipient (optional; pass config again to keep)
//!
//! Data:
//! - fee_bps: u16
//! - paused: u8 (0/1)
//! - update_recipient: u8 (1 = use account[2] as new recipient)

use pinocchio::{error::ProgramError, AccountView, Address, ProgramResult};

use crate::{
    state::{RouterConfig, MAX_FEE_BPS},
    RouterError,
};

pub fn process(
    program_id: &Address,
    accounts: &mut [AccountView],
    data: &[u8],
) -> ProgramResult {
    if data.len() < 4 {
        return Err(RouterError::InvalidInstructionData.into());
    }
    let fee_bps = u16::from_le_bytes([data[0], data[1]]);
    let paused = data[2];
    let update_recipient = data[3];
    if fee_bps > MAX_FEE_BPS {
        return Err(RouterError::InvalidFeeBps.into());
    }
    if paused > 1 {
        return Err(RouterError::InvalidInstructionData.into());
    }

    let (authority, rest) = accounts
        .split_first_mut()
        .ok_or(RouterError::InsufficientAccounts)?;
    let (config, rest) = rest
        .split_first_mut()
        .ok_or(RouterError::InsufficientAccounts)?;

    if !authority.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let (expected, _) = crate::state::config_pda(program_id);
    if config.address() != &expected || config.owner() != program_id {
        return Err(RouterError::InvalidConfig.into());
    }

    let cfg = RouterConfig::read(config)?;
    if authority.address() != &cfg.authority {
        return Err(RouterError::Unauthorized.into());
    }

    let new_recipient = if update_recipient == 1 {
        let recipient = rest.first().ok_or(RouterError::InsufficientAccounts)?;
        Some(*recipient.address())
    } else {
        None
    };

    RouterConfig::update(
        config,
        new_recipient,
        Some(fee_bps),
        Some(paused == 1),
    )?;

    Ok(())
}
