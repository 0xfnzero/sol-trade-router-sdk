use solana_sdk::instruction::AccountMeta;
use solana_sdk::pubkey;
use solana_sdk::pubkey::Pubkey;

/// Deployed router program id (`keys/router-keypair.json`).
pub const PROGRAM_ID: Pubkey = pubkey!("CMrrMgrEvXW3oo6RtxnneDf5D5TeujfbqveFiKuvqrYg");

pub const WSOL_MINT: Pubkey = pubkey!("So11111111111111111111111111111111111111112");
pub const USDC_MINT: Pubkey = pubkey!("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
pub const USDT_MINT: Pubkey = pubkey!("Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB");

pub const TOKEN_PROGRAM: Pubkey = pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");

pub const TOKEN_2022_PROGRAM: Pubkey = pubkey!("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");

pub const SYSTEM_PROGRAM: Pubkey = pubkey!("11111111111111111111111111111111");
pub const ASSOCIATED_TOKEN_PROGRAM: Pubkey =
    pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");

pub const LAUNCHLAB_PROGRAM: Pubkey = pubkey!("LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj");

pub const LAUNCHLAB_AUTHORITY: Pubkey = pubkey!("WLHv2UAZm6z4KyaaELi5pjdbJh6RESMva1Rnn8pJVVh");

pub const LAUNCHLAB_EVENT_AUTHORITY: Pubkey =
    pubkey!("2DPAtwB8L12vrMRExbLuyGnC7n2J5LNoZQSejeQGpwkr");

/// StonkFun standard platform config (sol-trade-sdk).
pub const STONKFUN_STANDARD_PLATFORM_CONFIG: Pubkey =
    pubkey!("4E876qZTE9FJMrBzgVtBrSrzz2TLivB5Y5QXPjB4gZL7");

/// StonkFun reward platform config (sol-trade-sdk).
pub const STONKFUN_REWARD_PLATFORM_CONFIG: Pubkey =
    pubkey!("6BwHHDg3u1854jC8PDLXvR4spTcLNaoBxLJNGC4nTESt");

pub const RAYDIUM_CPMM_PROGRAM: Pubkey = pubkey!("CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C");

pub const RAYDIUM_CPMM_AUTHORITY: Pubkey = pubkey!("GpMZbSM2GgvTKHJirzeGfMFoaZ8UR2X7F4v8vHTvxFbL");

pub const PUMPFUN_PROGRAM: Pubkey = pubkey!("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P");
pub const PUMPFUN_GLOBAL: Pubkey = pubkey!("4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf");
pub const PUMPFUN_EVENT_AUTHORITY: Pubkey =
    pubkey!("Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1");
pub const PUMPFUN_FEE_PROGRAM: Pubkey = pubkey!("pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ");
pub const PUMPFUN_GLOBAL_VOLUME_ACCUMULATOR: Pubkey =
    pubkey!("Hq2wp8uJ9jCPsYgNHex8RtqdvMPfVGoYwjvF1ATiwn2Y");
pub const PUMPFUN_FEE_CONFIG: Pubkey = pubkey!("8Wf5TiAheLUqBrKXeYg2JtAFFMWtKdG2BSFgqUcPVwTt");

pub const PUMPSWAP_PROGRAM: Pubkey = pubkey!("pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA");
pub const PUMPSWAP_GLOBAL: Pubkey = pubkey!("ADyA8hdefvWN2dbGGWFotbzWxrAvLW83WG6QCVXvJKqw");
pub const PUMPSWAP_EVENT_AUTHORITY: Pubkey =
    pubkey!("GS4CU59F31iL7aR2Q8zVS8DRrcRnXX1yjQ66TqNVQnaR");
pub const PUMPSWAP_PROTOCOL_FEE_RECIPIENT: Pubkey =
    pubkey!("62qc2CNXwrYqQScmEdiZFFAnJR262PxWEuNQtxfafNgV");
pub const PUMPSWAP_BUYBACK_FEE_RECIPIENT: Pubkey =
    pubkey!("5YxQFdt3Tr9zJLvkFccqXVUwhdTWJQc1fFg2YPbxvxeD");
/// Pump mayhem fee recipient pool (shared bonding-curve + PumpSwap; pump-public-docs).
pub const PUMP_MAYHEM_FEE_RECIPIENT: Pubkey =
    pubkey!("GesfTA3X2arioaHp8bbKdjG9vJtskViWACZoYvxp4twS");
pub const PUMPSWAP_GLOBAL_VOLUME_ACCUMULATOR: Pubkey =
    pubkey!("C2aFPdENg4A2HQsmrd5rTw5TaYBX5Ku887cWjbFKtZpw");
pub const PUMPSWAP_FEE_CONFIG: Pubkey = pubkey!("5PHirr8joyTMp9JMm6nW7hNDVyEYdkzDqazxPD7RaTjx");
pub const PUMPSWAP_FEE_PROGRAM: Pubkey = pubkey!("pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ");

pub const RAYDIUM_AMM_V4_PROGRAM: Pubkey = pubkey!("675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8");
pub const RAYDIUM_AMM_V4_AUTHORITY: Pubkey =
    pubkey!("5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1");

pub const METEORA_DAMM_V2_PROGRAM: Pubkey = pubkey!("cpamdpZCGKUy5JxQXB4dcpGPiikHawvSWAd6mEn1sGG");
pub const METEORA_DAMM_V2_AUTHORITY: Pubkey =
    pubkey!("HLnpSz9h2S4hiLQ43rnSD9XkcUThA7B8hQMKmDaiTLcC");
pub const RAYDIUM_CLMM_PROGRAM: Pubkey = pubkey!("CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK");

/// PDA: `["pool_tick_array_bitmap_extension", pool_state]` (Raydium CLMM IDL).
#[inline]
pub fn raydium_clmm_tick_array_bitmap_extension(pool_state: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[b"pool_tick_array_bitmap_extension", pool_state.as_ref()],
        &RAYDIUM_CLMM_PROGRAM,
    )
    .0
}

/// PumpFun `user_volume_accumulator` PDA — seeds `["user_volume_accumulator", user]`.
/// Must be derived from the **signer**, never from a foreign event account.
#[inline]
pub fn pumpfun_user_volume_accumulator(user: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[b"user_volume_accumulator", user.as_ref()], &PUMPFUN_PROGRAM).0
}

/// PumpFun `bonding-curve-v2` PDA — seeds `["bonding-curve-v2", mint]`.
#[inline]
pub fn pumpfun_bonding_curve_v2(mint: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[b"bonding-curve-v2", mint.as_ref()], &PUMPFUN_PROGRAM).0
}
pub const ORCA_WHIRLPOOL_PROGRAM: Pubkey = pubkey!("whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc");
pub const METEORA_DLMM_PROGRAM: Pubkey = pubkey!("LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo");
pub const MEMO_PROGRAM: Pubkey = pubkey!("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr");
pub const METEORA_DLMM_EVENT_AUTHORITY: Pubkey =
    pubkey!("D1ZN9Wj1fRSUQfCjhvnu1hqDMT7hzjzBBpi12nVniYD6");
pub const SYSVAR_INSTRUCTIONS: Pubkey = pubkey!("Sysvar1nstructions1111111111111111111111111");

pub const BUY_EXACT_IN_LAUNCHLAB: [u8; 8] = [250, 234, 13, 123, 213, 156, 19, 236];
pub const SELL_EXACT_IN_LAUNCHLAB: [u8; 8] = [149, 39, 222, 155, 211, 124, 152, 26];

pub const CPMM_SWAP_BASE_IN: [u8; 8] = [143, 190, 90, 218, 196, 30, 51, 222];
/// Raydium CPMM default `trade_fee_rate` (denominator 1e6). Used when swap
/// events do not carry AmmConfig — matches sol-trade-sdk / raydium-sdk-V2.
pub const CPMM_DEFAULT_TRADE_FEE_RATE: u64 = 2_500;

pub const PUMPFUN_BUY_EXACT_SOL_IN: [u8; 8] = [56, 252, 116, 8, 158, 223, 205, 95];
pub const PUMPFUN_SELL: [u8; 8] = [51, 230, 133, 164, 1, 127, 131, 173];
pub const PUMPFUN_BUY_EXACT_QUOTE_IN_V2: [u8; 8] = [194, 171, 28, 70, 104, 77, 91, 47];
pub const PUMPFUN_SELL_V2: [u8; 8] = [93, 246, 130, 60, 231, 233, 64, 178];
pub const PUMPFUN_BUYBACK_FEE_RECIPIENT: Pubkey =
    pubkey!("5YxQFdt3Tr9zJLvkFccqXVUwhdTWJQc1fFg2YPbxvxeD");
pub const PUMPSWAP_BUY_EXACT_QUOTE_IN: [u8; 8] = [198, 46, 21, 82, 180, 217, 232, 112];
/// PumpSwap `buy` (exact-out / max quote).
pub const PUMPSWAP_BUY: [u8; 8] = [102, 6, 61, 18, 1, 218, 235, 234];
pub const PUMPSWAP_SELL: [u8; 8] = [51, 230, 133, 164, 1, 127, 131, 173];
/// Raydium CPMM `swap_base_output` (exact-out); hot path uses `swap_base_input`.
pub const CPMM_SWAP_BASE_OUT: [u8; 8] = [55, 217, 98, 86, 163, 74, 180, 173];
/// Legacy `swapBaseIn` (tag 9) — kept for tx decode; hot path uses V2.
#[allow(dead_code)]
pub const RAYDIUM_AMM_V4_SWAP_BASE_IN: u8 = 9;
/// Raydium AMM V4 `swapBaseInV2` — no OpenBook accounts (enum index 16).
pub const RAYDIUM_AMM_V4_SWAP_BASE_IN_V2: u8 = 16;
/// Raydium AMM V4 `swapBaseOutV2` (enum index 17).
pub const RAYDIUM_AMM_V4_SWAP_BASE_OUT_V2: u8 = 17;
pub const METEORA_DAMM_V2_SWAP2: [u8; 8] = [65, 75, 63, 76, 235, 91, 91, 136];
pub const METEORA_DAMM_V2_EXACT_IN: u8 = 0;
pub const METEORA_DAMM_V2_PARTIAL_FILL: u8 = 1;
pub const METEORA_DAMM_V2_EXACT_OUT: u8 = 2;
pub const CLMM_SWAP_V2: [u8; 8] = [43, 4, 237, 11, 26, 201, 30, 98];
/// Orca Whirlpool `swap_v2` shares the same Anchor disc as Raydium CLMM `swap_v2`.
pub const WHIRLPOOL_SWAP_V2: [u8; 8] = CLMM_SWAP_V2;
pub const METEORA_DLMM_SWAP2: [u8; 8] = [65, 75, 63, 76, 235, 91, 91, 136];

/// Raydium CLMM tick-math bounds (Q64.64).
/// Official: raydium-clmm `tick_math::{MIN,MAX}_SQRT_PRICE_X64`.
pub const MIN_SQRT_PRICE_X64: u128 = 4_295_048_016;
pub const MAX_SQRT_PRICE_X64: u128 = 79_226_673_521_066_979_257_578_248_091;

/// Orca Whirlpool tick-math bounds (Q64.64).
/// Official: whirlpools `tick_math::{MIN,MAX}_SQRT_PRICE_X64` — MAX ≠ Raydium CLMM.
pub const WHIRLPOOL_MIN_SQRT_PRICE_X64: u128 = 4_295_048_016;
pub const WHIRLPOOL_MAX_SQRT_PRICE_X64: u128 = 79_226_673_515_401_279_992_447_579_055;

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
pub const ASSOCIATED_TOKEN_PROGRAM_META: AccountMeta = AccountMeta {
    pubkey: ASSOCIATED_TOKEN_PROGRAM,
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
pub const PUMPSWAP_GLOBAL_META: AccountMeta = AccountMeta {
    pubkey: PUMPSWAP_GLOBAL,
    is_signer: false,
    is_writable: false,
};
pub const PUMPSWAP_EVENT_AUTHORITY_META: AccountMeta = AccountMeta {
    pubkey: PUMPSWAP_EVENT_AUTHORITY,
    is_signer: false,
    is_writable: false,
};
pub const PUMPSWAP_PROGRAM_META: AccountMeta = AccountMeta {
    pubkey: PUMPSWAP_PROGRAM,
    is_signer: false,
    is_writable: false,
};
pub const PUMPSWAP_GLOBAL_VOLUME_ACCUMULATOR_META: AccountMeta = AccountMeta {
    pubkey: PUMPSWAP_GLOBAL_VOLUME_ACCUMULATOR,
    is_signer: false,
    is_writable: false, // IDL buy / buy_exact_quote_in index 19
};
pub const PUMPSWAP_FEE_CONFIG_META: AccountMeta = AccountMeta {
    pubkey: PUMPSWAP_FEE_CONFIG,
    is_signer: false,
    is_writable: false,
};
pub const PUMPSWAP_FEE_PROGRAM_META: AccountMeta = AccountMeta {
    pubkey: PUMPSWAP_FEE_PROGRAM,
    is_signer: false,
    is_writable: false,
};
pub const RAYDIUM_AMM_V4_AUTHORITY_META: AccountMeta = AccountMeta {
    pubkey: RAYDIUM_AMM_V4_AUTHORITY,
    is_signer: false,
    is_writable: false,
};
pub const METEORA_DAMM_V2_AUTHORITY_META: AccountMeta = AccountMeta {
    pubkey: METEORA_DAMM_V2_AUTHORITY,
    is_signer: false,
    is_writable: false,
};
pub const METEORA_DAMM_V2_PROGRAM_META: AccountMeta = AccountMeta {
    pubkey: METEORA_DAMM_V2_PROGRAM,
    is_signer: false,
    is_writable: false,
};
pub const SYSVAR_INSTRUCTIONS_META: AccountMeta = AccountMeta {
    pubkey: SYSVAR_INSTRUCTIONS,
    is_signer: false,
    is_writable: false,
};
pub const TOKEN_PROGRAM_META: AccountMeta = AccountMeta {
    pubkey: TOKEN_PROGRAM,
    is_signer: false,
    is_writable: false,
};
pub const TOKEN_2022_PROGRAM_META: AccountMeta = AccountMeta {
    pubkey: TOKEN_2022_PROGRAM,
    is_signer: false,
    is_writable: false,
};
pub const MEMO_PROGRAM_META: AccountMeta = AccountMeta {
    pubkey: MEMO_PROGRAM,
    is_signer: false,
    is_writable: false,
};
pub const METEORA_DLMM_PROGRAM_META: AccountMeta = AccountMeta {
    pubkey: METEORA_DLMM_PROGRAM,
    is_signer: false,
    is_writable: false,
};
pub const METEORA_DLMM_EVENT_AUTHORITY_META: AccountMeta = AccountMeta {
    pubkey: METEORA_DLMM_EVENT_AUTHORITY,
    is_signer: false,
    is_writable: false,
};
