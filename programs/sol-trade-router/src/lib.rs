#![cfg_attr(feature = "bpf-entrypoint", no_std)]

pub mod error;
pub mod instructions;
pub mod state;

pub use error::RouterError;
pub use state::{RouterConfig, CONFIG_SEED};

pinocchio::address::declare_id!("CmNFUmRJL7YcnVn22oZzwG5Xg5WJqbcHEc6BK5mzDNR8");
// Program ID must match the pubkey of keys/router-keypair.json (local only).

/// Instruction tags.
pub mod tag {
    pub const INITIALIZE: u8 = 0;
    pub const UPDATE_CONFIG: u8 = 1;
    pub const ROUTE: u8 = 2;
    /// Dynamic two-hop exact-input route; legacy ROUTE remains unchanged.
    pub const ROUTE_DYNAMIC: u8 = 3;
    /// Dynamic three-hop route; each later leg spends only the previous leg's new output.
    pub const ROUTE_DYNAMIC_THREE: u8 = 4;
}

#[cfg(feature = "bpf-entrypoint")]
mod entrypoint_impl {
    use pinocchio::{
        default_allocator,
        nostd_panic_handler,
        program_entrypoint,
        AccountView,
        Address,
        ProgramResult,
    };

    use crate::{instructions, tag, RouterError};

    program_entrypoint!(process_instruction);
    default_allocator!();
    nostd_panic_handler!();

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
            tag::ROUTE_DYNAMIC => instructions::route::process_dynamic(program_id, accounts, data),
            tag::ROUTE_DYNAMIC_THREE => {
                instructions::route::process_dynamic_three(program_id, accounts, data)
            }
            _ => Err(RouterError::UnknownInstruction.into()),
        }
    }
}
