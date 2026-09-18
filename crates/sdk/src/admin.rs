//! Admin helpers: initialize / update router config (cold path).

use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    system_program,
};

use crate::config_pda;

pub const TAG_INITIALIZE: u8 = 0;
pub const TAG_UPDATE_CONFIG: u8 = 1;

pub fn initialize_config(
    program_id: &Pubkey,
    authority: &Pubkey,
    fee_recipient: &Pubkey,
    fee_bps: u16,
) -> Instruction {
    let (config, bump) = config_pda(program_id);
    let mut data = vec![TAG_INITIALIZE];
    data.extend_from_slice(&fee_bps.to_le_bytes());
    data.push(bump);
    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new(*authority, true),
            AccountMeta::new(config, false),
            AccountMeta::new_readonly(*fee_recipient, false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
        data,
    }
}

pub fn update_config(
    program_id: &Pubkey,
    authority: &Pubkey,
    fee_bps: u16,
    paused: bool,
    new_fee_recipient: Option<&Pubkey>,
) -> Instruction {
    let (config, _) = config_pda(program_id);
    let mut data = vec![TAG_UPDATE_CONFIG];
    data.extend_from_slice(&fee_bps.to_le_bytes());
    data.push(u8::from(paused));
    data.push(u8::from(new_fee_recipient.is_some()));

    let mut accounts = vec![
        AccountMeta::new_readonly(*authority, true),
        AccountMeta::new(config, false),
    ];
    if let Some(r) = new_fee_recipient {
        accounts.push(AccountMeta::new_readonly(*r, false));
    }

    Instruction {
        program_id: *program_id,
        accounts,
        data,
    }
}
