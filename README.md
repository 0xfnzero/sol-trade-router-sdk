# sol-trade-router

Pinocchio 链上多跳 Router + 热路径零 RPC 的客户端 SDK。

| 市场 | 类型 | 说明 |
|------|------|------|
| StonkFun / LaunchLab | **内盘** | bonding curve，任意 quote（含 CARDS） |
| StonkFun 毕业池 | **外盘** | Raydium CPMM |
| PumpFun | **内盘** | SOL bonding curve（单跳） |

## 命名规范

| 资产 | 买入 | 卖出 |
|------|------|------|
| 原生 SOL | `buy_with_sol` | `sell_to_sol` |
| WSOL | `buy_with_wsol` | `sell_to_wsol` |
| stock/quote | `buy_with_token` | `sell_to_token` |
| 自定义 | `buy_with_opts` | `sell_with_opts` |

## ATA 策略

| 类型 | 提前单独创建 | 交易同笔创建 | 关闭 |
|------|-------------|-------------|------|
| **WSOL / 股票(quote)** | ✅ 推荐冷路径 | 默认否（可 opt-in） | 默认否 |
| **meme** | ❌ 不要提前建 | ✅ 买入默认同笔创建 | 默认否 |

Meme 不提前建：买失败整笔回滚，不会留下空 ATA 浪费 rent。WSOL / stock 可复用，适合冷路径准备。

启动时可预热 ATA 缓存（避免热路径反复 `find_program_address`）：

```rust
warm_ata_cache(&payer, &[(stock_mint, token_program)]);
```

### 报价对齐（sol-trade-sdk）

| 协议 | 要点 |
|------|------|
| LaunchLab | `virtual_base - real_base`；Token-2022 transfer fee；毕业 `total_base_sell` 夹取 |
| CPMM | trade + creator fee（合并向上取整）；transfer fee；`creator_fee_on` |
| PumpFun | 费率 95(+30) bps；买入费用**外加**；cashback sell 含 `user_volume_accumulator` |

填充 `CpmmPool` 时：`base_reserve` / `quote_reserve` 需已扣除 vault 内 protocol/fund/creator fees。

### 1) 冷路径：只准备可复用 ATA

```rust
// 不含 meme
let prep = client.prepare_buy_atas(&market, BuyWith::Sol);
// 或细粒度：
client.create_wsol_ata();
client.create_quote_ata(&market); // stock 对需要时
// meme 需要时用 create_meme_ata，但买入场景请走交易同笔 create_meme
```

### 2) 热路径买入（默认创建 meme ATA）

```rust
// buy_with_* 默认 create_meme = true；WSOL/stock 假设已 prepare
let ixs = client.buy_with_sol(amount, &market)?.into_instructions();
```

### 3) 交易里顺带创建 / 关闭（显式）

```rust
client.buy_with_opts(
    amount,
    &market,
    TradeOpts::default()
        .buy_with_sol()           // 已打开 create_meme
        .create_wsol(true)        // 同笔建 WSOL（未 prepare 时）
        .create_quote(true)       // 同笔建 stock
        .close_wsol(true),        // 卖回 SOL 时 unwrap
)?;

// 或一次全开（无冷路径准备）
.with_ata(AtaPolicy::create_all_in_trade())
```

| 策略 | meme | WSOL/quote | close |
|------|------|------------|-------|
| `AtaPolicy::default()` | ❌ | ❌ | ❌ |
| `buy_with_*` | ✅ | ❌ | ❌ |
| `sell_to_sol` | ❌ | ❌ | WSOL ✅（unwrap） |
| `AtaPolicy::create_all_in_trade()` | ✅ | ✅ | ❌ |
| `.create_wsol/quote/meme(true).close_*(true)` | 自选 | 自选 | 自选 |

`sell_to_sol` 默认 `.close_wsol(true)`，收到原生 SOL；若要保留 WSOL 用 `sell_to_wsol`。

## 手续费完整性（链上）

- `amount_in` = **手续费 + 实际 swap 花费**（同一 `fee_source`）
- 非 PumpFun 的 `buy_with_sol`：先 wrap 全部 `amount_in` 到 WSOL，再从 WSOL 扣费并 swap
- PumpFun：从原生 SOL 扣费 + 花费，链上校验 `fee_source` 余额减少 ≥ `amount_in`
- SPL 手续费校验收款 ATA 的 owner / mint，且 `fee_program` 必须是 Token / Token-2022

## 最简热路径

```rust
// 冷：prepare_buy_atas（WSOL + stock + fee recipient，不含 meme）
// 热：
let ixs = client.buy_with_sol(amount, &market)?.into_instructions();
let ixs = client.sell_to_sol(amount, &market)?.into_instructions(); // 默认 unwrap WSOL
```

## 安全注意

- `keys/` 含程序 keypair，**勿提交**（已在 `.gitignore`）
- 部署后请尽快 `initialize`，避免他人抢占 authority

## 仓库 / 构建

```
programs/router/   # Pinocchio
crates/sdk/        # sol-trade-router-sdk
```

```bash
cargo check -p sol-trade-router-sdk
cargo build-sbf --manifest-path programs/router/Cargo.toml --features bpf-entrypoint
```
