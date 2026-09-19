#![cfg_attr(feature = "bpf-entrypoint", no_std)]

pub mod error;
pub mod instructions;
pub mod state;

pub use error::RouterError;
pub use state::{RouterConfig, CONFIG_SEED};

pinocchio::address::declare_id!("CMrrMgrEvXW3oo6RtxnneDf5D5TeujfbqveFiKuvqrYg");
// Program ID must match the pubkey of keys/router-keypair.json (local only).

/// Instruction tags.
pub mod tag {
    pub const INITIALIZE: u8 = 0;
    pub const UPDATE_CONFIG: u8 = 1;
    pub const ROUTE: u8 = 2;
}

#[cfg(feature = "bpf-entrypoint")]
mod entrypoint_impl {
    use pinocchio::{entrypoint, AccountView, Address, ProgramResult};

    use crate::{instructions, tag, RouterError};

    entrypoint!(process_instruction);

    pub fn process_instruction(
        program_id: &Address,
        accounts: &mut [AccountView],
        instruction_data: &[u8],
    ) -> ProgramResult {
        let (disc, data) = instruction_data
            .split_first()
            .ok_or(RouterError::InvalidInstructionData)?;

        match *disc {
            tag::INITIALIZE => instructions::initialize::process(program_id, accounts, data),
            tag::UPDATE_CONFIG => {
                instructions::update_config::process(program_id, accounts, data)
            }
            tag::ROUTE => instructions::route::process(program_id, accounts, data),
            _ => Err(RouterError::UnknownInstruction.into()),
        }
    }
}
