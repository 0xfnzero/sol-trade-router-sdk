use solana_sdk::instruction::AccountMeta;
use solana_sdk::pubkey;
use solana_sdk::pubkey::Pubkey;

/// Deployed router program id (`keys/router-keypair.json`).
pub const PROGRAM_ID: Pubkey = pubkey!("CMrrMgrEvXW3oo6RtxnneDf5D5TeujfbqveFiKuvqrYg");

pub const WSOL_MINT: Pubkey = pubkey!("So11111111111111111111111111111111111111112");

pub const TOKEN_PROGRAM: Pubkey = pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");

pub const TOKEN_2022_PROGRAM: Pubkey =
    pubkey!("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");

pub const SYSTEM_PROGRAM: Pubkey = pubkey!("11111111111111111111111111111111");

pub const LAUNCHLAB_PROGRAM: Pubkey =
    pubkey!("LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj");

pub const LAUNCHLAB_AUTHORITY: Pubkey =
    pubkey!("WLHv2UAZm6z4KyaaELi5pjdbJh6RESMva1Rnn8pJVVh");

pub const LAUNCHLAB_EVENT_AUTHORITY: Pubkey =
    pubkey!("2DPAtwB8L12vrMRExbLuyGnC7n2J5LNoZQSejeQGpwkr");

/// StonkFun standard platform config (sol-trade-sdk).
pub const STONKFUN_STANDARD_PLATFORM_CONFIG: Pubkey =
    pubkey!("4E876qZTE9FJMrBzgVtBrSrzz2TLivB5Y5QXPjB4gZL7");

/// StonkFun reward platform config (sol-trade-sdk).
pub const STONKFUN_REWARD_PLATFORM_CONFIG: Pubkey =
    pubkey!("6BwHHDg3u1854jC8PDLXvR4spTcLNaoBxLJNGC4nTESt");

pub const RAYDIUM_CPMM_PROGRAM: Pubkey =
    pubkey!("CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C");

pub const RAYDIUM_CPMM_AUTHORITY: Pubkey =
    pubkey!("GpMZbSM2GgvTKHJirzeGfMFoaZ8UR2X7F4v8vHTvxFbL");

pub const PUMPFUN_PROGRAM: Pubkey =
    pubkey!("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P");

pub const BUY_EXACT_IN_LAUNCHLAB: [u8; 8] = [250, 234, 13, 123, 213, 156, 19, 236];
pub const SELL_EXACT_IN_LAUNCHLAB: [u8; 8] = [149, 39, 222, 155, 211, 124, 152, 26];

pub const CPMM_SWAP_BASE_IN: [u8; 8] = [143, 190, 90, 218, 196, 30, 51, 222];

pub const PUMPFUN_BUY_EXACT_SOL_IN: [u8; 8] = [56, 252, 116, 8, 158, 223, 205, 95];
pub const PUMPFUN_SELL: [u8; 8] = [51, 230, 133, 164, 1, 127, 131, 173];

// Const AccountMetas — avoid reconstructing static keys on every leg (latency).
pub const LAUNCHLAB_AUTHORITY_META: AccountMeta = AccountMeta {
    pubkey: LAUNCHLAB_AUTHORITY,
    is_signer: false,
    is_writable: false,
};
pub const LAUNCHLAB_EVENT_AUTHORITY_META: AccountMeta = AccountMeta {
    pubkey: LAUNCHLAB_EVENT_AUTHORITY,
    is_signer: false,
    is_writable: false,
};
pub const LAUNCHLAB_PROGRAM_META: AccountMeta = AccountMeta {
    pubkey: LAUNCHLAB_PROGRAM,
    is_signer: false,
    is_writable: false,
};
pub const SYSTEM_PROGRAM_META: AccountMeta = AccountMeta {
    pubkey: SYSTEM_PROGRAM,
    is_signer: false,
    is_writable: false,
};
pub const RAYDIUM_CPMM_AUTHORITY_META: AccountMeta = AccountMeta {
    pubkey: RAYDIUM_CPMM_AUTHORITY,
    is_signer: false,
    is_writable: false,
};
pub const PUMPFUN_PROGRAM_META: AccountMeta = AccountMeta {
    pubkey: PUMPFUN_PROGRAM,
    is_signer: false,
    is_writable: false,
};
