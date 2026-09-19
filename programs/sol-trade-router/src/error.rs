use pinocchio::error::ProgramError;

/// Router program errors.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RouterError {
    InvalidInstructionData = 1,
    UnknownInstruction = 2,
    Unauthorized = 3,
    AlreadyInitialized = 4,
    NotInitialized = 5,
    InvalidConfig = 6,
    Paused = 7,
    InvalidFeeBps = 8,
    InvalidFeeAsset = 9,
    SlippageExceeded = 10,
    TooManyLegs = 11,
    InvalidLeg = 12,
    InsufficientAccounts = 13,
    ArithmeticOverflow = 14,
    InvalidProgramId = 15,
    /// fee_source did not spend at least `amount_in` (fee + swap).
    FeeSourceMismatch = 16,
    /// user_output is not a Token / Token-2022 account.
    InvalidOutputAccount = 17,
}

impl From<RouterError> for ProgramError {
    #[inline(always)]
    fn from(e: RouterError) -> Self {
        ProgramError::Custom(e as u32)
    }
}
