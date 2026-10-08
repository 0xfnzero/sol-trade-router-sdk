//! Shared helpers for gated mainnet `simulateTransaction` tests.
//!
//! Pattern (aligned with sol-trade-sdk):
//! 1. `Keypair::new()` ephemeral wallet
//! 2. Virtually fund from a high-balance mainnet account (`sigVerify=false`)
//! 3. Build real DEX legs from live `sol-parser-sdk` events / fixtures
//! 4. `simulateTransaction` — never submit on-chain

#![cfg(test)]

use solana_client::rpc_client::GetConfirmedSignaturesForAddress2Config;
use solana_client::rpc_client::RpcClient;
use solana_client::rpc_config::{RpcSimulateTransactionConfig, RpcTransactionConfig};
use solana_commitment_config::CommitmentConfig;
use solana_sdk::{
    instruction::Instruction,
    pubkey,
    pubkey::Pubkey,
    signature::{Keypair, Signature},
    signer::Signer,
    transaction::{Transaction, VersionedTransaction},
};
use solana_system_interface::instruction as system_instruction;
use solana_transaction_status::UiTransactionEncoding;
use std::str::FromStr;
use std::thread;
use std::time::Duration;

use crate::constants::{TOKEN_2022_PROGRAM, TOKEN_PROGRAM};
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
    pub const PUMPSWAP_SEED_POOL: Pubkey = pubkey!("9qKxzRejsV6Bp2zkefXWCbGvg61c3hHei7ShXJ4FythA");
    pub const PUMPSWAP_SEED_BASE: Pubkey = pubkey!("2zMMhcVQEXDtdE6vsFS7S7D5oUodfJHE8vd1gnBouauv");

    pub const AMM_V4_WSOL_USDT: Pubkey = pubkey!("7XawhbbxtsRcQA8KTkHT9f9nc6d69UwqCDh6U5EEbEmX");
    pub const AMM_V4_WSOL_USDC: Pubkey = pubkey!("58oQChx4yWmvKdwLLZzBi4ChoCc2fqCUWBkwMihLYQo2");
    pub const USDT_MINT: Pubkey = pubkey!("Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB");
    pub const USDC_MINT: Pubkey = pubkey!("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");

    pub const METEORA_DAMM_V2_POOL: Pubkey =
        pubkey!("7dVri3qjYD3uobSZL3Zth8vSCgU6r6R2nvFsh7uVfDte");
}

pub fn rpc_url() -> String {
    std::env::var("SOLANA_RPC_URL")
        .or_else(|_| std::env::var("RPC_URL"))
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_owned())
}

pub fn rpc() -> RpcClient {
    RpcClient::new_with_timeout_and_commitment(
        rpc_url(),
        Duration::from_secs(12),
        CommitmentConfig::confirmed(),
    )
}

/// Create a fresh ephemeral wallet for each simulation (never uses a real private key).
pub fn create_wallet() -> Keypair {
    let wallet = Keypair::new();
    println!("[mainnet_sim] created test wallet={}", wallet.pubkey());
    wallet
}

/// Create `n` distinct ephemeral wallets (for multi-wallet / concurrency checks).
pub fn create_wallets(n: usize) -> Vec<Keypair> {
    let mut out = Vec::with_capacity(n);
    let mut seen = std::collections::HashSet::new();
    while out.len() < n {
        let w = Keypair::new();
        if seen.insert(w.pubkey()) {
            println!(
                "[mainnet_sim] created test wallet[{}]={}",
                out.len(),
                w.pubkey()
            );
            out.push(w);
        }
    }
    out
}

/// True when the Pinocchio router program account exists on this cluster.
pub fn router_program_deployed(client: &RpcClient) -> bool {
    match rpc_retry("get_account(PROGRAM_ID)", || {
        client.get_account(&crate::constants::PROGRAM_ID)
    }) {
        Ok(acc) if acc.executable => {
            println!(
                "[mainnet_sim] router PROGRAM_ID={} deployed executable=true",
                crate::constants::PROGRAM_ID
            );
            true
        }
        Ok(_) => {
            println!(
                "[mainnet_sim] router PROGRAM_ID={} exists but not executable",
                crate::constants::PROGRAM_ID
            );
            false
        }
        Err(err) => {
            println!(
                "[mainnet_sim] router PROGRAM_ID={} not on cluster: {err}",
                crate::constants::PROGRAM_ID
            );
            false
        }
    }
}

/// Assert a built trade contains a Route ix targeting [`crate::constants::PROGRAM_ID`].
pub fn assert_route_ix(built: &crate::trade::BuiltTrade, label: &str) {
    let route = built
        .route
        .as_ref()
        .unwrap_or_else(|| panic!("[{label}] BuiltTrade missing route ix"));
    assert_eq!(
        route.program_id,
        crate::constants::PROGRAM_ID,
        "[{label}] route program_id must be router PROGRAM_ID"
    );
    assert_eq!(
        route.data.first().copied(),
        Some(crate::route_ix::TAG_ROUTE),
        "[{label}] route data must start with TAG_ROUTE"
    );
    assert!(
        !route.accounts.is_empty(),
        "[{label}] route accounts must be non-empty"
    );
    println!(
        "[{label}] route ix ok program={} accounts={} data_len={}",
        route.program_id,
        route.accounts.len(),
        route.data.len()
    );
}

/// Extract the single DEX leg actually emitted by a high-level builder, for
/// direct simulation independent of the unchanged deployed router's header.
pub fn single_route_leg_for_direct_simulation(built: &crate::trade::BuiltTrade) -> Leg {
    let route = built.route.as_ref().expect("high-level route is required");
    assert!(route.data.len() >= 86, "truncated single-leg route");
    assert_eq!(route.data[0], crate::route_ix::TAG_ROUTE);
    assert_eq!(route.data[18], 1, "single-hop route required");
    let program_id = Pubkey::new_from_array(route.data[51..83].try_into().unwrap());
    let account_count = route.data[83] as usize;
    assert!((1..=crate::legs::MAX_LEG_ACCOUNTS).contains(&account_count));
    let data_len = u16::from_le_bytes(route.data[84..86].try_into().unwrap()) as usize;
    assert_eq!(route.data.len(), 86 + data_len);
    assert!(route.accounts.len() >= 6 + account_count);
    Leg {
        program_id,
        accounts: route.accounts[6..6 + account_count].to_vec(),
        data: route.data[86..].to_vec(),
    }
}

/// Simulate a full [`BuiltTrade`] (setup + Route + cleanup) with a freshly funded wallet.
///
/// Missing programs or execution failures never count as successful simulation.
pub fn simulate_built_trade(
    client: &RpcClient,
    wallet: &Keypair,
    built: crate::trade::BuiltTrade,
) -> Option<SimVerdict> {
    assert_route_ix(&built, "simulate_built_trade");
    let ixs = built.into_instructions();
    simulate_with_fresh_wallet(client, wallet, ixs)
}

/// Simulate DEX legs only (direct program calls) — used as layout coverage when
/// the router program is not yet deployed on mainnet.
pub fn simulate_direct_legs(
    client: &RpcClient,
    wallet: &Keypair,
    setup: Vec<Instruction>,
    legs: &[crate::legs::Leg],
) -> Option<SimVerdict> {
    simulate_legs_funded(client, wallet, setup, legs)
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
    for attempt in 0..3u32 {
        match f() {
            Ok(v) => return Ok(v),
            Err(err) => {
                let transient = is_transient_rpc_error(&err);
                println!(
                    "[{label}] rpc attempt={} err={}",
                    attempt + 1,
                    redact_rpc(&err.to_string())
                );
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
            println!("[mainnet_sim] rpc ok slot={slot}");
            true
        }
        Err(err) => {
            panic!(
                "mainnet test was requested but RPC is unavailable: {}",
                redact_rpc(&err.to_string())
            )
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
    // Oversized packet is an RPC/encoding limit (no ALT), not a layout bug.
    if lower.contains("too large")
        || lower.contains("versionedtransaction too large")
        || lower.contains("max: encoded/raw")
        || (lower.contains("1644") && lower.contains("1232"))
    {
        return SimVerdict::Soft(msg.to_string());
    }
    let hard_needles = [
        "constraint",
        "constraintaddress",
        "accountownedbywrongprogram",
        "invalidaccountdata",
        "invalidprogramid",
        "incorrecttokenprogramid",
        "invalidspltokenprogram",
        "invalid spl token program",
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
        // Raydium / SPL Token: InvalidSplTokenProgram / IncorrectProgramId
        "0x26",
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
    // SPL Token InsufficientFunds is exactly custom 0x1 — do not soft-match 0x10/0x1771/etc.
    if custom_program_error_code(&lower) == Some(1) {
        return SimVerdict::Soft(msg.to_string());
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
        "buy zero amount",
        "zerobaseamount",
        "zero amount",
        "notenoughtokenstosell",
        "unsupportedquotemint",
        "may not be used to pay transaction fees",
        "requiregteviolated",
        "0x9ca",
        "0x17af", // PumpFun UnsupportedQuoteMint = 6063 (V1 vs V2 path mismatch)
        // Router program may not be deployed on mainnet yet — treat as soft for route-path sims.
        "attempt to load a program that does not exist",
        "program account does not exist",
        "programaccountnotfound",
        "invalid program for execution",
        // Route + ATA setup often exceeds legacy simulate packet without ALT.
        "too large",
        "1644",
        "1232",
        "encoded/raw",
        "transaction too large",
        "versionedtransaction too large",
    ];
    for s in soft_needles {
        if lower.contains(s) {
            return SimVerdict::Soft(msg.to_string());
        }
    }
    // Unknown custom program errors are HARD — do not soft-hide layout bugs (e.g. 0x26).
    SimVerdict::Hard(msg.to_string())
}

fn custom_program_error_code(lower: &str) -> Option<u64> {
    const PREFIX: &str = "custom program error: 0x";
    let idx = lower.find(PREFIX)?;
    let rest = &lower[idx + PREFIX.len()..];
    let hex: String = rest.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
    if hex.is_empty() {
        return None;
    }
    u64::from_str_radix(&hex, 16).ok()
}

fn sim_config() -> RpcSimulateTransactionConfig {
    RpcSimulateTransactionConfig {
        sig_verify: false,
        replace_recent_blockhash: true,
        commitment: Some(CommitmentConfig::confirmed()),
        ..Default::default()
    }
}

pub(crate) fn classify_response(err: Option<impl std::fmt::Debug>, logs: Option<Vec<String>>) -> SimVerdict {
    match err {
        None => SimVerdict::Ok,
        Some(e) => {
            // Fault assertions need the target invocation as well as the final
            // failure; long inner-CPI diagnostics can push it out of a tail slice.
            let logs = logs
                .unwrap_or_default()
                .join(" | ");
            classify_err(&format!("{e:?}; logs={logs}"))
        }
    }
}

/// Simulate raw instructions with an explicit fee payer (no funding).
fn simulate_ixs_signed(
    client: &RpcClient,
    fee_payer: &Pubkey,
    ixs: &[Instruction],
    wallet: Option<&Keypair>,
) -> SimVerdict {
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
    let mut tx: VersionedTransaction = tx.into();
    if bincode::serialized_size(&tx).expect("transaction size") > 1232 {
        let message = solana_message::v1::Message::try_compile_with_config(
            fee_payer,
            ixs,
            blockhash,
            solana_message::v1::TransactionConfig::empty()
                .with_compute_unit_limit(1_400_000)
                .with_loaded_accounts_data_size_limit(64 * 1024 * 1024),
        )
        .expect("compile V1 simulation transaction");
        tx = VersionedTransaction {
            signatures: vec![Signature::default(); message.header.num_required_signatures as usize],
            message: solana_message::VersionedMessage::V1(message),
        };
    }
    let authority_signed = wallet
        .map(|wallet| sign_and_verify_authority(&mut tx, wallet))
        .unwrap_or(false);
    // The real trade authority signs the exact message. The virtual funding
    // prefix uses a public funder whose key we do not own, so RPC sigVerify stays
    // false. Report this distinction rather than claiming full RPC verification.
    record_evidence(
        "signed-simulation",
        &serde_json::json!({
            "transaction": tx, "authority": wallet.map(|w| w.pubkey().to_string()),
            "authority_signature_verified": authority_signed, "rpc_sig_verify": false, "virtual_funder_signed": false,
        }),
    );
    match rpc_retry("simulate", || {
        client.simulate_transaction_with_config(&tx, sim_config())
    }) {
        Ok(resp) => {
            record_evidence(
                "simulation-result",
                &serde_json::json!({"transaction": tx, "authority_signature": tx.signatures.iter().find(|signature| **signature != Signature::default()).map(ToString::to_string), "response": resp}),
            );
            classify_response(resp.value.err, resp.value.logs)
        }
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
/// Returns `None` when virtual funding is unavailable; explicit tests must fail.
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
    Some(simulate_ixs_signed(client, &funder, &ixs, Some(wallet)))
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
        SimVerdict::Soft(m) => panic!("[{scenario}] simulation did not succeed: {m}"),
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
        let last_sig = list
            .last()
            .and_then(|s| Signature::from_str(&s.signature).ok());
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
    let tx = rpc_retry("get_latest_transaction", || {
        client.get_transaction_with_config(
            sig,
            RpcTransactionConfig {
                encoding: Some(UiTransactionEncoding::Base64),
                commitment: Some(CommitmentConfig::confirmed()),
                max_supported_transaction_version: Some(1),
            },
        )
    })
    .unwrap_or_else(|err| panic!("latest transaction {sig}: {}", redact_rpc(&err.to_string())));
    let wire = tx
        .transaction
        .transaction
        .decode()
        .expect("RPC returned decodable transaction");
    wire.verify_and_hash_message()
        .expect("latest on-chain transaction has valid signatures");
    assert_eq!(
        wire.signatures.first(),
        Some(sig),
        "RPC returned a different signature"
    );
    let events =
        sol_parser_sdk::parse_rpc_transaction(&tx, None).expect("latest transaction parser");
    for event in &events {
        assert_eq!(event.metadata().signature, *sig, "parsed event signature");
        assert_eq!(event.metadata().slot, tx.slot, "parsed event slot");
    }
    record_evidence(
        "parsed-transaction",
        &serde_json::json!({"signature": sig.to_string(),
        "slot": tx.slot, "signature_verified": true, "event_count": events.len(), "transaction": tx}),
    );
    events
}

fn redact_rpc(text: &str) -> String {
    text.replace(&rpc_url(), "[RPC]")
}

pub fn record_evidence(kind: &str, value: &serde_json::Value) {
    use std::io::Write;
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let Ok(directory) = std::env::var("ROUTER_TEST_EVIDENCE_DIR") else {
        return;
    };
    let _guard = LOCK.lock().expect("evidence lock");
    std::fs::create_dir_all(&directory).expect("create evidence directory");
    let path = std::path::Path::new(&directory).join(format!("{kind}.jsonl"));
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .expect("open evidence");
    writeln!(file, "{}", value).expect("write evidence");
}

/// Cryptographically verify the ephemeral authority, without signing the public
/// virtual funder's slot. Fund-only scenarios do not require an authority signer.
pub fn sign_and_verify_authority(tx: &mut VersionedTransaction, wallet: &Keypair) -> bool {
    let n = tx.message.header().num_required_signatures as usize;
    if let Some(index) = tx.message.static_account_keys()[..n]
        .iter()
        .position(|k| *k == wallet.pubkey())
    {
        let message = tx.message.serialize();
        tx.signatures[index] = wallet.sign_message(&message);
        assert!(
            tx.signatures[index].verify(wallet.pubkey().as_ref(), &message),
            "trade authority signature invalid"
        );
        true
    } else {
        false
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

#[allow(dead_code)]
pub fn require_coverage(label: &str, found: bool) {
    assert!(
        found,
        "[{label}] required live mainnet coverage but found no matching events/fixtures after deep scan"
    );
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

/// Load PumpSwap pool snapshot from on-chain account + vault balances (no recent swap needed).
pub fn load_pumpswap_pool(
    client: &RpcClient,
    pool: &Pubkey,
) -> Option<crate::market::PumpSwapPool> {
    use sol_trade_sdk::trading::core::params::PumpSwapParams;
    let _ = client;
    let runtime = tokio::runtime::Runtime::new().expect("test runtime");
    let rpc = sol_trade_sdk::common::SolanaRpcClient::new_with_timeout_and_commitment(
        rpc_url(),
        Duration::from_secs(12),
        CommitmentConfig::confirmed(),
    );
    let params = runtime
        .block_on(PumpSwapParams::from_pool_address_by_rpc(&rpc, pool))
        .unwrap_or_else(|err| {
            panic!(
                "current PumpSwap pool {pool}: {}",
                redact_rpc(&err.to_string())
            )
        });
    Some(crate::adapter::pumpswap_from_params(&params))
}

/// Current config, creator fees and epoch-sensitive mint transfer fees are
/// loaded by the current Rust SDK; zero rates must not become guessed defaults.
pub fn load_cpmm_pool(client: &RpcClient, pool: &Pubkey) -> Option<crate::market::CpmmPool> {
    use sol_trade_sdk::trading::core::params::RaydiumCpmmParams;
    let _ = client;
    let runtime = tokio::runtime::Runtime::new().expect("test runtime");
    let rpc = sol_trade_sdk::common::SolanaRpcClient::new_with_timeout_and_commitment(
        rpc_url(),
        Duration::from_secs(12),
        CommitmentConfig::confirmed(),
    );
    let params = runtime
        .block_on(RaydiumCpmmParams::from_pool_address_by_rpc(&rpc, pool))
        .unwrap_or_else(|err| panic!("current CPMM pool {pool}: {}", redact_rpc(&err.to_string())));
    Some(crate::adapter::cpmm_from_params(&params))
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
            UiMessage::Raw(m) => m
                .account_keys
                .first()
                .and_then(|k| Pubkey::from_str(k).ok()),
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

/// Mint pubkey stored at offset 0 of an SPL / Token-2022 token account.
pub fn token_account_mint(client: &RpcClient, token_account: &Pubkey) -> Option<Pubkey> {
    let acc = rpc_retry("get_account", || client.get_account(token_account)).ok()?;
    if acc.data.len() < 32 {
        return None;
    }
    let mint = Pubkey::new_from_array(acc.data[0..32].try_into().ok()?);
    if mint == Pubkey::default() {
        None
    } else {
        Some(mint)
    }
}

/// Fill AMM V4 coin/pc mints, reserves, and token_program from vault accounts.
pub fn fill_amm_v4_mints(client: &RpcClient, pool: &mut crate::market::RaydiumAmmV4Pool) -> bool {
    let Some(coin) = token_account_mint(client, &pool.token_coin) else {
        return false;
    };
    let Some(pc) = token_account_mint(client, &pool.token_pc) else {
        return false;
    };
    pool.coin_mint = coin;
    pool.pc_mint = pc;
    if let Some(tp) = mint_token_program_opt(client, &coin) {
        pool.token_program = tp;
    } else if pool.token_program == Pubkey::default() {
        pool.token_program = TOKEN_PROGRAM;
    }
    // Router buy quotes need live vault balances (swap events do not carry reserves).
    pool.coin_reserve = token_account_amount(client, &pool.token_coin).unwrap_or(0);
    pool.pc_reserve = token_account_amount(client, &pool.token_pc).unwrap_or(0);
    if pool.coin_reserve == 0 || pool.pc_reserve == 0 {
        println!(
            "[mainnet_sim] fill_amm_v4 {}: empty vaults coin={} pc={}",
            pool.amm, pool.coin_reserve, pool.pc_reserve
        );
        return false;
    }
    true
}

/// Fail explicit live runs when no matching fixture or event was exercised.
pub fn soft_coverage(label: &str, found: bool) {
    if found {
        println!("[{label}] live coverage ok");
    } else {
        panic!("[{label}] no matching live events/fixtures; coverage was not executed");
    }
}

/// Load Raydium AMM V4 pool from on-chain AmmInfo + vault balances (no swap events needed).
pub fn load_amm_v4_pool(
    client: &RpcClient,
    amm: &Pubkey,
) -> Option<crate::market::RaydiumAmmV4Pool> {
    use sol_trade_sdk::trading::core::params::RaydiumAmmV4Params;
    let _ = client;
    let runtime = tokio::runtime::Runtime::new().expect("test runtime");
    let rpc = sol_trade_sdk::common::SolanaRpcClient::new_with_timeout_and_commitment(
        rpc_url(),
        Duration::from_secs(12),
        CommitmentConfig::confirmed(),
    );
    let params = runtime
        .block_on(RaydiumAmmV4Params::from_amm_address_by_rpc(&rpc, *amm))
        .unwrap_or_else(|err| {
            panic!(
                "current AMM V4 pool {amm}: {}",
                redact_rpc(&err.to_string())
            )
        });
    Some(crate::adapter::raydium_amm_v4_from_params(&params))
}

/// Overlay mint owners onto a CLMM snapshot (Token-2022 safe ATAs).
pub fn fill_clmm_token_programs(
    client: &RpcClient,
    pool: &mut crate::market::RaydiumClmmPool,
) -> bool {
    let Some(t0) = mint_token_program_opt(client, &pool.token_0_mint) else {
        return false;
    };
    let Some(t1) = mint_token_program_opt(client, &pool.token_1_mint) else {
        return false;
    };
    crate::parser::clmm_apply_token_programs(pool, t0, t1);
    true
}

/// Overlay mint owners onto a Whirlpool snapshot.
pub fn fill_whirlpool_token_programs(
    client: &RpcClient,
    pool: &mut crate::market::WhirlpoolPool,
) -> bool {
    let Some(ta) = mint_token_program_opt(client, &pool.mint_a) else {
        return false;
    };
    let Some(tb) = mint_token_program_opt(client, &pool.mint_b) else {
        return false;
    };
    pool.token_program_a = ta;
    pool.token_program_b = tb;
    true
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

/// Match the requested venue so nested swaps cannot satisfy another protocol's coverage.
pub fn is_trade_for_program(event: &sol_parser_sdk::DexEvent, program: Pubkey) -> bool {
    use crate::constants::*;
    use sol_parser_sdk::DexEvent::*;
    match event {
        PumpFunTrade(_) | PumpFunBuy(_) | PumpFunSell(_) | PumpFunBuyExactSolIn(_) => {
            program == PUMPFUN_PROGRAM
        }
        PumpSwapBuy(_) | PumpSwapSell(_) => program == PUMPSWAP_PROGRAM,
        RaydiumCpmmSwap(_) => program == RAYDIUM_CPMM_PROGRAM,
        RaydiumAmmV4Swap(_) => program == RAYDIUM_AMM_V4_PROGRAM,
        RaydiumClmmSwap(_) => program == RAYDIUM_CLMM_PROGRAM,
        OrcaWhirlpoolSwap(_) => program == ORCA_WHIRLPOOL_PROGRAM,
        MeteoraDlmmSwap(_) => program == METEORA_DLMM_PROGRAM,
        MeteoraDammV2Swap(_) => program == METEORA_DAMM_V2_PROGRAM,
        RaydiumLaunchlabTrade(_) => program == LAUNCHLAB_PROGRAM,
        _ => false,
    }
}

/// Empty input ATAs must fail with a balance error, not an unrelated ABI failure.
pub fn assert_sim_balance_failure(scenario: &str, verdict: Option<SimVerdict>) {
    let Some(SimVerdict::Soft(message) | SimVerdict::Hard(message)) = verdict else {
        panic!("[{scenario}] expected an unfunded-input failure");
    };
    let lower = message.to_lowercase();
    assert!(
        lower.contains("insufficient funds")
            || lower.contains("insufficientfunds")
            || lower.contains("notenoughtokenstosell")
            || lower.contains("insufficient token")
            || lower.contains("error: insufficient balance")
            || (scenario == "launchlab_sell"
                && lower.contains("sell_exact_in.rs:16")
                && lower.contains("requiregteviolated")
                && lower.contains("left: 0")),
        "[{scenario}] expected a balance failure, got: {message}"
    );
}

/// Refresh the curve, mint owner, creator and fee recipients before live trades.
pub fn load_pumpfun_pool(mint: &Pubkey) -> Option<crate::market::PumpFunPool> {
    let runtime = tokio::runtime::Runtime::new().expect("test runtime");
    let rpc = sol_trade_sdk::common::SolanaRpcClient::new_with_timeout_and_commitment(
        rpc_url(),
        Duration::from_secs(12),
        CommitmentConfig::confirmed(),
    );
    let loaded = runtime.block_on(crate::adapter::load_routed_market_by_rpc(
        &rpc,
        crate::adapter::LoadMarketRequest::PumpFun { mint: *mint },
        &Pubkey::new_unique(),
    ));
    match loaded {
        Ok((_, market)) => match market.market {
            crate::market::Market::PumpFunInner(pool) => Some(pool),
            _ => panic!("unexpected Pump market"),
        },
        Err(err) if err.to_string().contains("completed PumpFun") => None,
        Err(err) => panic!(
            "current Pump curve {mint}: {}",
            redact_rpc(&err.to_string())
        ),
    }
}

/// Load current tick arrays for the requested direction, including reverse trades.
pub fn load_clmm_pool(
    pool: &Pubkey,
    input: &Pubkey,
    output: &Pubkey,
) -> crate::market::RaydiumClmmPool {
    let runtime = tokio::runtime::Runtime::new().expect("test runtime");
    let rpc = sol_trade_sdk::common::SolanaRpcClient::new_with_timeout_and_commitment(
        rpc_url(),
        Duration::from_secs(12),
        CommitmentConfig::confirmed(),
    );
    let params = runtime
        .block_on(
            sol_trade_sdk::trading::core::params::RaydiumClmmParams::from_pool_address_by_rpc(
                &rpc, pool, input, output,
            ),
        )
        .unwrap_or_else(|err| panic!("current CLMM pool {pool}: {}", redact_rpc(&err.to_string())));
    crate::adapter::raydium_clmm_from_params(&params)
}
