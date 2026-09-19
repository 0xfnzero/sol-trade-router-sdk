<div align="center">
    <h1>🔀 Sol Trade Router SDK</h1>
    <h3><em>Pinocchio on-chain multi-hop router + ultra-low-latency client SDK</em></h3>
</div>

<p align="center">
    <strong>Same trading surface as <a href="https://github.com/0xfnzero/sol-trade-sdk">sol-trade-sdk</a> — SWQoS, durable nonce, SimpleBuy/Sell, ViaSol — but every swap is packed through this repo’s Pinocchio <code>Route</code> CPI (platform fee + multi-hop). Pool snapshots come from <a href="https://github.com/0xfnzero/sol-parser-sdk">sol-parser-sdk</a> events / local cache: <em>no RPC on the hot path</em>.</strong>
</p>

<p align="center">
    <a href="https://github.com/0xfnzero/sol-trade-router-sdk/blob/main/LICENSE">
        <img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License">
    </a>
    <a href="https://github.com/0xfnzero/sol-trade-router-sdk">
        <img src="https://img.shields.io/github/stars/0xfnzero/sol-trade-router-sdk?style=social" alt="GitHub stars">
    </a>
    <a href="https://github.com/0xfnzero/sol-trade-router-sdk/network">
        <img src="https://img.shields.io/github/forks/0xfnzero/sol-trade-router-sdk?style=social" alt="GitHub forks">
    </a>
</p>

<p align="center">
    <img src="https://img.shields.io/badge/Rust-000000?style=for-the-badge&logo=rust&logoColor=white" alt="Rust">
    <img src="https://img.shields.io/badge/Solana-9945FF?style=for-the-badge&logo=solana&logoColor=white" alt="Solana">
    <img src="https://img.shields.io/badge/Pinocchio-0A7B83?style=for-the-badge&logo=solana&logoColor=white" alt="Pinocchio">
    <img src="https://img.shields.io/badge/DEX-4B8BBE?style=for-the-badge&logo=bitcoin&logoColor=white" alt="DEX Trading">
</p>

<p align="center">
    <a href="https://github.com/0xfnzero/sol-trade-router-sdk/blob/main/README_CN.md">中文</a> |
    <a href="https://github.com/0xfnzero/sol-trade-router-sdk/blob/main/README.md">English</a> |
    <a href="https://fnzero.dev/">Website</a> |
    <a href="https://t.me/fnzero_group">Telegram</a> |
    <a href="https://discord.gg/vuazbGkqQE">Discord</a>
</p>

## 📋 Table of Contents

- [✨ Features](#-features)
- [📚 Documentation](#-documentation)
- [📦 Installation](#-installation)
- [🆚 vs sol-trade-sdk](#-vs-sol-trade-sdk)
- [🛠️ Use the SDK](#️-use-the-sdk)
- [🚀 Deploy the Router program](#-deploy-the-router-program)
- [📚 Examples](#-examples)
- [⚡ Low latency](#-low-latency)
- [📦 Workspace & markets](#-workspace--markets)
- [📁 Project Structure](#-project-structure)
- [🔨 Build & test](#-build--test)
- [📄 License](#-license)
- [💬 Contact](#-contact)
- [⚠️ Important Notes](#️-important-notes)

---

## ✨ Features

1. **Capability = sol-trade-sdk; execution = Route CPI** — `TradingClient`, SimpleBuy/Sell, SWQoS, durable nonce, ALT, middleware, RiskGate, exact-out, ViaSol
2. **On-chain multi-hop router** — Pinocchio program takes platform fee, then CPI into DEX legs
3. **Ultra-low latency** — warm before subscribe; prefer **durable nonce**; hot path never fetches blockhash / balances / pools
4. **Full DexType parity** — PumpFun, PumpSwap, LaunchLab/StonkFun/Bonk, Raydium CPMM / AMM V4 / CLMM, Orca Whirlpool, Meteora DLMM / DAMM V2
5. **Zero-RPC pool snapshots** — `market_from_dex_event` / `to_routed_market(DexParamEnum)`
6. **ATA policy** — WSOL / quote on the cold path; meme ATA in the buy tx
7. **Fee integrity** — on-chain `fee_source` spend ≥ `amount_in` (fee + swap)
8. **Pool guard** — optional PDA / allowlist / `stonk_strict`

## 📚 Documentation

| Guide | Purpose |
|-------|---------|
| [Low-Latency Bot Integration](docs/LOW_LATENCY_BOTS.md) | Warm / hot path, RiskGate, submit timing |
| [Durable Nonce](docs/NONCE_CACHE.md) | Preferred clock for multi-SWQoS / MEV |
| [examples/README.md](examples/README.md) | gRPC / Shred sniper & copy templates |
| [keys/README.md](keys/README.md) | Local deploy keypair (never commit) |

Related: [sol-trade-sdk docs](https://github.com/0xfnzero/sol-trade-sdk) (Trading Parameters, Gas Fee, ALT, SWQoS) apply the same way — only the swap instruction program changes.

## 📦 Installation

This workspace client crate is currently **`publish = false`**. Depend on a git checkout (or path):

```toml
[dependencies]
sol-trade-router-sdk = { git = "https://github.com/0xfnzero/sol-trade-router-sdk", package = "sol-trade-router-sdk" }
# Streaming bots only — declare if you `use sol_parser_sdk::...`:
sol-parser-sdk = "0.7.6"
```

Transitive crates.io deps (pulled automatically):

| Crate | Version | Role |
|-------|---------|------|
| [sol-trade-sdk](https://crates.io/crates/sol-trade-sdk) | `=5.0.5` | SWQoS submit, params, infra (re-exported) |
| [sol-parser-sdk](https://crates.io/crates/sol-parser-sdk) | `=0.7.6` | gRPC / Shred events (direct dep only if you subscribe) |

Do **not** add `sol-trade-sdk` to your `Cargo.toml` unless you need a symbol that is not re-exported — trading types are available from `sol_trade_router_sdk::*`.

## 🆚 vs sol-trade-sdk

| | sol-trade-sdk | sol-trade-router-sdk |
|--|---------------|----------------------|
| Client API | `TradingClient` / SimpleBuy\|Sell | Same names & params |
| Swap ix | Direct DEX program | Pinocchio **Route** CPI + platform fee |
| Hot path | Durable nonce / blockhash, no RPC | Same |
| Streaming | Use sol-parser-sdk | Same |
| On-chain | None | Deploy + `initialize` this router |

Migration: keep `TradeBuyParams` / `DexParamEnum` / SWQoS config; construct `sol_trade_router_sdk::TradingClient` with `RouterTradeConfig` (optional `fee_recipient` + `fee_bps`).

### Platform fee (optional / reserved)

If you deploy the Router yourself, you **do not have to charge a platform fee**:

| Field | Role | Self-deploy suggestion |
|-------|------|------------------------|
| `fee_bps` | Platform fee in basis points | **`0` = no fee** (on-chain skips transfer) |
| `fee_recipient` | Always stored in config | Use your own wallet as a placeholder; enable later via `update_config` |

- On-chain: when `fee_bps == 0`, `Route` takes **no** platform fee and does not validate the fee destination.
- SDK / examples: unset `FEE_BPS` defaults to `0`; unset `FEE_RECIPIENT` defaults to the payer.
- Later, if you want to charge: keep the reserved recipient (or change it) and set e.g. `fee_bps = 50` (0.50%) with `update_config`.

## 🛠️ Use the SDK

### 1. Create `TradingClient`

```rust
use std::sync::Arc;
use sol_trade_router_sdk::{
    keypair, RouterTradeConfig, SwqosConfig, TradeConfig, TradingClient,
};
use solana_commitment_config::CommitmentConfig;

let payer = Arc::new(keypair::load_keypair_from_env("PRIVATE_KEY")?);
let rpc = std::env::var("RPC_URL")?;
// Self-deploy default: no platform fee. Recipient reserved as yourself.
let fee_recipient = payer.pubkey();
let fee_bps: u16 = 0; // 0 = no fee; raise later on-chain + here if needed

let trade = TradeConfig::builder(
    rpc.clone(),
    vec![SwqosConfig::Default(rpc)],
    CommitmentConfig::confirmed(),
)
.create_wsol_ata_on_startup(true)
.build();

let client = TradingClient::new(
    payer,
    RouterTradeConfig::new(trade, fee_recipient, fee_bps),
)
.await
.with_dedicated_sender_threads(Some(vec![])); // optional SWQoS threads
```

Shared infra across wallets: `TradingInfrastructure::new` → `TradingClient::from_infrastructure(...)`.

### 2. Buy / sell (parity with sol-trade-sdk)

Prefer high-level params (same as trade-sdk):

```rust
use sol_trade_router_sdk::{
    fetch_nonce_info, AccountPolicy, BuyAmount, DexParamEnum, DexType, GasFeeStrategy,
    SimpleBuyParams, TradeTokenType,
};

// Production: durable nonce (multi-SWQoS). See docs/NONCE_CACHE.md
let nonce = fetch_nonce_info(client.get_rpc(), nonce_account).await.unwrap();
let gas = GasFeeStrategy::new();
gas.set_global_fee_strategy(150_000, 150_000, 500_000, 500_000, 0.001, 0.001);

let buy = SimpleBuyParams::with_durable_nonce(
    DexType::PumpFun,
    TradeTokenType::SOL,
    mint,
    BuyAmount::ExactInput(100_000),
    DexParamEnum::PumpFun(params), // from event / from_trade / from_dev_trade
    nonce,
    gas,
)
.account_policy(AccountPolicy::HotPathMinimal)
.grpc_recv_us(event_recv_us)
.wait_tx_confirmed(false);

client.buy_simple(buy).await?;
```

Or low-level Route builders (offline ix only):

```rust
let ixs = client.buy_with_sol(amount, &market)?.into_instructions();
let ixs = client.sell_to_sol(amount, &market)?.into_instructions();
```

| Asset | Buy | Sell |
|-------|-----|------|
| Native SOL | `buy_with_sol` | `sell_to_sol` |
| WSOL | `buy_with_wsol` | `sell_to_wsol` |
| Quote token | `buy_with_token` | `sell_to_token` |

### 3. Markets from events / params

```rust
use sol_trade_router_sdk::{market_from_dex_event, to_routed_market};

// From sol-parser-sdk DexEvent (gRPC / Shred)
if let Some(market) = market_from_dex_event(&event) { /* ... */ }

// From trade-sdk DexParamEnum
let routed = to_routed_market(&DexParamEnum::PumpFun(params), mint)?;
```

### 4. ATA strategy

| Kind | Cold path | In trade | Close |
|------|-----------|----------|-------|
| WSOL / quote | ✅ `prepare_buy_atas` | opt-in | default off |
| meme | ❌ | ✅ on buy | default off |

```rust
client.prepare_buy_atas(&market, BuyWith::Sol);
```

### 5. Admin (after deploy)

```rust
use sol_trade_router_sdk::{initialize_config, update_config, PROGRAM_ID};

// Self-deploy: fee_bps=0 (no charge); fee_recipient = yourself as placeholder
let ix = initialize_config(&PROGRAM_ID, &authority, &authority, 0);
// send once as config authority (payer)

// Later, if you want to charge: set bps + optional new recipient
let ix = update_config(&PROGRAM_ID, &authority, 50, false, Some(fee_recipient));
```

## 🚀 Deploy the Router program

You must deploy the on-chain program and call `initialize` before live Route trades. Client SDK alone is not enough.

### Prerequisites

- [Solana CLI](https://docs.solana.com/cli/install) (`solana`, `solana-keygen`)
- `cargo-build-sbf` / Solana platform-tools (same as building Pinocchio/Anchor programs)
- Deployer keypair with SOL for rent + fees

### Step 1 — Program keypair & Program ID

```bash
# Generate (once). Do NOT commit keys/*.json
solana-keygen new --outfile keys/router-keypair.json --no-bip39-passphrase
solana-keygen pubkey keys/router-keypair.json
```

Sync the printed pubkey into:

1. `programs/sol-trade-router/src/lib.rs` → `declare_id!("...")`
2. `crates/sdk/src/constants.rs` → `PROGRAM_ID`

Details: [keys/README.md](./keys/README.md).

### Step 2 — Build SBF `.so`

```bash
./scripts/build-program.sh
# → target/deploy/sol_trade_router.so  (or sbpf release path printed by the script)
```

### Step 3 — Deploy

```bash
solana config set --url https://api.mainnet-beta.solana.com   # or your RPC / devnet
solana program deploy \
  --program-id keys/router-keypair.json \
  target/deploy/sol_trade_router.so
```

Confirm the Program ID matches `PROGRAM_ID` in the SDK.

### Step 4 — Initialize config (critical)

The first caller of `initialize` becomes **config authority**. Call it immediately after deploy:

```rust
use sol_trade_router_sdk::{initialize_config, PROGRAM_ID};
// Self-deploy, no fee: fee_bps=0, reserve fee_recipient as yourself
// initialize_config(&PROGRAM_ID, &authority_pubkey, &authority_pubkey, 0)
// authority must sign; pays for config PDA rent
```

- `fee_bps = 0` → no platform fee on trades  
- `fee_recipient` is still stored (reserved); enable charging later with `update_config`  

### Step 5 — Point the bot at the program

`TradingClient` / `RouterClient` default to `PROGRAM_ID`. If you rotate IDs:

```rust
RouterTradeConfig::new(trade, fee_recipient, 0).with_program_id(your_id)
```

Bot `fee_bps` / `fee_recipient` should match on-chain intent (for self-deploy: both sides `0` + your pubkey is fine).

### Checklist

- [ ] Keypair generated; `declare_id!` + `PROGRAM_ID` match
- [ ] `.so` built and deployed
- [ ] `initialize` sent (you own config authority)
- [ ] Self-deploy: `fee_bps = 0` (or intentional fee); if charging, bot matches on-chain rate/recipient
- [ ] Durable nonce accounts created for the trading payer ([NONCE_CACHE.md](docs/NONCE_CACHE.md))

## 📚 Examples

| Package | Stream | Behavior |
|---------|--------|----------|
| `grpc_event_listen` | Yellowstone gRPC | Listen + `market_from_dex_event` (no submit) |
| `pumpfun_sniper_trading` | gRPC | Creator-first buy sniper → Route |
| `pumpfun_copy_trading` | gRPC | Copy first PumpFun trade → Route |
| `pumpfun_shred_sniper` | ShredStream | Creator-first buy sniper → Route |

```bash
# Safe listen-only
export GRPC_ENDPOINT=https://your-yellowstone.example
cargo run -p grpc_event_listen

# Live bots (real mainnet txs)
export PRIVATE_KEY=...
export RPC_URL=https://your-rpc.example
export GRPC_ENDPOINT=https://your-yellowstone.example
export NONCE_ACCOUNT=<nonce_pubkey>[,...]   # recommended
cargo run -p pumpfun_sniper_trading
```

Warm helper: `examples/common` (`warm_router_client` + `take_tx_clock`). Full notes: [examples/README.md](./examples/README.md).

## ⚡ Low latency

```text
warm client + nonce pool + ATAs  →  subscribe
hot: filter → map event → take_tx_clock → buy/sell → submit
```

- Prefer **`NONCE_ACCOUNT`** over blockhash for multi-SWQoS
- Never create the client / fetch blockhash / query balances on the event path
- Default examples use `wait_tx_confirmed=false`

→ [docs/LOW_LATENCY_BOTS.md](./docs/LOW_LATENCY_BOTS.md) · [docs/NONCE_CACHE.md](./docs/NONCE_CACHE.md)

## 📦 Workspace & markets

| Path | Crate | Description |
|------|-------|-------------|
| `programs/sol-trade-router` | `sol-trade-router` | On-chain fee + multi-hop `route` |
| `crates/sdk` | `sol-trade-router-sdk` | Client SDK |

| Market | Notes |
|--------|-------|
| PumpFun | Bonding curve V1 / V2 |
| PumpSwap | Graduated `pAMM` |
| LaunchLab / Bonk / StonkFun | Curve + graduated CPMM / ViaSol |
| Raydium CPMM / AMM V4 / CLMM | |
| Orca Whirlpool | |
| Meteora DLMM / DAMM V2 | |

## 📁 Project Structure

```text
sol-trade-router-sdk/
├── programs/sol-trade-router/     # Pinocchio on-chain program
├── crates/sdk/                    # sol-trade-router-sdk client
├── docs/                          # LOW_LATENCY + NONCE guides
├── examples/                      # gRPC / Shred bots + common warm helper
├── scripts/check.sh
├── scripts/build-program.sh
├── keys/                          # deploy keypair (gitignored)
├── README.md
└── README_CN.md
```

## 🔨 Build & test

```bash
./scripts/check.sh
cargo check -p sol-trade-router-sdk
cargo test -p sol-trade-router-sdk --lib offline_ -- --nocapture

# Optional mainnet simulate (ephemeral wallets; Soft if router undeployed)
RUN_MAINNET_SIM=1 cargo test -p sol-trade-router-sdk --lib mainnet_ -- --nocapture --test-threads=1

./scripts/build-program.sh
```

## 📄 License

MIT License

## 💬 Contact

- Website: https://fnzero.dev/
- Repo: https://github.com/0xfnzero/sol-trade-router-sdk
- Telegram: https://t.me/fnzero_group
- Discord: https://discord.gg/vuazbGkqQE

## ⚠️ Important Notes

1. Test on devnet / simulate before mainnet size
2. Never commit `keys/*.json`
3. Call `initialize` immediately after deploy
4. Keep `declare_id!` and SDK `PROGRAM_ID` in sync
5. Durable nonce ≠ quote validity — refresh pool state from events
6. Comply with applicable laws and regulations
