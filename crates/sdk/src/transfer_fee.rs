//! Token-2022 transfer fee (aligned with sol-trade-sdk).

/// Active Token-2022 transfer fee for the epoch in which pool state was loaded.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TokenTransferFee {
    pub basis_points: u16,
    pub maximum_fee: u64,
}

impl TokenTransferFee {
    #[inline(always)]
    pub const fn none() -> Self {
        Self {
            basis_points: 0,
            maximum_fee: 0,
        }
    }

    #[inline(always)]
    pub fn calculate(&self, amount: u64) -> u64 {
        if self.basis_points == 0 || amount == 0 || self.maximum_fee == 0 {
            return 0;
        }
        let numerator = (amount as u128).saturating_mul(self.basis_points as u128);
        let fee = numerator.div_ceil(10_000);
        fee.min(self.maximum_fee as u128) as u64
    }

    /// Fee that must be added so the recipient receives `post_fee_amount`.
    #[inline(always)]
    pub fn calculate_inverse(&self, post_fee_amount: u64) -> u64 {
        if self.basis_points == 0 || post_fee_amount == 0 || self.maximum_fee == 0 {
            return 0;
        }
        if self.basis_points >= 10_000 {
            return self.maximum_fee;
        }
        let numerator = (post_fee_amount as u128).saturating_mul(self.basis_points as u128);
        let fee = numerator.div_ceil(10_000 - self.basis_points as u128);
        fee.min(self.maximum_fee as u128) as u64
    }
}
