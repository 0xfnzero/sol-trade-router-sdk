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
    /// Idempotent canonical Pump creator-vault rent preparation.
    pub const PREPARE_PUMPFUN: u8 = 7;
    // Tags 3/4 used an unbound output mint and are deliberately no longer dispatched.
    /// Dynamic two-hop exact-input route with an explicit expected output mint.
    pub const ROUTE_DYNAMIC: u8 = 5;
    /// Dynamic three-hop route; each later leg spends only the previous leg's new output.
    pub const ROUTE_DYNAMIC_THREE: u8 = 6;
}

/// Dispatch a router instruction. Tags 3/4 are retired because they did not bind the output mint.
pub fn process_instruction(
    program_id: &pinocchio::Address,
    accounts: &mut [pinocchio::AccountView],
    instruction_data: &[u8],
) -> pinocchio::ProgramResult {
    let (disc, data) = instruction_data
        .split_first()
        .ok_or(RouterError::InvalidInstructionData)?;
    match *disc {
        tag::INITIALIZE => instructions::initialize::process(program_id, accounts, data),
        tag::UPDATE_CONFIG => instructions::update_config::process(program_id, accounts, data),
        tag::PREPARE_PUMPFUN => instructions::prepare_pumpfun::process(program_id, accounts, data),
        tag::ROUTE => instructions::route::process(program_id, accounts, data),
        tag::ROUTE_DYNAMIC => instructions::route::process_dynamic(program_id, accounts, data),
        tag::ROUTE_DYNAMIC_THREE => {
            instructions::route::process_dynamic_three(program_id, accounts, data)
        }
        _ => Err(RouterError::UnknownInstruction.into()),
    }
}

#[cfg(feature = "bpf-entrypoint")]
mod entrypoint_impl {
    use crate::process_instruction;
    use pinocchio::{default_allocator, nostd_panic_handler, program_entrypoint};
    program_entrypoint!(process_instruction);
    default_allocator!();
    nostd_panic_handler!();
}

#[cfg(test)]
mod dispatch_tests {
    use super::*;

    #[test]
    fn retired_unbound_dynamic_tags_are_rejected() {
        for tag in [3, 4] {
            assert_eq!(
                process_instruction(&ID, &mut [], &[tag]),
                Err(RouterError::UnknownInstruction.into())
            );
        }
        assert_eq!(
            process_instruction(&ID, &mut [], &[tag::PREPARE_PUMPFUN]),
            Err(RouterError::InsufficientAccounts.into())
        );
        assert_eq!(
            process_instruction(&ID, &mut [], &[tag::PREPARE_PUMPFUN, 0]),
            Err(RouterError::InvalidInstructionData.into())
        );
        for tag in [2, 5, 6] {
            assert_eq!(
                process_instruction(&ID, &mut [], &[tag]),
                Err(RouterError::InvalidInstructionData.into())
            );
        }
    }
}
