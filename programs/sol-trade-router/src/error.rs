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
    /// fee_source spend does not match `amount_in` (exact-in: ==; exact-out: <= budget).
    FeeSourceMismatch = 16,
    /// user_output is not a token account, or native SOL output is not the signer.
    InvalidOutputAccount = 17,
    /// user_output mint does not match the mint declared in instruction data.
    InvalidOutputMint = 18,
    /// Dynamic route's intermediate token account is invalid or disconnected.
    InvalidIntermediateAccount = 21,
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
