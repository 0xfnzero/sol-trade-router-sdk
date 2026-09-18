//! Buy payment asset / sell settlement asset (hot path, no RPC).

use solana_sdk::pubkey::Pubkey;

use crate::constants::WSOL_MINT;

/// What the user pays with on **buy** (`buy_with_*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuyWith {
    /// Native SOL. Wrap to WSOL when the DEX needs it; PumpFun spends native SOL.
    Sol,
    /// Existing WSOL ATA — no wrap.
    Wsol,
    /// Already-held quote/stock token (e.g. CARDS) — single hop, no SOL→stock leg.
    Token(Pubkey),
}

impl Default for BuyWith {
    fn default() -> Self {
        Self::Sol
    }
}

impl BuyWith {
    #[inline]
    pub fn is_sol_family(self) -> bool {
        matches!(self, Self::Sol | Self::Wsol)
    }

    #[inline]
    pub fn mint(self) -> Option<Pubkey> {
        match self {
            Self::Sol | Self::Wsol => Some(WSOL_MINT),
            Self::Token(m) => Some(m),
        }
    }
}

/// What the user receives on **sell** (`sell_to_*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SellTo {
    /// Unwrap WSOL → native SOL.
    Sol,
    /// Keep WSOL in ATA — no unwrap/close.
    Wsol,
    /// Stop at quote/stock (meme→stock only) — no stock→SOL leg.
    Token(Pubkey),
}

impl Default for SellTo {
    fn default() -> Self {
        Self::Sol
    }
}

impl SellTo {
    #[inline]
    pub fn is_sol_family(self) -> bool {
        matches!(self, Self::Sol | Self::Wsol)
    }

    #[inline]
    pub fn mint(self) -> Option<Pubkey> {
        match self {
            Self::Sol | Self::Wsol => Some(WSOL_MINT),
            Self::Token(m) => Some(m),
        }
    }

    /// Native SOL settlement needs a WSOL close (unwrap) unless the caller keeps WSOL.
    #[inline]
    pub fn needs_wsol_unwrap(self) -> bool {
        matches!(self, Self::Sol)
    }
}
