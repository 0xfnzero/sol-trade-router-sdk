<div align="center">
    <h1>🔀 Sol Trade Router SDK</h1>
    <h3><em>Pinocchio on-chain multi-hop router + zero-RPC hot-path client SDK</em></h3>
</div>

<p align="center">
    <strong>A Solana monorepo with a Pinocchio router program and a Rust client SDK for fee-aware, multi-hop DEX swaps. Pool snapshots come from <a href="https://github.com/0xfnzero/sol-parser-sdk">sol-parser-sdk</a> events / local cache — no RPC on the hot path.</strong>
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
- [📦 Workspace](#-workspace)
- [🔖 Program ID](#-program-id)
- [🛠️ Usage](#️-usage)
- [📁 Project Structure](#-project-structure)
- [🔨 Build](#-build)
- [📄 License](#-license)
- [💬 Contact](#-contact)
- [⚠️ Important Notes](#️-important-notes)

---

## ✨ Features

1. **On-chain multi-hop router**: Pinocchio program takes platform fee then CPI into DEX legs
2. **Zero-RPC hot path**: `market_from_dex_event` builds pools from [sol-parser-sdk](https://github.com/0xfnzero/sol-parser-sdk) events
3. **Full DexType parity with sol-trade-sdk**: PumpFun, PumpSwap, LaunchLab/StonkFun/Bonk, Raydium CPMM / AMM V4 / CLMM, Orca Whirlpool, Meteora DLMM / DAMM V2
4. **Symmetric API**: `buy_with_{sol|wsol|token}` / `sell_to_{sol|wsol|token}`
5. **ATA policy**: WSOL / stock prepared on the cold path; meme ATA created in the same buy tx
6. **Fee integrity**: On-chain check that `fee_source` spends ≥ `amount_in` (fee + swap)
7. **Pool guard**: optional PDA / allowlist checks via `market_from_dex_event_checked`

## 📦 Workspace

| Path | Crate | Description |
|------|-------|-------------|
| `programs/sol-trade-router` | `sol-trade-router` | On-chain program: fee + multi-hop `route` CPI |
| `crates/sdk` | `sol-trade-router-sdk` | Client SDK: offline instruction building |

### Supported markets

Aligned with [sol-trade-sdk](https://github.com/0xfnzero/sol-trade-sdk) `DexType` coverage (Router CPI version):

| Market | Type | Notes |
|--------|------|-------|
| PumpFun | Inner curve | SOL / quote bonding curve (V1 / V2) |
| PumpSwap | Outer AMM | Graduated PumpFun pools (`pAMM`) |
| LaunchLab / Bonk / StonkFun | Inner curve | Bonding curve, arbitrary quote (incl. CARDS) |
| StonkFun graduated / Raydium CPMM | Outer CPMM | Raydium CPMM |
| Raydium AMM V4 | AMM | `SwapBaseInV2` |
| Raydium CLMM | CLMM | `swap_v2` + tick arrays |
| Orca Whirlpool | CLMM | `swap_v2` + tick arrays |
| Meteora DLMM | DLMM | `swap2` + bin arrays |
| Meteora DAMM V2 | Dynamic AMM | `swap2` exact-in |

Snapshots are built from [`sol-parser-sdk`](https://github.com/0xfnzero/sol-parser-sdk) `DexEvent` via `market_from_dex_event`.

## 🔖 Program ID

```text
CMrrMgrEvXW3oo6RtxnneDf5D5TeujfbqveFiKuvqrYg
```

Must match `declare_id!` in the program and `PROGRAM_ID` in the SDK. Local deploy keypair lives under `keys/` (gitignored) — see [keys/README.md](./keys/README.md).

## 🛠️ Usage

### Naming

| Asset | Buy | Sell |
|-------|-----|------|
| Native SOL | `buy_with_sol` | `sell_to_sol` |
| WSOL | `buy_with_wsol` | `sell_to_wsol` |
| Stock / quote | `buy_with_token` | `sell_to_token` |
| Custom | `buy_with_opts` | `sell_with_opts` |

### ATA strategy

| Kind | Create ahead | Create in trade | Close |
|------|--------------|-----------------|-------|
| **WSOL / stock (quote)** | ✅ cold path | opt-in | default off |
| **meme** | ❌ | ✅ default on buy | default off |

Meme ATAs are not pre-created: a failed buy rolls back and leaves no empty rent-paying ATA.

```rust
warm_ata_cache(&payer, &[(stock_mint, token_program)]);
```

### Cold path — reusable ATAs only

```rust
let prep = client.prepare_buy_atas(&market, BuyWith::Sol);
client.create_wsol_ata();
client.create_quote_ata(&market);
```

### Hot path — buy (creates meme ATA by default)

```rust
let ixs = client.buy_with_sol(amount, &market)?.into_instructions();
```

### Explicit create / close in the same tx

```rust
client.buy_with_opts(
    amount,
    &market,
    TradeOpts::default()
        .buy_with_sol()
        .create_wsol(true)
        .create_quote(true)
        .close_wsol(true),
)?;
```

| Policy | meme | WSOL/quote | close |
|--------|------|------------|-------|
| `AtaPolicy::default()` | ❌ | ❌ | ❌ |
| `buy_with_*` | ✅ | ❌ | ❌ |
| `sell_to_sol` | ❌ | ❌ | WSOL ✅ (unwrap) |
| `AtaPolicy::create_all_in_trade()` | ✅ | ✅ | ❌ |

`sell_to_sol` defaults to `.close_wsol(true)`; use `sell_to_wsol` to keep WSOL.

### Quote alignment (sol-trade-sdk)

| Protocol | Notes |
|----------|-------|
| LaunchLab | `virtual_base - real_base`; Token-2022 transfer fee; graduate clamp on `total_base_sell` |
| CPMM | trade + creator fee (ceil combined); transfer fee; `creator_fee_on` |
| PumpFun | 95(+30) bps; buy fee **on top**; cashback sell includes `user_volume_accumulator` |

When filling `CpmmPool`, `base_reserve` / `quote_reserve` must already exclude protocol/fund/creator fees sitting in vaults.

### On-chain fee integrity

- `amount_in` = **fee + swap spend** (same `fee_source`)
- Non-PumpFun `buy_with_sol`: wrap full `amount_in` to WSOL, then fee + swap from WSOL
- PumpFun: fee + spend from native SOL; on-chain asserts `fee_source` delta ≥ `amount_in`
- SPL fee checks recipient ATA owner/mint; `fee_program` must be Token or Token-2022

### Minimal hot path

```rust
// cold: prepare_buy_atas (WSOL + stock + fee recipient; no meme)
let ixs = client.buy_with_sol(amount, &market)?.into_instructions();
let ixs = client.sell_to_sol(amount, &market)?.into_instructions();
```

## 📁 Project Structure

```text
sol-trade-router-sdk/
├── programs/
│   └── sol-trade-router/   # On-chain Pinocchio program
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs
│           ├── state.rs
│           ├── error.rs
│           └── instructions/
│               ├── initialize.rs
│               ├── update_config.rs
│               └── route.rs
├── crates/
│   └── sdk/                # Client SDK (sol-trade-router-sdk)
│       ├── Cargo.toml
│       └── src/
│           ├── trade.rs
│           ├── legs.rs             # DEX swap legs (all DexTypes)
│           ├── parser.rs           # DexEvent → Market (sol-parser-sdk)
│           ├── market.rs / quote.rs / ata.rs / pool_guard.rs / ...
│           └── ...
├── scripts/
│   ├── check.sh
│   └── build-program.sh
├── keys/
│   └── README.md           # local keypair docs (*.json not committed)
├── LICENSE
├── Cargo.toml
├── README.md
└── README_CN.md
```

## 🔨 Build

Requires a sibling checkout of [`sol-parser-sdk`](https://github.com/0xfnzero/sol-parser-sdk) at `../Solana-SDK-Projects/sol-parser-sdk` (same layout as [sol-trade-sdk](https://github.com/0xfnzero/sol-trade-sdk)).

```bash
# Host compile check (SDK + program)
./scripts/check.sh
# or
cargo check -p sol-trade-router-sdk
cargo check -p sol-trade-router

# Offline unit tests (no RPC)
cargo test -p sol-trade-router-sdk --lib offline_ -- --nocapture

# Mainnet simulateTransaction suite (creates ephemeral wallets, virtually funds them)
RUN_MAINNET_SIM=1 cargo test -p sol-trade-router-sdk --lib mainnet_sim -- --nocapture --test-threads=1
# optional: SOLANA_RPC_URL=https://...

# On-chain SBF build (requires Solana platform-tools / cargo-build-sbf)
./scripts/build-program.sh
```

## 📄 License

MIT License

## 💬 Contact

- Official Website: https://fnzero.dev/
- Project Repository: https://github.com/0xfnzero/sol-trade-router-sdk
- Telegram Group: https://t.me/fnzero_group
- Discord: https://discord.gg/vuazbGkqQE

## ⚠️ Important Notes

1. Test thoroughly before using on mainnet
2. Never commit `keys/*.json` deploy keypairs
3. Call `initialize` promptly after deploy so others cannot claim config authority
4. When rotating Program ID, update both `declare_id!` and SDK `PROGRAM_ID`
5. Comply with relevant laws and regulations
