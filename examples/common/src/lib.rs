//! Shared low-latency helpers for router examples.
//!
//! Warm **before** subscribe:
//! - `TradingClient` (RPC + SWQoS + WSOL ATA + fast_init + clock)
//! - **Durable nonce pool (preferred)** and/or blockhash cache
//! - `GasFeeStrategy`
//!
//! Hot path: filter → map event → `buy`/`sell` with pre-fetched nonce (or cached hash).
//! Multi-SWQoS / MEV lanes should use durable nonce so all relays share one tx identity.

use std::collections::VecDeque;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use solana_commitment_config::CommitmentConfig;
use solana_hash::Hash;
use solana_sdk::{pubkey::Pubkey, signer::Signer};
use sol_trade_router_sdk::{
    fetch_nonce_info, keypair, DurableNonceInfo, GasFeeStrategy, RouterTradeConfig, SwqosConfig,
    TradeConfig, TradingClient,
};
use tokio::sync::Mutex;
use tokio::sync::RwLock;

/// Transaction validity for one submit: durable nonce preferred over recent blockhash.
#[derive(Clone)]
pub enum TxClock {
    DurableNonce(DurableNonceInfo),
    Blockhash(Hash),
}

impl TxClock {
    pub fn recent_blockhash(&self) -> Option<Hash> {
        match self {
            Self::Blockhash(h) => Some(*h),
            Self::DurableNonce(_) => None,
        }
    }

    pub fn durable_nonce(&self) -> Option<DurableNonceInfo> {
        match self {
            Self::DurableNonce(n) => Some(n.clone()),
            Self::Blockhash(_) => None,
        }
    }

    pub fn is_nonce(&self) -> bool {
        matches!(self, Self::DurableNonce(_))
    }
}

/// Prefetched durable-nonce pool. Take one entry per trade (same nonce across all SWQoS lanes),
/// then refresh that account in the background for the next trade.
#[derive(Clone)]
pub struct NoncePool {
    ready: Arc<Mutex<VecDeque<DurableNonceInfo>>>,
    accounts: Arc<Vec<Pubkey>>,
    rpc: Arc<sol_trade_sdk::common::SolanaRpcClient>,
}

impl NoncePool {
    /// Prefetch all accounts; returns Err if none can be loaded.
    pub async fn warm(
        rpc: Arc<sol_trade_sdk::common::SolanaRpcClient>,
        accounts: Vec<Pubkey>,
    ) -> Result<Self> {
        if accounts.is_empty() {
            return Err(anyhow!("nonce account list is empty"));
        }
        let mut ready = VecDeque::with_capacity(accounts.len());
        for acc in &accounts {
            match fetch_nonce_info(&rpc, *acc).await {
                Some(info) => ready.push_back(info),
                None => eprintln!("[nonce] failed to prefetch {acc}"),
            }
        }
        if ready.is_empty() {
            return Err(anyhow!(
                "no durable nonce could be prefetched — check NONCE_ACCOUNT authority/data"
            ));
        }
        Ok(Self {
            ready: Arc::new(Mutex::new(ready)),
            accounts: Arc::new(accounts),
            rpc,
        })
    }

    /// Hot path: take one ready nonce (no RPC). Spawns background refresh for that account.
    pub async fn take(&self) -> Result<DurableNonceInfo> {
        let info = {
            let mut q = self.ready.lock().await;
            q.pop_front()
                .ok_or_else(|| anyhow!("nonce pool empty — refresh lagging or all in-flight"))?
        };
        let account = info
            .nonce_account
            .ok_or_else(|| anyhow!("DurableNonceInfo missing nonce_account"))?;
        self.spawn_refresh(account);
        Ok(info)
    }

    fn spawn_refresh(&self, account: Pubkey) {
        let rpc = self.rpc.clone();
        let ready = self.ready.clone();
        tokio::spawn(async move {
            // Small delay so the in-flight AdvanceNonceAccount can land before we re-read.
            tokio::time::sleep(Duration::from_millis(50)).await;
            for attempt in 0..8 {
                if let Some(info) = fetch_nonce_info(&rpc, account).await {
                    ready.lock().await.push_back(info);
                    return;
                }
                tokio::time::sleep(Duration::from_millis(50 * (attempt + 1))).await;
            }
            eprintln!("[nonce] refresh failed for {account} after retries");
        });
    }

    pub fn len(&self) -> usize {
        self.accounts.len()
    }
}

/// Cached recent blockhash (fallback when no NONCE_ACCOUNT). Prefer [`NoncePool`] for production.
#[derive(Clone)]
pub struct BlockhashCache {
    inner: Arc<RwLock<Option<Hash>>>,
}

impl BlockhashCache {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn get(&self) -> Option<Hash> {
        *self.inner.read().await
    }

    pub async fn set(&self, hash: Hash) {
        *self.inner.write().await = Some(hash);
    }

    pub async fn spawn_refresher(
        &self,
        client: Arc<TradingClient>,
        interval: Duration,
    ) -> Result<()> {
        let rpc = client.infrastructure.rpc.clone();
        let first = rpc.get_latest_blockhash().await?;
        self.set(first).await;

        let cache = self.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(interval).await;
                match rpc.get_latest_blockhash().await {
                    Ok(h) => cache.set(h).await,
                    Err(e) => eprintln!("[blockhash] refresh error: {e}"),
                }
            }
        });
        Ok(())
    }
}

impl Default for BlockhashCache {
    fn default() -> Self {
        Self::new()
    }
}

pub struct WarmContext {
    pub client: Arc<TradingClient>,
    /// Preferred when `NONCE_ACCOUNT` is set.
    pub nonce_pool: Option<NoncePool>,
    /// Fallback when durable nonce is not configured.
    pub blockhash: BlockhashCache,
    pub gas: GasFeeStrategy,
    pub wait_tx_confirmed: bool,
    pub buy_sol_lamports: u64,
}

impl WarmContext {
    /// Hot-path clock: durable nonce if pooled, else cached blockhash. **No RPC.**
    pub async fn take_tx_clock(&self) -> Result<TxClock> {
        if let Some(pool) = &self.nonce_pool {
            return Ok(TxClock::DurableNonce(pool.take().await?));
        }
        let h = self
            .blockhash
            .get()
            .await
            .ok_or_else(|| anyhow!("blockhash cache empty — warm failed"))?;
        Ok(TxClock::Blockhash(h))
    }
}

fn parse_nonce_accounts() -> Result<Vec<Pubkey>> {
    let raw = match std::env::var("NONCE_ACCOUNT") {
        Ok(s) if !s.trim().is_empty() => s,
        _ => return Ok(Vec::new()),
    };
    let mut out = Vec::new();
    for part in raw.split(',') {
        let s = part.trim();
        if s.is_empty() {
            continue;
        }
        out.push(
            Pubkey::from_str(s)
                .map_err(|e| anyhow!("invalid NONCE_ACCOUNT entry '{s}': {e}"))?,
        );
    }
    Ok(out)
}

/// Build + warm a production-style router client **before** gRPC/Shred subscribe.
///
/// Set `NONCE_ACCOUNT=<pubkey>[,pubkey…]` for durable-nonce mode (recommended for
/// multi-SWQoS / MEV). Without it, falls back to a background blockhash cache.
pub async fn warm_router_client() -> Result<WarmContext> {
    let payer = keypair::load_keypair_from_env("PRIVATE_KEY")?;
    let fee_recipient = std::env::var("FEE_RECIPIENT")
        .ok()
        .and_then(|s| Pubkey::from_str(&s).ok())
        .unwrap_or_else(|| payer.pubkey());
    let fee_bps: u16 = std::env::var("FEE_BPS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let commitment = CommitmentConfig::confirmed();
    let swqos_configs: Vec<SwqosConfig> = vec![SwqosConfig::Default(rpc_url.clone())];
    let trade_config = TradeConfig::builder(rpc_url, swqos_configs, commitment)
        .create_wsol_ata_on_startup(true)
        .build();

    let mut client = TradingClient::new(
        Arc::new(payer),
        RouterTradeConfig::new(trade_config, fee_recipient, fee_bps),
    )
    .await;

    if let Ok(raw) = std::env::var("SENDER_CORES") {
        let cores: Option<Vec<usize>> = if raw.trim().is_empty() || raw == "0" {
            Some(vec![])
        } else {
            Some(
                raw.split(',')
                    .filter_map(|s| s.trim().parse().ok())
                    .collect(),
            )
        };
        client = client.with_dedicated_sender_threads(cores);
    }

    let client = Arc::new(client);
    let nonce_accounts = parse_nonce_accounts()?;
    let (nonce_pool, blockhash) = if !nonce_accounts.is_empty() {
        let pool = NoncePool::warm(client.infrastructure.rpc.clone(), nonce_accounts).await?;
        println!(
            "[warm] durable nonce pool ready ({} account(s)) — preferred low-latency clock",
            pool.len()
        );
        (Some(pool), BlockhashCache::new())
    } else {
        eprintln!(
            "[warm] WARNING: NONCE_ACCOUNT unset — using blockhash cache. \
             For multi-SWQoS / production latency, set NONCE_ACCOUNT=<nonce_pubkey>[,…]"
        );
        let blockhash = BlockhashCache::new();
        blockhash
            .spawn_refresher(client.clone(), Duration::from_millis(400))
            .await?;
        (None, blockhash)
    };

    let gas = GasFeeStrategy::new();
    gas.set_global_fee_strategy(150000, 150000, 500000, 500000, 0.001, 0.001);

    let wait_tx_confirmed = std::env::var("WAIT_TX_CONFIRMED")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let buy_sol_lamports: u64 = std::env::var("BUY_SOL_LAMPORTS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100_000);

    println!("[warm] TradingClient ready (wait_tx_confirmed={wait_tx_confirmed})");

    Ok(WarmContext {
        client,
        nonce_pool,
        blockhash,
        gas,
        wait_tx_confirmed,
        buy_sol_lamports,
    })
}
