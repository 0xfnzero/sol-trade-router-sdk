//! ATA create / close helpers.
//!
//! # Policy
//! - **WSOL / stock(quote)**: reusable — create on a **cold/prepare** tx (`prepare_*_atas`).
//! - **meme**: do **not** pre-create. Create inside the **same buy tx** so a failed trade
//!   rolls back and leaves no empty ATA. Default buy path sets `create_meme = true`.
//! - Closes are always opt-in (default off).

use std::cell::RefCell;
use std::collections::HashMap;

use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
};
use solana_system_interface::instruction as system_instruction;

use crate::constants::{ASSOCIATED_TOKEN_PROGRAM, SYSTEM_PROGRAM, TOKEN_PROGRAM, WSOL_MINT};

type AtaKey = (Pubkey, Pubkey, Pubkey); // owner, mint, token_program

thread_local! {
    /// Per-thread ATA cache — no Mutex on the hot path.
    static ATA_CACHE: RefCell<HashMap<AtaKey, Pubkey>> =
        RefCell::new(HashMap::with_capacity(64));
}

/// Cached ATA derivation (hot path — avoids repeated `find_program_address`).
#[inline]
pub fn ata(owner: &Pubkey, mint: &Pubkey, token_program: &Pubkey) -> Pubkey {
    let key = (*owner, *mint, *token_program);
    ATA_CACHE.with(|cache| {
        let mut map = cache.borrow_mut();
        if let Some(cached) = map.get(&key) {
            return *cached;
        }
        let addr = Pubkey::find_program_address(
            &[owner.as_ref(), token_program.as_ref(), mint.as_ref()],
            &ASSOCIATED_TOKEN_PROGRAM,
        )
        .0;
        if map.len() < 4096 {
            map.insert(key, addr);
        }
        addr
    })
}

/// Warm ATA cache for a payer (call once on bot start / per thread).
pub fn warm_ata_cache(owner: &Pubkey, mints: &[(Pubkey, Pubkey)]) {
    for (mint, tp) in mints {
        let _ = ata(owner, mint, tp);
    }
    let _ = ata(owner, &WSOL_MINT, &TOKEN_PROGRAM);
}

/// Idempotent create ATA (Associated Token Program ix = 1).
#[inline]
pub fn create_ata(
    payer: &Pubkey,
    owner: &Pubkey,
    mint: &Pubkey,
    token_program: &Pubkey,
) -> Instruction {
    let associated = ata(owner, mint, token_program);
    Instruction {
        program_id: ASSOCIATED_TOKEN_PROGRAM,
        accounts: vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new(associated, false),
            AccountMeta::new_readonly(*owner, false),
            AccountMeta::new_readonly(*mint, false),
            AccountMeta::new_readonly(SYSTEM_PROGRAM, false),
            AccountMeta::new_readonly(*token_program, false),
        ],
        data: vec![1],
    }
}

#[inline]
pub fn create_ata_idempotent(
    payer: &Pubkey,
    owner: &Pubkey,
    mint: &Pubkey,
    token_program: &Pubkey,
) -> Instruction {
    create_ata(payer, owner, mint, token_program)
}

#[inline]
pub fn create_wsol_ata(payer: &Pubkey) -> Instruction {
    create_ata(payer, payer, &WSOL_MINT, &TOKEN_PROGRAM)
}

/// SPL Token `CloseAccount` (ix = 9).
pub fn close_ata(
    owner: &Pubkey,
    mint: &Pubkey,
    token_program: &Pubkey,
    destination: &Pubkey,
) -> Instruction {
    let token_ata = ata(owner, mint, token_program);
    Instruction {
        program_id: *token_program,
        accounts: vec![
            AccountMeta::new(token_ata, false),
            AccountMeta::new(*destination, false),
            AccountMeta::new_readonly(*owner, true),
        ],
        data: vec![9],
    }
}

#[inline]
pub fn close_wsol_ata(owner: &Pubkey) -> Instruction {
    close_ata(owner, &WSOL_MINT, &TOKEN_PROGRAM, owner)
}

/// SPL Token `SyncNative` (ix = 17).
#[inline]
fn sync_native(token_program: &Pubkey, account: &Pubkey) -> Instruction {
    Instruction {
        program_id: *token_program,
        accounts: vec![AccountMeta::new(*account, false)],
        data: vec![17],
    }
}

/// Wrap native SOL into an **existing** WSOL ATA (no create).
pub fn wrap_sol(payer: &Pubkey, lamports: u64) -> Vec<Instruction> {
    wrap_sol_with_options(payer, lamports, false)
}

pub fn wrap_sol_with_options(
    payer: &Pubkey,
    lamports: u64,
    create_ata_account: bool,
) -> Vec<Instruction> {
    let wsol_ata = ata(payer, &WSOL_MINT, &TOKEN_PROGRAM);
    let mut ixs = Vec::with_capacity(3);
    if create_ata_account {
        ixs.push(create_wsol_ata(payer));
    }
    ixs.push(system_instruction::transfer(payer, &wsol_ata, lamports));
    ixs.push(sync_native(&TOKEN_PROGRAM, &wsol_ata));
    ixs
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AtaKind {
    /// Meme / base token — prefer create **in the buy tx** (not ahead).
    Meme,
    /// WSOL — prefer cold-path prepare.
    Wsol,
    /// Stock / quote — prefer cold-path prepare.
    Quote,
}

#[derive(Clone, Debug, Default)]
pub struct TouchedAtas {
    pub meme_mint: Option<Pubkey>,
    pub meme_token_program: Option<Pubkey>,
    pub quote_mint: Option<Pubkey>,
    pub quote_token_program: Option<Pubkey>,
    pub wsol: bool,
}

impl TouchedAtas {
    pub fn touch_meme(&mut self, mint: Pubkey, token_program: Pubkey) {
        self.meme_mint = Some(mint);
        self.meme_token_program = Some(token_program);
    }

    pub fn touch_quote(&mut self, mint: Pubkey, token_program: Pubkey) {
        if mint == WSOL_MINT {
            self.wsol = true;
        } else {
            self.quote_mint = Some(mint);
            self.quote_token_program = Some(token_program);
        }
    }

    pub fn touch_wsol(&mut self) {
        self.wsol = true;
    }
}

/// In-trade ATA policy.
///
/// Defaults: **no** WSOL/quote create, **no** closes.
/// Buy builders set [`Self::create_meme`] = `true` so meme ATA is created atomically
/// with the swap (failed buy → no leftover empty meme ATA).
#[derive(Clone, Debug)]
pub struct AtaPolicy {
    /// Create meme ATA in this trade tx. Buy defaults enable this.
    pub create_meme: bool,
    /// Create WSOL ATA in this trade tx (default `false` — use `prepare_*` / `create_wsol_ata`).
    pub create_wsol: bool,
    /// Create stock/quote ATA in this trade tx (default `false` — prepare ahead).
    pub create_quote: bool,
    /// Close WSOL after trade (unwrap to native SOL). Default `false`.
    pub close_wsol: bool,
    /// Close meme ATA after trade (must be empty). Default `false`.
    pub close_meme: bool,
    /// Close stock/quote ATA after trade (must be empty). Default `false`.
    pub close_quote: bool,
}

impl Default for AtaPolicy {
    fn default() -> Self {
        Self {
            create_meme: false,
            create_wsol: false,
            create_quote: false,
            close_wsol: false,
            close_meme: false,
            close_quote: false,
        }
    }
}

impl AtaPolicy {
    #[inline]
    pub fn none() -> Self {
        Self::default()
    }

    /// Buy path: create meme in-tx only (WSOL/stock assumed prepared).
    pub fn for_buy() -> Self {
        Self {
            create_meme: true,
            ..Self::default()
        }
    }

    /// Sell path: assume all ATAs exist; no create/close.
    pub fn for_sell() -> Self {
        Self::default()
    }

    /// Create meme + WSOL/quote in the same trade tx (no pre-prepare).
    pub fn create_all_in_trade() -> Self {
        Self {
            create_meme: true,
            create_wsol: true,
            create_quote: true,
            ..Self::default()
        }
    }

    pub fn allows(&self, kind: AtaKind) -> bool {
        match kind {
            AtaKind::Meme => self.create_meme,
            AtaKind::Wsol => self.create_wsol,
            AtaKind::Quote => self.create_quote,
        }
    }

    pub fn with_create_meme(mut self, v: bool) -> Self {
        self.create_meme = v;
        self
    }
    pub fn with_create_wsol(mut self, v: bool) -> Self {
        self.create_wsol = v;
        self
    }
    pub fn with_create_quote(mut self, v: bool) -> Self {
        self.create_quote = v;
        self
    }
    pub fn with_close_wsol(mut self, v: bool) -> Self {
        self.close_wsol = v;
        self
    }
    pub fn with_close_meme(mut self, v: bool) -> Self {
        self.close_meme = v;
        self
    }
    pub fn with_close_quote(mut self, v: bool) -> Self {
        self.close_quote = v;
        self
    }

    pub fn build_cleanup(&self, owner: &Pubkey, touched: &TouchedAtas) -> Vec<Instruction> {
        let mut out = Vec::with_capacity(3);
        if self.close_wsol && touched.wsol {
            out.push(close_wsol_ata(owner));
        }
        if self.close_quote {
            if let (Some(mint), Some(tp)) = (touched.quote_mint, touched.quote_token_program) {
                out.push(close_ata(owner, &mint, &tp, owner));
            }
        }
        if self.close_meme {
            if let (Some(mint), Some(tp)) = (touched.meme_mint, touched.meme_token_program) {
                out.push(close_ata(owner, &mint, &tp, owner));
            }
        }
        out
    }
}
