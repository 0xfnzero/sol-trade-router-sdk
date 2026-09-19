//! Shared helpers for gated mainnet `simulateTransaction` tests.
//!
//! Pattern (aligned with sol-trade-sdk):
//! 1. `Keypair::new()` ephemeral wallet
//! 2. Virtually fund from a high-balance mainnet account (`sigVerify=false`)
//! 3. Build real DEX legs from live `sol-parser-sdk` events / fixtures
//! 4. `simulateTransaction` — never submit on-chain

#![cfg(test)]

use solana_client::rpc_client::RpcClient;
use solana_client::rpc_config::{
    RpcSimulateTransactionConfig, RpcTransactionConfig,
};
use solana_client::rpc_client::GetConfirmedSignaturesForAddress2Config;
use solana_commitment_config::CommitmentConfig;
use solana_sdk::{
    instruction::Instruction,
    pubkey,
    pubkey::Pubkey,
    signature::{Keypair, Signature},
    signer::Signer,
    transaction::Transaction,
};
use solana_system_interface::instruction as system_instruction;
use solana_transaction_status::UiTransactionEncoding;
use std::str::FromStr;
use std::thread;
use std::time::Duration;

use crate::constants::{
    PUMPSWAP_BUYBACK_FEE_RECIPIENT, PUMPSWAP_PROGRAM, PUMPSWAP_PROTOCOL_FEE_RECIPIENT,
    PUMP_MAYHEM_FEE_RECIPIENT, TOKEN_2022_PROGRAM, TOKEN_PROGRAM,
};
use crate::legs::Leg;

/// Known high-balance mainnet accounts — simulation fee-payer / virtual funder only.
pub const SIM_FUNDER_CANDIDATES: [Pubkey; 4] = [
    pubkey!("9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM"),
    pubkey!("5tzFkiKscXHK5ZXCGbXZxdw7gTjjD1mBwuoFbhUvuAi9"),
    pubkey!("2ojv9BAiHUrvsm9gxDeBFNCuoK1xKdcHGa5Y6XD7Tcuv"),
    pubkey!("FWznbcNXWQuHTawe9RxvQ2LdCENssh12dsznf4RiouN5"),
];

pub const SIM_FUND_LAMPORTS: u64 = 100_000_000; // 0.1 SOL

/// Stable mainnet fixtures (same set as sol-trade-sdk where applicable).
#[allow(dead_code)]
pub mod fixtures {
    use solana_sdk::{pubkey, pubkey::Pubkey};

    pub const CURVE_POOL: Pubkey = pubkey!("84XZdJNyBVVBqGe3BHY8n6x1jbcnxNWA5x4GetwQsjgp");
    pub const CURVE_MEME: Pubkey = pubkey!("BJ56gcrMNKDzVwjQXKToya9cAcMZvN9pz6ZzUejxQary");
    pub const CURVE_QUOTE_CARDS: Pubkey = pubkey!("CARDSccUMFKoPRZxt5vt3ksUbxEFEcnZ3H2pd3dKxYjp");
    pub const WSOL_CARDS_CPMM: Pubkey = pubkey!("3kMBV4dFBLoAaNFBcXcnaY2sY6k4k45Pcuo2zXSJHXQx");

    pub const GRAD_POOL: Pubkey = pubkey!("BUVzsLLLG7GWoyJVoU31pXiBveazA6GXTavZ9VD3CwS9");
    pub const GRAD_MEME_KNOTS: Pubkey = pubkey!("8RVBk8vxLiUHueLUW1f4izFVqN3nWippLhkohKg6EGkS");
    pub const GRAD_QUOTE_STONK: Pubkey = pubkey!("6GmAFSYs4gk3FDao5FzzySQpPZaWsa4rUJHacpMpUNgx");
    pub const WSOL_STONK_CPMM: Pubkey = pubkey!("EKPjNvowpSFPaZcroeUcgAPtdUTZZrP3v8sCKKyfpe5x");

    pub const PUMPSWAP_POOL: Pubkey = pubkey!("539m4mVWt6iduB6W8rDGPMarzNCMesuqY5eUTiiYHAgR");
    pub const PUMPSWAP_BASE: Pubkey = pubkey!("pumpCmXqMfrsAkQ5r49WcJnRayYRqmXz6ae8H7H9Dfn");
    pub const PUMPSWAP_SEED_POOL: Pubkey =
        pubkey!("9qKxzRejsV6Bp2zkefXWCbGvg61c3hHei7ShXJ4FythA");
    pub const PUMPSWAP_SEED_BASE: Pubkey =
        pubkey!("2zMMhcVQEXDtdE6vsFS7S7D5oUodfJHE8vd1gnBouauv");

    pub const AMM_V4_WSOL_USDT: Pubkey = pubkey!("7XawhbbxtsRcQA8KTkHT9f9nc6d69UwqCDh6U5EEbEmX");
    pub const AMM_V4_WSOL_USDC: Pubkey = pubkey!("58oQChx4yWmvKdwLLZzBi4ChoCc2fqCUWBkwMihLYQo2");
    pub const USDT_MINT: Pubkey = pubkey!("Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB");
    pub const USDC_MINT: Pubkey = pubkey!("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");

    pub const METEORA_DAMM_V2_POOL: Pubkey =
        pubkey!("7dVri3qjYD3uobSZL3Zth8vSCgU6r6R2nvFsh7uVfDte");
}

pub fn enabled() -> bool {
    std::env::var("RUN_MAINNET_SIM").as_deref() == Ok("1")
        || std::env::var("RUN_MAINNET_TESTS").as_deref() == Ok("1")
}

pub fn rpc_url() -> String {
    std::env::var("SOLANA_RPC_URL")
        .or_else(|_| std::env::var("RPC_URL"))
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_owned())
}

pub fn rpc() -> RpcClient {
    RpcClient::new_with_commitment(rpc_url(), CommitmentConfig::confirmed())
}

/// Create a fresh ephemeral wallet for each simulation (never uses a real private key).
pub fn create_wallet() -> Keypair {
    let wallet = Keypair::new();
    println!("[mainnet_sim] created test wallet={}", wallet.pubkey());
    wallet
}

pub fn is_transient_rpc_error(err: &impl std::fmt::Display) -> bool {
    let msg = err.to_string().to_lowercase();
    if msg.contains("transaction not found")
        || msg.contains("invalid type: null")
        || msg.contains("too old")
        || msg.contains("pruned")
    {
        return false;
    }
    msg.contains("error sending request")
        || msg.contains("429")
        || msg.contains("rate limit")
        || msg.contains("timeout")
        || msg.contains("timed out")
        || msg.contains("temporar")
        || msg.contains("connection reset")
        || msg.contains("broken pipe")
        || msg.contains("connection refused")
        || msg.contains("tls")
        || msg.contains("ssl")
        || msg.contains("eof")
        || msg.contains("dns")
        || msg.contains("network")
}

/// Retry transient public-RPC failures (rate limits / transport).
pub fn rpc_retry<T, E, F>(label: &str, mut f: F) -> Result<T, E>
where
    F: FnMut() -> Result<T, E>,
    E: std::fmt::Display,
{
    let mut last_err: Option<E> = None;
    for attempt in 0..6u32 {
        match f() {
            Ok(v) => return Ok(v),
            Err(err) => {
                let transient = is_transient_rpc_error(&err);
                println!("[{label}] rpc attempt={} err={err}", attempt + 1);
                if !transient {
                    return Err(err);
                }
                last_err = Some(err);
                let backoff_ms = 400u64.saturating_mul(1u64 << attempt.min(4));
                thread::sleep(Duration::from_millis(backoff_ms));
            }
        }
    }
    Err(last_err.expect("rpc_retry exhausted without error"))
}

/// Probe RPC; soft-skip caller when transport is down.
pub fn require_rpc(client: &RpcClient) -> bool {
    match rpc_retry("get_slot", || client.get_slot()) {
        Ok(slot) => {
            println!("[mainnet_sim] rpc ok slot={slot} url={}", rpc_url());
            true
        }
        Err(err) => {
            println!("[mainnet_sim] SKIP: RPC unavailable after retries: {err}");
            false
        }
    }
}

pub fn pick_funder(client: &RpcClient) -> Option<Pubkey> {
    for candidate in SIM_FUNDER_CANDIDATES {
        match rpc_retry("get_balance", || client.get_balance(&candidate)) {
            Ok(lamports) if lamports >= SIM_FUND_LAMPORTS * 10 => {
                println!("[mainnet_sim] funder={candidate} lamports={lamports}");
                return Some(candidate);
            }
            Ok(lamports) => println!("[mainnet_sim] skip funder {candidate}: lamports={lamports}"),
            Err(err) => println!("[mainnet_sim] skip funder {candidate}: {err}"),
        }
    }
    None
}

#[derive(Debug)]
pub enum SimVerdict {
    Ok,
    Soft(String),
    Hard(String),
}

#[allow(dead_code)]
pub fn is_fee_payer_only_soft(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    let debit = lower.contains("debit an account")
        || lower.contains("no record of a prior credit")
        || lower.contains("may not be used to pay transaction fees");
    if !debit {
        return false;
    }
    !lower.contains(" invoke [")
        && !lower.contains("anchorerror")
        && !lower.contains("program log: instruction")
}

pub fn classify_err(msg: &str) -> SimVerdict {
    let lower = msg.to_ascii_lowercase();
    let hard_needles = [
        "constraint",
        "constraintaddress",
        "accountownedbywrongprogram",
        "invalidaccountdata",
        "invalidprogramid",
        "missingrequiredsignature",
        "notenoughaccountkeys",
        "incorrectprogramid",
        "failed to serialize",
        "instruction fallback",
        "instructionfallback",
        "fallback functions are not supported",
        "a seeds constraint was violated",
        "has_one",
        "mut constraint",
        "owner constraint",
        "discriminator",
        "accountdiscriminatormismatch",
        "0xbbd",
        "0xbbf",
        "0x7dc",
        "0x7d6",
        "0x7d1",
        "0x7d3",
        "0x65",
        "sqrtpriceoutofbounds",
        "0x177b",
    ];
    for h in hard_needles {
        if lower.contains(h) {
            return SimVerdict::Hard(msg.to_string());
        }
    }
    if lower.contains("accountnotinitialized") || lower.contains("0xbc4") {
        let layout_accounts = [
            "amm_config",
            "pool_state",
            "bonding_curve",
            "lb_pair",
            "whirlpool",
            "caused by account: pool.",
            "account: pool.",
        ];
        if layout_accounts.iter().any(|a| lower.contains(a)) {
            return SimVerdict::Hard(msg.to_string());
        }
    }
    let soft_needles = [
        "insufficient",
        "insufficientfunds",
        "insufficient lamports",
        "insufficient funds",
        "no record of a prior credit",
        "debit an account",
        "slippage",
        "exceededslippage",
        "minout",
        "minimum",
        "amountoutdelta",
        "overflow",
        "underflow",
        "too little",
        "too much",
        "price",
        "liquidity",
        "binarray",
        "tickarray",
        "bitmap",
        "accountnotinitialized",
        "0xbc4",
        "buyzeroamount",
        "notenoughtokenstosell",
        "custom program error: 0x1",
        "may not be used to pay transaction fees",
        "requiregteviolated",
        "0x9ca",
    ];
    for s in soft_needles {
        if lower.contains(s) {
            return SimVerdict::Soft(msg.to_string());
        }
    }
    if lower.contains("custom program error") {
        return SimVerdict::Soft(msg.to_string());
    }
    SimVerdict::Hard(msg.to_string())
}

fn sim_config() -> RpcSimulateTransactionConfig {
    RpcSimulateTransactionConfig {
        sig_verify: false,
        replace_recent_blockhash: true,
        commitment: Some(CommitmentConfig::confirmed()),
        ..Default::default()
    }
}

fn classify_response(err: Option<impl std::fmt::Debug>, logs: Option<Vec<String>>) -> SimVerdict {
    match err {
        None => SimVerdict::Ok,
        Some(e) => {
            let logs = logs
                .unwrap_or_default()
                .into_iter()
                .rev()
                .take(12)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join(" | ");
            classify_err(&format!("{e:?}; logs={logs}"))
        }
    }
}

/// Simulate raw instructions with an explicit fee payer (no funding).
pub fn simulate_ixs(client: &RpcClient, fee_payer: &Pubkey, ixs: &[Instruction]) -> SimVerdict {
    if ixs.is_empty() {
        return SimVerdict::Hard("no instructions".into());
    }
    let blockhash = match rpc_retry("get_latest_blockhash", || client.get_latest_blockhash()) {
        Ok(b) => b,
        Err(e) => {
            if is_transient_rpc_error(&e) {
                return SimVerdict::Soft(format!("transient rpc get_latest_blockhash: {e}"));
            }
            return SimVerdict::Hard(format!("get_latest_blockhash: {e}"));
        }
    };
    let mut tx = Transaction::new_with_payer(ixs, Some(fee_payer));
    tx.message.recent_blockhash = blockhash;
    match rpc_retry("simulate", || client.simulate_transaction_with_config(&tx, sim_config())) {
        Ok(resp) => classify_response(resp.value.err, resp.value.logs),
        Err(e) => {
            if is_transient_rpc_error(&e) {
                SimVerdict::Soft(format!("transient rpc simulate: {e}"))
            } else {
                classify_err(&e.to_string())
            }
        }
    }
}

/// Create ephemeral wallet, virtually fund it, prepend fund ix, then simulate.
///
/// `business` must use `wallet.pubkey()` as the trade authority / signer.
/// Returns `None` when no funder can be reached (caller should soft-skip).
pub fn simulate_with_fresh_wallet(
    client: &RpcClient,
    wallet: &Keypair,
    business: Vec<Instruction>,
) -> Option<SimVerdict> {
    let funder = pick_funder(client)?;
    let mut ixs = Vec::with_capacity(business.len() + 1);
    ixs.push(system_instruction::transfer(
        &funder,
        &wallet.pubkey(),
        SIM_FUND_LAMPORTS,
    ));
    ixs.extend(business);
    Some(simulate_ixs(client, &funder, &ixs))
}

pub fn simulate_legs_funded(
    client: &RpcClient,
    wallet: &Keypair,
    setup: Vec<Instruction>,
    legs: &[Leg],
) -> Option<SimVerdict> {
    let mut business = setup;
    for leg in legs {
        business.push(Instruction {
            program_id: leg.program_id,
            accounts: leg.accounts.clone(),
            data: leg.data.clone(),
        });
    }
    simulate_with_fresh_wallet(client, wallet, business)
}

pub fn assert_sim_ok(scenario: &str, verdict: SimVerdict) {
    match verdict {
        SimVerdict::Ok => println!("[{scenario}] simulate OK"),
        SimVerdict::Soft(m) => println!("[{scenario}] soft fail (accepted): {m}"),
        SimVerdict::Hard(m) => panic!("[{scenario}] HARD simulate failure: {m}"),
    }
}

pub fn assert_sim_hard(scenario: &str, verdict: SimVerdict) {
    match verdict {
        SimVerdict::Hard(m) => println!("[{scenario}] HARD as expected: {m}"),
        SimVerdict::Ok => panic!("[{scenario}] expected HARD layout failure, got Ok"),
        SimVerdict::Soft(m) => panic!("[{scenario}] expected HARD layout failure, got soft: {m}"),
    }
}

#[allow(dead_code)]
pub fn recent_sigs(client: &RpcClient, program: &Pubkey, limit: usize) -> Vec<Signature> {
    recent_sigs_deep(client, program, limit)
}

/// Paginate `getSignaturesForAddress` until `target` successful signatures (or pages exhaust).
pub fn recent_sigs_deep(client: &RpcClient, address: &Pubkey, target: usize) -> Vec<Signature> {
    let mut out = Vec::with_capacity(target);
    let mut before: Option<Signature> = None;
    let mut pages = 0u32;
    while out.len() < target && pages < 12 {
        pages += 1;
        let page_limit = (target - out.len()).clamp(20, 100);
        let before_opt = before;
        let list = match rpc_retry("get_signatures", || {
            let cfg = GetConfirmedSignaturesForAddress2Config {
                before: before_opt,
                until: None,
                limit: Some(page_limit),
                commitment: Some(CommitmentConfig::confirmed()),
            };
            client.get_signatures_for_address_with_config(address, cfg)
        }) {
            Ok(v) => v,
            Err(err) => {
                println!("[mainnet_sim] recent_sigs_deep({address}) page={pages} err={err}");
                break;
            }
        };
        if list.is_empty() {
            break;
        }
        let last_sig = list.last().and_then(|s| Signature::from_str(&s.signature).ok());
        let mut got_any = false;
        for s in list {
            if s.err.is_some() {
                continue;
            }
            if let Ok(sig) = Signature::from_str(&s.signature) {
                out.push(sig);
                got_any = true;
                if out.len() >= target {
                    break;
                }
            }
        }
        before = last_sig;
        if !got_any && before.is_none() {
            break;
        }
        // Gentle pacing for public RPCs.
        thread::sleep(Duration::from_millis(80));
    }
    println!(
        "[mainnet_sim] recent_sigs_deep({address}) got={} pages={pages}",
        out.len()
    );
    out
}

/// Parse a tx with retries; no event-type filter (match in caller).
pub fn parse_tx_events(client: &RpcClient, sig: &Signature) -> Vec<sol_parser_sdk::DexEvent> {
    match rpc_retry("parse_tx", || {
        sol_parser_sdk::parse_transaction_from_rpc(client, sig, None)
    }) {
        Ok(v) => v,
        Err(err) => {
            println!("[mainnet_sim] parse_tx({sig}) err={err}");
            Vec::new()
        }
    }
}

/// Scan one or more addresses for a matching DexEvent. Returns true when handler succeeds.
pub fn scan_events(
    client: &RpcClient,
    addresses: &[Pubkey],
    target_per_address: usize,
    mut handle: impl FnMut(&Signature, &sol_parser_sdk::DexEvent) -> bool,
) -> bool {
    for addr in addresses {
        for sig in recent_sigs_deep(client, addr, target_per_address) {
            for ev in parse_tx_events(client, &sig) {
                if handle(&sig, &ev) {
                    return true;
                }
            }
        }
    }
    false
}

pub fn require_coverage(label: &str, found: bool) {
    assert!(
        found,
        "[{label}] required live mainnet coverage but found no matching events/fixtures after deep scan"
    );
}

fn account_data(client: &RpcClient, key: &Pubkey) -> Option<sol_parser_sdk::accounts::AccountData> {
    let acc = rpc_retry("get_account", || client.get_account(key)).ok()?;
    Some(sol_parser_sdk::accounts::AccountData {
        pubkey: *key,
        executable: acc.executable,
        lamports: acc.lamports,
        owner: acc.owner,
        rent_epoch: acc.rent_epoch,
        data: acc.data,
    })
}

/// SPL / Token-2022 token account amount (offset 64).
pub fn token_account_amount(client: &RpcClient, token_account: &Pubkey) -> Option<u64> {
    let acc = rpc_retry("get_account", || client.get_account(token_account)).ok()?;
    if acc.data.len() < 72 {
        return None;
    }
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&acc.data[64..72]);
    Some(u64::from_le_bytes(buf))
}

fn pumpswap_creator_vault_authority(coin_creator: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[b"creator_vault", coin_creator.as_ref()], &PUMPSWAP_PROGRAM).0
}

/// Load PumpSwap pool snapshot from on-chain account + vault balances (no recent swap needed).
pub fn load_pumpswap_pool(client: &RpcClient, pool: &Pubkey) -> Option<crate::market::PumpSwapPool> {
    use sol_parser_sdk::accounts::parse_pumpswap_pool;
    use sol_parser_sdk::DexEvent;
    use sol_parser_sdk::EventMetadata;

    let data = account_data(client, pool)?;
    let DexEvent::PumpSwapPoolAccount(ev) = parse_pumpswap_pool(&data, EventMetadata::default())?
    else {
        return None;
    };
    let p = ev.pool;
    let base_tp = mint_token_program_opt(client, &p.base_mint)?;
    let quote_tp = mint_token_program_opt(client, &p.quote_mint)?;
    let base_reserve = token_account_amount(client, &p.pool_base_token_account).unwrap_or(0);
    let quote_reserve = token_account_amount(client, &p.pool_quote_token_account).unwrap_or(0);
    if base_reserve == 0 || quote_reserve == 0 {
        println!("[mainnet_sim] load_pumpswap {pool}: empty vaults");
        return None;
    }
    let authority = pumpswap_creator_vault_authority(&p.coin_creator);
    let creator_ata = crate::ata::ata(&authority, &p.quote_mint, &quote_tp);
    Some(crate::market::PumpSwapPool {
        pool: *pool,
        base_mint: p.base_mint,
        quote_mint: p.quote_mint,
        pool_base_token_account: p.pool_base_token_account,
        pool_quote_token_account: p.pool_quote_token_account,
        base_token_program: base_tp,
        quote_token_program: quote_tp,
        coin_creator_vault_ata: creator_ata,
        coin_creator_vault_authority: authority,
        coin_creator: p.coin_creator,
        base_reserve,
        quote_reserve,
        virtual_quote_reserves: p.virtual_quote_reserves,
        lp_fee_bps: 20,
        protocol_fee_bps: 5,
        creator_fee_bps: p.creator_fee_bps,
        is_cashback_coin: p.is_cashback_coin,
        protocol_fee_recipient: if p.is_mayhem_mode {
            PUMP_MAYHEM_FEE_RECIPIENT
        } else {
            PUMPSWAP_PROTOCOL_FEE_RECIPIENT
        },
        buyback_fee_recipient: PUMPSWAP_BUYBACK_FEE_RECIPIENT,
    })
}

/// Load Raydium CPMM pool from on-chain pool_state + vault balances.
pub fn load_cpmm_pool(client: &RpcClient, pool: &Pubkey) -> Option<crate::market::CpmmPool> {
    use sol_parser_sdk::accounts::raydium_cpmm::parse_pool_state;
    use sol_parser_sdk::DexEvent;
    use sol_parser_sdk::EventMetadata;

    let data = account_data(client, pool)?;
    let DexEvent::RaydiumCpmmPoolStateAccount(ev) =
        parse_pool_state(&data, EventMetadata::default())?
    else {
        return None;
    };
    let mut pool = crate::parser::cpmm_from_pool_state(&ev);
    pool.base_token_program = mint_token_program_opt(client, &pool.base_mint)?;
    pool.quote_token_program = mint_token_program_opt(client, &pool.quote_mint)?;
    let base_raw = token_account_amount(client, &pool.base_vault).unwrap_or(0);
    let quote_raw = token_account_amount(client, &pool.quote_vault).unwrap_or(0);
    // Best-effort: subtract protocol/fund/creator fees already in account state.
    let s = &ev.pool_state;
    pool.base_reserve = base_raw
        .saturating_sub(s.protocol_fees_token_0)
        .saturating_sub(s.fund_fees_token_0)
        .saturating_sub(s.creator_fees_token_0);
    pool.quote_reserve = quote_raw
        .saturating_sub(s.protocol_fees_token_1)
        .saturating_sub(s.fund_fees_token_1)
        .saturating_sub(s.creator_fees_token_1);
    if pool.base_reserve == 0 || pool.quote_reserve == 0 {
        println!("[mainnet_sim] load_cpmm {}: empty effective reserves", pool.pool_state);
        return None;
    }
    // Fetch trade fee from amm_config when possible.
    if let Some(cfg_data) = account_data(client, &pool.amm_config) {
        if let Some(DexEvent::RaydiumCpmmAmmConfigAccount(cfg)) =
            sol_parser_sdk::accounts::raydium_cpmm::parse_amm_config(
                &cfg_data,
                EventMetadata::default(),
            )
        {
            pool.trade_fee_rate = cfg.amm_config.trade_fee_rate;
        }
    }
    if pool.trade_fee_rate == 0 {
        pool.trade_fee_rate = 2500;
    }
    Some(pool)
}

#[allow(dead_code)]
pub fn tx_fee_payer(client: &RpcClient, sig: &Signature) -> Option<Pubkey> {
    use solana_transaction_status::{EncodedTransaction, UiMessage};
    let tx = rpc_retry("get_transaction", || {
        let cfg = RpcTransactionConfig {
            encoding: Some(UiTransactionEncoding::Json),
            commitment: Some(CommitmentConfig::confirmed()),
            max_supported_transaction_version: Some(0),
        };
        client.get_transaction_with_config(sig, cfg)
    })
    .ok()?;
    match tx.transaction.transaction {
        EncodedTransaction::Json(ui) => match ui.message {
            UiMessage::Raw(m) => m.account_keys.first().and_then(|k| Pubkey::from_str(k).ok()),
            UiMessage::Parsed(m) => m
                .account_keys
                .first()
                .and_then(|k| Pubkey::from_str(&k.pubkey).ok()),
        },
        _ => None,
    }
}

/// Resolve SPL vs Token-2022 program for a mint via on-chain owner.
#[allow(dead_code)]
pub fn mint_token_program(client: &RpcClient, mint: &Pubkey) -> Pubkey {
    mint_token_program_opt(client, mint).unwrap_or(TOKEN_PROGRAM)
}

pub fn mint_token_program_opt(client: &RpcClient, mint: &Pubkey) -> Option<Pubkey> {
    let acc = rpc_retry("get_account", || client.get_account(mint)).ok()?;
    if acc.owner == TOKEN_2022_PROGRAM {
        Some(TOKEN_2022_PROGRAM)
    } else if acc.owner == TOKEN_PROGRAM {
        Some(TOKEN_PROGRAM)
    } else {
        None
    }
}

/// Overlay live on-chain account keys onto a built leg (keeps vault/config layout honest).
#[allow(dead_code)]
pub fn overlay_leg_keys(leg: &mut Leg, onchain: &[Pubkey]) {
    let n = leg.accounts.len().min(onchain.len());
    for i in 0..n {
        if leg.accounts[i].is_signer {
            continue;
        }
        if onchain[i] != Pubkey::default() {
            leg.accounts[i].pubkey = onchain[i];
        }
    }
}
