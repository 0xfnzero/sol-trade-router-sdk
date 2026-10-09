<div align="center">
    <h1>🔀 Sol Trade Router SDK</h1>
    <h3><em>Pinocchio 链上多跳 Router + 超低延迟客户端 SDK</em></h3>
</div>

<p align="center">
    <strong>交易能力对齐 <a href="https://github.com/0xfnzero/sol-trade-sdk">sol-trade-sdk</a>（SWQoS、durable nonce、SimpleBuy/Sell、ViaSol），但每笔成交经本仓库 Pinocchio <code>Route</code> CPI（平台费 + 多跳）。池子快照来自 <a href="https://github.com/0xfnzero/sol-parser-sdk">sol-parser-sdk</a> 事件 / 本地缓存：<em>热路径无 RPC</em>。</strong>
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
- [📚 文档](#-文档)
- [📦 安装](#-安装)
- [🆚 与 sol-trade-sdk 的差异](#-与-sol-trade-sdk-的差异)
- [🛠️ 如何使用 SDK](#️-如何使用-sdk)
- [🚀 部署 Router 合约](#-部署-router-合约)
- [📚 示例](#-示例)
- [⚡ 低延迟](#-低延迟)
- [📦 Workspace 与市场](#-workspace-与市场)
- [📁 项目结构](#-项目结构)
- [🔨 构建与测试](#-构建与测试)
- [📄 许可证](#-许可证)
- [💬 联系方式](#-联系方式)
- [⚠️ 重要注意事项](#️-重要注意事项)

---

## ✨ 项目特性

1. **能力对齐 sol-trade-sdk，执行走 Route CPI** — `TradingClient`、SimpleBuy/Sell、SWQoS、durable nonce、ALT、middleware、RiskGate、exact-out、ViaSol
2. **链上多跳 Router** — Pinocchio 先扣平台费，再 CPI 到各 DEX leg
3. **超低延迟** — 订阅前预热；优先 **durable nonce**；热路径禁止取 blockhash / 查余额 / 搜池
4. **DexType 全覆盖** — PumpFun、PumpSwap、LaunchLab/StonkFun/Bonk、Raydium CPMM / AMM V4 / CLMM、Orca Whirlpool、Meteora DLMM / DAMM V2
5. **热路径零 RPC 池快照** — `market_from_dex_event` / `to_routed_market(DexParamEnum)`
6. **ATA 策略** — WSOL / quote 冷路径准备；meme ATA 买入同笔创建
7. **花费 / 产出完整性** — exact-in 总花费必须等于 `amount_in`；exact-out 不超过预算；校验目标 mint / 原生 SOL 与最低到账量
8. **Pool guard** — 可选 PDA / 白名单 / `stonk_strict`

## 📚 文档

| 指南 | 用途 |
|------|------|
| [低延迟 Bot 接入](docs/LOW_LATENCY_BOTS_CN.md) | 冷/热路径、RiskGate、提交计时 |
| [动态两跳买卖](docs/DYNAMIC_TWO_HOP_CN.md) | 首跳实际到账全额作为第二跳输入；动态末跳支持 LaunchLab 买入及现有 Raydium/Meteora/Orca 精确输入池 |
| [动态三跳买卖](docs/DYNAMIC_THREE_HOP_CN.md) | WSOL ↔ USDC ↔ quote ↔ LaunchLab；Raydium/Meteora/Orca 精确输入池可用作动态中间跳或末跳 |
| [Durable Nonce](docs/NONCE_CACHE_CN.md) | 多 SWQoS / MEV 推荐时钟 |
| [examples/README_CN.md](examples/README_CN.md) | gRPC / Shred 狙击与跟单模板 |
| [keys/README.md](keys/README.md) | 本地部署 keypair（勿提交） |

相关：[sol-trade-sdk 文档](https://github.com/0xfnzero/sol-trade-sdk)（Trading Parameters、Gas Fee、ALT、SWQoS）同样适用，仅成交指令程序不同。

## 📦 安装

客户端已准备为 crates.io **0.2.0** 版本。正式发布前可使用 `main` 分支的 Git 依赖：

```toml
[dependencies]
sol-trade-router-sdk = { git = "https://github.com/0xfnzero/sol-trade-router-sdk", branch = "main", package = "sol-trade-router-sdk" }
# 正式发布后：sol-trade-router-sdk = "=0.2.0"
# 仅直接导入解析器类型的订阅程序需要：
sol-parser-sdk = "=0.7.12"
```

依赖使用已发布的 **sol-trade-sdk 6.0.0** 和 **sol-parser-sdk 0.7.12**，不再需要使用方添加 Git 补丁或本地源码路径。请自行部署本仓库配套的 Router 合约，并通过 `with_program_id(your_id)` 配置地址；示例要求设置 `ROUTER_PROGRAM_ID`。历史默认地址不支持修复后的指令格式，合并源码和发布 SDK 不会部署或升级合约。

详见[发布验证与自部署兼容性](docs/RELEASE_0.2.0.md)。固定版本的 Yellowstone 依赖仍存在上游 Windows 导入限制，本次发布验证面向 Unix。

一般**不必**额外依赖 `sol-trade-sdk`，交易类型已经从 `sol_trade_router_sdk::*` 导出。

## 🆚 与 sol-trade-sdk 的差异

| | sol-trade-sdk | sol-trade-router-sdk |
|--|---------------|----------------------|
| 客户端 API | `TradingClient` / SimpleBuy\|Sell | 同名同参 |
| 成交指令 | 直调 DEX | Pinocchio **Route** CPI + 平台费 |
| 热路径 | durable nonce / blockhash，无 RPC | 相同 |
| 事件流 | sol-parser-sdk | 相同 |
| 链上程序 | 无 | 需部署并 `initialize` 本 Router |

迁移：保留 `TradeBuyParams` / `DexParamEnum` / SWQoS；改用 `sol_trade_router_sdk::TradingClient` + `RouterTradeConfig`（增加可选的 `fee_recipient`、`fee_bps`）。

### 平台手续费（可选 / 可预留）

自部署 Router 时**不必收手续费**：

| 字段 | 作用 | 自部署建议 |
|------|------|------------|
| `fee_bps` | 平台费率（万分比） | **`0` = 不扣费**（链上跳过转账） |
| `fee_recipient` | 收款地址（config 里常驻） | 可填自己的钱包作占位；以后要用再 `update_config` |

- 链上：`fee_bps == 0` 时 `Route` **不转任何平台费**，也不校验收款账户。
- SDK / 示例：未设 `FEE_BPS` 时默认 `0`；未设 `FEE_RECIPIENT` 时默认用 payer。
- 你以后自己部署并想收费：`initialize` 时仍可先 `fee_bps = 0`，之后用 `update_config` 改成例如 `50`（0.50%）并指定收款地址。

## 🛠️ 如何使用 SDK

### 1. 创建 `TradingClient`

```rust
use std::sync::Arc;
use sol_trade_router_sdk::{
    keypair, RouterTradeConfig, SwqosConfig, TradeConfig, TradingClient,
};
use solana_commitment_config::CommitmentConfig;

let payer = Arc::new(keypair::load_keypair_from_env("PRIVATE_KEY")?);
let rpc = std::env::var("RPC_URL")?;
// 自部署默认：不收平台费。收款地址预留为自己即可。
let fee_recipient = payer.pubkey();
let fee_bps: u16 = 0; // 0 = 不扣费；以后收费再改链上 config + 这里

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
.with_dedicated_sender_threads(Some(vec![])); // 可选：专用 SWQoS 发送线程
```

多钱包共享：`TradingInfrastructure::new` → `TradingClient::from_infrastructure(...)`。

### 2. 买入 / 卖出（对齐 sol-trade-sdk）

推荐高层参数（与 trade-sdk 相同）：

```rust
use sol_trade_router_sdk::{
    fetch_nonce_info, AccountPolicy, BuyAmount, DexParamEnum, DexType, GasFeeStrategy,
    SimpleBuyParams, TradeTokenType,
};

// 生产：durable nonce（多 SWQoS）。见 docs/NONCE_CACHE_CN.md
let nonce = fetch_nonce_info(client.get_rpc(), nonce_account).await.unwrap();
let gas = GasFeeStrategy::new();
gas.set_global_fee_strategy(150_000, 150_000, 500_000, 500_000, 0.001, 0.001);

let buy = SimpleBuyParams::with_durable_nonce(
    DexType::PumpFun,
    TradeTokenType::SOL,
    mint,
    BuyAmount::ExactInput(100_000),
    DexParamEnum::PumpFun(params), // 来自事件 / from_trade / from_dev_trade
    nonce,
    gas,
)
.account_policy(AccountPolicy::HotPathMinimal)
.grpc_recv_us(event_recv_us)
.wait_tx_confirmed(false);

client.buy_simple(buy).await?;
```

或仅组 Route 指令（离线）：

```rust
let ixs = client.buy_with_sol(amount, &market)?.into_instructions();
let ixs = client.sell_to_sol(amount, &market)?.into_instructions();
```

| 资产 | 买入 | 卖出 |
|------|------|------|
| 原生 SOL | `buy_with_sol` | `sell_to_sol` |
| WSOL | `buy_with_wsol` | `sell_to_wsol` |
| quote | `buy_with_token` | `sell_to_token` |

复用 `TradeOpts` 时，调用 `sell_to_wsol()` 或 `sell_to_token(...)` 会清除
`sell_to_sol()` 设置的 WSOL 关闭策略。需要自定义关闭行为时，请在选择接收资产后
调用 `close_wsol(...)` 或 `with_ata(...)` 覆盖策略。

### 3. 从事件 / 参数构建市场

```rust
use sol_trade_router_sdk::{market_from_dex_event, to_routed_market};

if let Some(market) = market_from_dex_event(&event) { /* ... */ }

let routed = to_routed_market(&DexParamEnum::PumpFun(params), mint)?;
```

### 4. ATA 策略

| 类型 | 冷路径 | 交易内 | 关闭 |
|------|--------|--------|------|
| WSOL / quote | ✅ `prepare_buy_atas` | 可选 | 默认否 |
| meme | ❌ | ✅ 买入时 | 默认否 |

```rust
client.prepare_buy_atas(&market, BuyWith::Sol);
```

SOL 交易对的 Pump 曲线设置 `use_v2=true` 后，使用 `BuyWith::Wsol` 或
`SellTo::Wsol` 准备对应的 WSOL ATA。原生 SOL 结算仍跳过这些账户；
V1 曲线不支持 WSOL 结算。

`TradeBuyParams` / `TradeSellParams` 中的 `TradeTokenType::Token(WSOL_MINT)`
与 `TradeTokenType::WSOL` 使用相同的 WSOL 账户创建和关闭标记。
原生 SOL 卖出设置 `close_output_token_ata=true` 时，会关闭并解包 WSOL 输出。

Router 交易及余额查询始终使用标准 ATA，覆盖 Token 和 Token-2022；
共享交易配置中的 `use_seed_optimize` 不改变 Router 的账户地址。

高层 Router 客户端尚未实现 PumpFun/PumpSwap 的 `BuyAmount::WithMaxInput`
（`use_exact_sol_amount=false`），请求该模式会明确报错。
可使用精确输入或已支持的固定输出目标；固定输出目标优先。

`TradeBuyParams` / `TradeSellParams` 的 `mint` 必须对应市场选定的目标代币，
`dex_type` 必须匹配 `extension_params`；仅构建指令的接口也会拒绝不匹配的请求。

原生 SOL 买入设置 `close_input_token_ata=true` 时，会在路由后关闭实际使用的 WSOL ATA，
返还租金并将账户内全部剩余 WSOL 转回 SOL。直接使用原生 SOL 的 PumpFun 路径不会关闭 WSOL ATA。

### 5. 管理指令（部署后）

```rust
use sol_trade_router_sdk::{initialize_config, update_config, PROGRAM_ID};

// 自部署：fee_bps=0 不收费；fee_recipient 填自己作预留
let ix = initialize_config(&PROGRAM_ID, &authority, &authority, 0);
// 仅发送一次；authority 签名并支付 config PDA rent

// 以后若要收费：改费率 + 可选新收款地址
let ix = update_config(&PROGRAM_ID, &authority, 50, false, Some(fee_recipient));
```

## 🚀 部署 Router 合约

上线 Route 交易前，必须部署链上程序并调用 `initialize`。仅有客户端 SDK 不够。

### 前置条件

- [Solana CLI](https://docs.solana.com/cli/install)（`solana`、`solana-keygen`）
- `cargo-build-sbf` / Solana platform-tools
- 有足够 SOL 的部署者 keypair

### 步骤 1 — 程序 keypair 与 Program ID

```bash
# 只生成一次。切勿提交 keys/*.json
solana-keygen new --outfile keys/router-keypair.json --no-bip39-passphrase
solana-keygen pubkey keys/router-keypair.json
```

将打印的公钥同步到：

1. `programs/sol-trade-router/src/lib.rs` → `declare_id!("...")`
2. `crates/sdk/src/constants.rs` → `PROGRAM_ID`

详见 [keys/README.md](./keys/README.md)。

### 步骤 2 — 编译 SBF `.so`

```bash
./scripts/build-program.sh
# → target/deploy/sol_trade_router.so（或以脚本打印路径为准）
```

### 步骤 3 — 部署

```bash
solana config set --url https://api.mainnet-beta.solana.com   # 或你的 RPC / devnet
solana program deploy \
  --program-id keys/router-keypair.json \
  target/deploy/sol_trade_router.so
```

确认链上 Program ID 与 SDK `PROGRAM_ID` 一致。

### 步骤 4 — 初始化配置（关键）

谁先调用 `initialize`，谁就是 **config authority**。部署后请立刻执行：

```rust
use sol_trade_router_sdk::{initialize_config, PROGRAM_ID};
// 自部署不收费：fee_bps=0，fee_recipient 用自己的公钥预留即可
// initialize_config(&PROGRAM_ID, &authority_pubkey, &authority_pubkey, 0)
```

- `fee_bps = 0` → 交易不收平台费  
- `fee_recipient` 仍写入 config（预留）；以后收费用 `update_config` 改 `fee_bps` / 收款地址  

### 步骤 5 — Bot 指向该程序

`TradingClient` / `RouterClient` 默认使用 `PROGRAM_ID`。若轮换 ID：

```rust
RouterTradeConfig::new(trade, fee_recipient, 0).with_program_id(your_id)
```

Bot 的 `fee_bps` / `fee_recipient` 应与链上 config **意图一致**（自部署两边都写 `0` + 自己的地址即可）。

### 检查清单

- [ ] 已生成 keypair；`declare_id!` 与 `PROGRAM_ID` 一致
- [ ] `.so` 已编译并部署
- [ ] 已发送 `initialize`（你拥有 config authority）
- [ ] 自部署：`fee_bps = 0`（或不收费）；若收费则 Bot 与链上费率/收款一致
- [ ] 交易 payer 已准备 durable nonce 账户（[NONCE_CACHE_CN.md](docs/NONCE_CACHE_CN.md)）

## 📚 示例

| Package | 数据源 | 行为 |
|---------|--------|------|
| `grpc_event_listen` | Yellowstone gRPC | 监听 + `market_from_dex_event`（不发交易） |
| `pumpfun_sniper_trading` | gRPC | 创建者首买狙击 → Route |
| `pumpfun_copy_trading` | gRPC | 跟单首笔 PumpFun → Route |
| `pumpfun_shred_sniper` | ShredStream | 创建者首买狙击 → Route |

```bash
# 仅监听（安全）
export GRPC_ENDPOINT=https://your-yellowstone.example
cargo run -p grpc_event_listen

# 实盘（真实主网交易）
export PRIVATE_KEY=...
export RPC_URL=https://your-rpc.example
export GRPC_ENDPOINT=https://your-yellowstone.example
export NONCE_ACCOUNT=<nonce_pubkey>[,...]   # 推荐
cargo run -p pumpfun_sniper_trading
```

预热辅助：`examples/common`（`warm_router_client` + `take_tx_clock`）。详见 [examples/README_CN.md](./examples/README_CN.md)。

## ⚡ 低延迟

```text
预热 client + nonce 池 + ATA  →  订阅
热路径：过滤 → 映射事件 → take_tx_clock → buy/sell → 提交
```

- 多 SWQoS 优先 **`NONCE_ACCOUNT`**，不要默认走 blockhash
- 事件回调内禁止建 client / 取 blockhash / 查余额
- 示例默认 `wait_tx_confirmed=false`

→ [docs/LOW_LATENCY_BOTS_CN.md](./docs/LOW_LATENCY_BOTS_CN.md) · [docs/NONCE_CACHE_CN.md](./docs/NONCE_CACHE_CN.md)

## 📦 Workspace 与市场

| 路径 | Crate | 说明 |
|------|-------|------|
| `programs/sol-trade-router` | `sol-trade-router` | 链上扣费 + 多跳 `route` |
| `crates/sdk` | `sol-trade-router-sdk` | 客户端 SDK |

| 市场 | 说明 |
|------|------|
| PumpFun | Bonding curve V1 / V2 |
| PumpSwap | 毕业 `pAMM` |
| LaunchLab / Bonk / StonkFun | 曲线 + 毕业 CPMM / ViaSol |
| Raydium CPMM / AMM V4 / CLMM | |
| Orca Whirlpool | |
| Meteora DLMM / DAMM V2 | |

## 📁 项目结构

```text
sol-trade-router-sdk/
├── programs/sol-trade-router/     # Pinocchio 链上程序
├── crates/sdk/                    # sol-trade-router-sdk 客户端
├── docs/                          # 低延迟 + Nonce 指南
├── examples/                      # gRPC / Shred bot + 公共预热
├── scripts/check.sh
├── scripts/build-program.sh
├── keys/                          # 部署 keypair（不入库）
├── README.md
└── README_CN.md
```

## 🔨 构建与测试

```bash
./scripts/check.sh
cargo check -p sol-trade-router-sdk
cargo test -p sol-trade-router-sdk --lib offline_ -- --nocapture

# 可选主网 simulate（临时钱包；Router 未部署时 Soft）
RUN_MAINNET_SIM=1 cargo test -p sol-trade-router-sdk --lib mainnet_ -- --nocapture --test-threads=1

./scripts/build-program.sh
```

## 📄 许可证

MIT License

## 💬 联系方式

- 官网：https://fnzero.dev/
- 仓库：https://github.com/0xfnzero/sol-trade-router-sdk
- Telegram：https://t.me/fnzero_group
- Discord：https://discord.gg/vuazbGkqQE

## ⚠️ 重要注意事项

1. 主网大资金前先在 devnet / simulate 验证
2. 切勿提交 `keys/*.json`
3. 部署后立刻调用 `initialize`
4. 保持 `declare_id!` 与 SDK `PROGRAM_ID` 同步
5. Durable nonce ≠ 报价有效期 — 池状态仍要从事件刷新
6. 请遵守当地法律法规
