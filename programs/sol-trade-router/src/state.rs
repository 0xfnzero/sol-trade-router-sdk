use pinocchio::{error::ProgramError, AccountView, Address};

use crate::RouterError;

/// PDA seed for the global router config.
pub const CONFIG_SEED: &[u8] = b"config";

/// Max platform fee: 10% (1000 bps).
pub const MAX_FEE_BPS: u16 = 1_000;

/// Config account magic / version tag.
pub const CONFIG_DISCRIMINATOR: [u8; 8] = *b"ROUTCFG1";

/// System Program (`11111…`).
pub const SYSTEM_PROGRAM_ID: [u8; 32] = [0u8; 32];

/// SPL Token Program.
pub const TOKEN_PROGRAM_ID: [u8; 32] = [
    6, 221, 246, 225, 215, 101, 161, 147, 217, 203, 225, 70, 206, 235, 121, 172, 28, 180, 133, 237,
    95, 91, 55, 145, 58, 140, 245, 133, 126, 255, 0, 169,
];

/// Token-2022 Program.
pub const TOKEN_2022_PROGRAM_ID: [u8; 32] = [
    6, 221, 246, 225, 238, 117, 143, 222, 24, 66, 93, 188, 228, 108, 205, 218, 182, 26, 252, 77,
    131, 185, 13, 39, 254, 189, 249, 40, 216, 161, 139, 252,
];

/// On-chain router configuration.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct RouterConfig {
    pub discriminator: [u8; 8],
    pub authority: Address,
    pub fee_recipient: Address,
    pub fee_bps: u16,
    pub bump: u8,
    pub paused: u8,
    pub _padding: [u8; 4],
}

impl RouterConfig {
    pub const LEN: usize = core::mem::size_of::<Self>();

    #[inline(always)]
    pub fn is_initialized(&self) -> bool {
        self.discriminator == CONFIG_DISCRIMINATOR
    }

    #[inline(always)]
    pub fn read(account: &AccountView) -> Result<Self, ProgramError> {
        let data = account.try_borrow()?;
        if data.len() < Self::LEN {
            return Err(RouterError::InvalidConfig.into());
        }
        let cfg = unsafe { *(data.as_ptr() as *const RouterConfig) };
        if !cfg.is_initialized() {
            return Err(RouterError::NotInitialized.into());
        }
        Ok(cfg)
    }

    #[inline(always)]
    pub fn write_new(
        account: &mut AccountView,
        authority: Address,
        fee_recipient: Address,
        fee_bps: u16,
        bump: u8,
    ) -> Result<(), ProgramError> {
        if fee_bps > MAX_FEE_BPS {
            return Err(RouterError::InvalidFeeBps.into());
        }
        let mut data = account.try_borrow_mut()?;
        if data.len() < Self::LEN {
            return Err(RouterError::InvalidConfig.into());
        }
        let existing = unsafe { &*(data.as_ptr() as *const RouterConfig) };
        if existing.is_initialized() {
            return Err(RouterError::AlreadyInitialized.into());
        }
        let cfg = RouterConfig {
            discriminator: CONFIG_DISCRIMINATOR,
            authority,
            fee_recipient,
            fee_bps,
            bump,
            paused: 0,
            _padding: [0; 4],
        };
        // SAFETY: buffer length checked above; RouterConfig is POD.
        unsafe {
            core::ptr::write(data.as_mut_ptr() as *mut RouterConfig, cfg);
        }
        Ok(())
    }

    #[inline(always)]
    pub fn update(
        account: &mut AccountView,
        fee_recipient: Option<Address>,
        fee_bps: Option<u16>,
        paused: Option<bool>,
    ) -> Result<(), ProgramError> {
        if let Some(bps) = fee_bps {
            if bps > MAX_FEE_BPS {
                return Err(RouterError::InvalidFeeBps.into());
            }
        }
        let mut data = account.try_borrow_mut()?;
        if data.len() < Self::LEN {
            return Err(RouterError::InvalidConfig.into());
        }
        let cfg = unsafe { &mut *(data.as_mut_ptr() as *mut RouterConfig) };
        if !cfg.is_initialized() {
            return Err(RouterError::NotInitialized.into());
        }
        if let Some(recipient) = fee_recipient {
            cfg.fee_recipient = recipient;
        }
        if let Some(bps) = fee_bps {
            cfg.fee_bps = bps;
        }
        if let Some(p) = paused {
            cfg.paused = u8::from(p);
        }
        Ok(())
    }
}

/// Derive config PDA.
#[inline(always)]
pub fn config_pda(program_id: &Address) -> (Address, u8) {
    Address::find_program_address(&[CONFIG_SEED], program_id)
}

#[inline(always)]
pub fn is_token_program(owner: &Address) -> bool {
    let b = owner.as_array();
    *b == TOKEN_PROGRAM_ID || *b == TOKEN_2022_PROGRAM_ID
}

/// Read SPL / Token-2022 mint (offset 0).
#[inline(always)]
pub fn token_mint(account: &AccountView) -> Result<Address, ProgramError> {
    let data = account.try_borrow()?;
    if data.len() < 32 {
        return Err(ProgramError::InvalidAccountData);
    }
    let mut buf = [0u8; 32];
    buf.copy_from_slice(&data[0..32]);
    Ok(Address::new_from_array(buf))
}

/// Read SPL / Token-2022 owner (offset 32).
#[inline(always)]
pub fn token_owner(account: &AccountView) -> Result<Address, ProgramError> {
    let data = account.try_borrow()?;
    if data.len() < 64 {
        return Err(ProgramError::InvalidAccountData);
    }
    let mut buf = [0u8; 32];
    buf.copy_from_slice(&data[32..64]);
    Ok(Address::new_from_array(buf))
}

/// Read SPL / Token-2022 token amount (offset 64).
#[inline(always)]
pub fn token_amount(account: &AccountView) -> Result<u64, ProgramError> {
    let data = account.try_borrow()?;
    if data.len() < 72 {
        return Err(ProgramError::InvalidAccountData);
    }
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&data[64..72]);
    Ok(u64::from_le_bytes(buf))
}
