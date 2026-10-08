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
    /// Dynamic route's intermediate token account is invalid or disconnected.
    InvalidIntermediateAccount = 18,
    /// Dynamic route only permits LaunchLab buyExactIn or CPMM swapBaseInput as leg two.
    InvalidDynamicLeg = 19,
    /// The second leg did not spend exactly the tokens received from leg one.
    IntermediateNotSpent = 20,
}

impl From<RouterError> for ProgramError {
    #[inline(always)]
    fn from(e: RouterError) -> Self {
        ProgramError::Custom(e as u32)
    }
}
