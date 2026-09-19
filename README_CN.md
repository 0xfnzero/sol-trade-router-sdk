<div align="center">
    <h1>🔀 Sol Trade Router SDK</h1>
    <h3><em>Pinocchio 链上多跳 Router + 热路径零 RPC 客户端 SDK</em></h3>
</div>

<p align="center">
    <strong>包含 Pinocchio 路由合约与 Rust 客户端 SDK 的 Solana monorepo。支持带平台费的多跳 DEX 交易；池子快照来自 <a href="https://github.com/0xfnzero/sol-parser-sdk">sol-parser-sdk</a> 事件 / 本地缓存，热路径无 RPC。</strong>
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

## 📋 目录

- [✨ 项目特性](#-项目特性)
- [📦 Workspace](#-workspace)
- [🔖 Program ID](#-program-id)
- [🛠️ 使用说明](#️-使用说明)
- [📁 项目结构](#-项目结构)
- [🔨 构建](#-构建)
- [📄 许可证](#-许可证)
- [💬 联系方式](#-联系方式)
- [⚠️ 重要注意事项](#️-重要注意事项)

---

## ✨ 项目特性

1. **链上多跳 Router**：Pinocchio 程序先扣平台费，再 CPI 到各 DEX leg
2. **热路径零 RPC**：`market_from_dex_event` 从 [sol-parser-sdk](https://github.com/0xfnzero/sol-parser-sdk) 事件构建池子快照
3. **与 sol-trade-sdk DexType 对齐**：PumpFun、PumpSwap、LaunchLab/StonkFun/Bonk、Raydium CPMM / AMM V4 / CLMM、Orca Whirlpool、Meteora DLMM / DAMM V2
4. **对称 API**：`buy_with_{sol|wsol|token}` / `sell_to_{sol|wsol|token}`
5. **ATA 策略**：WSOL / stock 冷路径准备；meme ATA 在买入同笔创建
6. **手续费完整性**：链上校验 `fee_source` 花费 ≥ `amount_in`（手续费 + swap）
7. **Pool guard**：可选 PDA / 白名单校验（`market_from_dex_event_checked`）

## 📦 Workspace

| 路径 | Crate | 说明 |
|------|-------|------|
| `programs/sol-trade-router` | `sol-trade-router` | 链上程序：扣费 + 多跳 `route` CPI |
| `crates/sdk` | `sol-trade-router-sdk` | 客户端 SDK：离线组交易指令 |

### 支持市场

与 [sol-trade-sdk](https://github.com/0xfnzero/sol-trade-sdk) 的 `DexType` 覆盖对齐（Router CPI 版本）：

| 市场 | 类型 | 说明 |
|------|------|------|
| PumpFun | 内盘 | SOL / quote bonding curve（V1 / V2） |
| PumpSwap | 外盘 AMM | PumpFun 毕业池（`pAMM`） |
| LaunchLab / Bonk / StonkFun | 内盘 | bonding curve，任意 quote（含 CARDS） |
| StonkFun 毕业 / Raydium CPMM | 外盘 CPMM | Raydium CPMM |
| Raydium AMM V4 | AMM | `SwapBaseInV2` |
| Raydium CLMM | CLMM | `swap_v2` + tick arrays |
| Orca Whirlpool | CLMM | `swap_v2` + tick arrays |
| Meteora DLMM | DLMM | `swap2` + bin arrays |
| Meteora DAMM V2 | Dynamic AMM | `swap2` exact-in |

池子快照由 [`sol-parser-sdk`](https://github.com/0xfnzero/sol-parser-sdk) 的 `DexEvent` 经 `market_from_dex_event` 转换。

## 🔖 Program ID

```text
CMrrMgrEvXW3oo6RtxnneDf5D5TeujfbqveFiKuvqrYg
```

须与程序内 `declare_id!`、SDK `PROGRAM_ID` 一致。本地部署 keypair 放在 `keys/`（不入库），见 [keys/README.md](./keys/README.md)。

## 🛠️ 使用说明

### 命名规范

| 资产 | 买入 | 卖出 |
|------|------|------|
| 原生 SOL | `buy_with_sol` | `sell_to_sol` |
| WSOL | `buy_with_wsol` | `sell_to_wsol` |
| stock/quote | `buy_with_token` | `sell_to_token` |
| 自定义 | `buy_with_opts` | `sell_with_opts` |

### ATA 策略

| 类型 | 提前单独创建 | 交易同笔创建 | 关闭 |
|------|-------------|-------------|------|
| **WSOL / 股票(quote)** | ✅ 推荐冷路径 | 默认否（可 opt-in） | 默认否 |
| **meme** | ❌ 不要提前建 | ✅ 买入默认同笔创建 | 默认否 |

Meme 不提前建：买失败整笔回滚，不会留下空 ATA 浪费 rent。

```rust
warm_ata_cache(&payer, &[(stock_mint, token_program)]);
```

### 1) 冷路径：只准备可复用 ATA

```rust
let prep = client.prepare_buy_atas(&market, BuyWith::Sol);
client.create_wsol_ata();
client.create_quote_ata(&market); // stock 对需要时
```

### 2) 热路径买入（默认创建 meme ATA）

```rust
let ixs = client.buy_with_sol(amount, &market)?.into_instructions();
```

### 3) 交易里顺带创建 / 关闭（显式）

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

| 策略 | meme | WSOL/quote | close |
|------|------|------------|-------|
| `AtaPolicy::default()` | ❌ | ❌ | ❌ |
| `buy_with_*` | ✅ | ❌ | ❌ |
| `sell_to_sol` | ❌ | ❌ | WSOL ✅（unwrap） |
| `AtaPolicy::create_all_in_trade()` | ✅ | ✅ | ❌ |

`sell_to_sol` 默认 `.close_wsol(true)`；若要保留 WSOL 用 `sell_to_wsol`。

### 报价对齐（sol-trade-sdk）

| 协议 | 要点 |
|------|------|
| LaunchLab | `virtual_base - real_base`；Token-2022 transfer fee；毕业 `total_base_sell` 夹取 |
| CPMM | trade + creator fee（合并向上取整）；transfer fee；`creator_fee_on` |
| PumpFun | 费率 95(+30) bps；买入费用**外加**；cashback sell 含 `user_volume_accumulator` |

填充 `CpmmPool` 时：`base_reserve` / `quote_reserve` 需已扣除 vault 内 protocol/fund/creator fees。

### 手续费完整性（链上）

- `amount_in` = **手续费 + 实际 swap 花费**（同一 `fee_source`）
- 非 PumpFun 的 `buy_with_sol`：先 wrap 全部 `amount_in` 到 WSOL，再从 WSOL 扣费并 swap
- PumpFun：从原生 SOL 扣费 + 花费，链上校验 `fee_source` 余额减少 ≥ `amount_in`
- SPL 手续费校验收款 ATA 的 owner / mint，且 `fee_program` 必须是 Token / Token-2022

### 最简热路径

```rust
// 冷：prepare_buy_atas（WSOL + stock + fee recipient，不含 meme）
let ixs = client.buy_with_sol(amount, &market)?.into_instructions();
let ixs = client.sell_to_sol(amount, &market)?.into_instructions();
```

## 📁 项目结构

```text
sol-trade-router-sdk/
├── programs/
│   └── sol-trade-router/   # 链上智能合约（Pinocchio）
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
│   └── sdk/                # 客户端 SDK
│       ├── Cargo.toml
│       └── src/
│           ├── trade.rs
│           ├── legs.rs             # 各 DEX swap leg（全协议）
│           ├── parser.rs           # DexEvent → Market（sol-parser-sdk）
│           ├── market.rs / quote.rs / ata.rs / pool_guard.rs / ...
│           └── ...
├── scripts/
│   ├── check.sh
│   └── build-program.sh
├── keys/
│   └── README.md           # 本地 keypair 说明（*.json 不入库）
├── LICENSE
├── Cargo.toml
├── README.md
└── README_CN.md
```

## 🔨 构建

需要同级目录检出 [`sol-parser-sdk`](https://github.com/0xfnzero/sol-parser-sdk) 到 `../Solana-SDK-Projects/sol-parser-sdk`（与 [sol-trade-sdk](https://github.com/0xfnzero/sol-trade-sdk) 布局一致）。

```bash
# Host 编译检查（SDK + 程序）
./scripts/check.sh
# 或
cargo check -p sol-trade-router-sdk
cargo check -p sol-trade-router

# 离线单元测试（不访问 RPC）
cargo test -p sol-trade-router-sdk --lib offline_ -- --nocapture

# 主网 simulateTransaction 套件（每次自动 Keypair::new 建钱包并虚拟注资）
RUN_MAINNET_SIM=1 cargo test -p sol-trade-router-sdk --lib mainnet_sim -- --nocapture --test-threads=1
# 可选: SOLANA_RPC_URL=https://...

# 链上程序 SBF 构建
./scripts/build-program.sh
```

## 📄 许可证

MIT 许可证

## 💬 联系方式

- 官方网站: https://fnzero.dev/
- 项目仓库: https://github.com/0xfnzero/sol-trade-router-sdk
- Telegram 群组: https://t.me/fnzero_group
- Discord: https://discord.gg/vuazbGkqQE

## ⚠️ 重要注意事项

1. 在主网使用前请充分测试
2. 切勿提交 `keys/*.json` 部署密钥
3. 部署后请尽快调用 `initialize`，避免他人抢占 config authority
4. 更换 Program ID 时同步修改 `declare_id!` 与 SDK `PROGRAM_ID`
5. 遵循相关法律法规
