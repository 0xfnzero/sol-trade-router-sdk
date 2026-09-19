# 低延迟 Bot 接入清单（Router）

订阅前完成 `TradingClient`（Route CPI）、RPC / SWQoS、后台 blockhash（或 durable nonce）、已知 ATA / ALT 的初始化与预热；恢复签名/指令去重与持仓状态后再处理事件。

事件热路径应限制为：

```text
过滤 → 去重 → 丢弃过期事件 → 映射成交后状态 → Trade*/Simple*Params → 签名 → 提交（Route CPI）
```

热路径中**禁止**：初始化 client、同步取 blockhash、查余额、搜池。Shred 字段不全时允许 RPC 回退，但不再是纯低延迟路径。

本仓库与 [sol-trade-sdk](https://github.com/0xfnzero/sol-trade-sdk) 低延迟规则对齐；唯一差异是成交走 Pinocchio **Route CPI**，而非直调 DEX。

## 冷路径预热（订阅前）

| 步骤 | API |
|------|-----|
| Client + SWQoS + 时钟 + `fast_init` | `TradingClient::new` / `from_infrastructure` |
| 后台创建 WSOL ATA | `TradeConfig::create_wsol_ata_on_startup(true)`（默认） |
| 专用发送线程 | `with_dedicated_sender_threads(Some(...))` |
| Blockhash / **durable nonce** | 优先 `NONCE_ACCOUNT` 池（[NONCE_CACHE_CN.md](./NONCE_CACHE_CN.md)）；无 nonce 时才用 blockhash 缓存 |
| 已知 ATA / ALT | `prepare_buy_atas` / `warm_ata_cache` / ALT |
| Gas | 预构建 `GasFeeStrategy` |
| 风控 | `with_risk_gate` + **本地不可变快照**（gate 内禁 RPC） |

参考：`examples/common`（`warm_router_client` + `take_tx_clock`）。

## 热路径

- 优先用预取池里的 **`durable_nonce`**（多 SWQoS）；否则才用缓存的 `recent_blockhash`
- `DexEvent` → `PumpFunParams::from_dev_trade` / `from_trade` / `market_from_dex_event`
- 填入 `grpc_recv_us` 做端到端计时
- 优先 `wait_tx_confirmed=false`，外部监控签名
- 买入：WSOL 已预热则 `create_input_token_ata=false`；未知 mint 可同笔建 meme ATA
- 卖出：热路径**禁止** `from_mint_by_rpc`，用事件/本地储备；延迟卖从更新事件刷新
- 狙击填单优先：`use_exact_sol_amount: Some(false)`

## 提交与确认延迟

`log_enabled` 时打印 `[SDK] Buy/Sell` 计时（`build_instructions` / `before_submit` / 各通道 `submit_done` / 可选 `confirmed`），语义对齐 sol-trade-sdk：`start_to_submit` 从 `grpc_recv_us`（或本地 now）算到 submit。

| 现象 | 处理 |
|------|------|
| `confirm` 慢 | `wait_tx_confirmed=false` + 外部监控 |
| `submit` 慢 | 付费 RPC / SWQoS；提高 CU / tip |
| 示例比流式 bot 慢 | 热路径仍在拉池/查余额 — 挪到预热/后台刷新 |

## 交易意图

与 sol-trade-sdk 相同：`BuyAmount::ExactInput` / `WithMaxInput` / `ExactOutput`（`SimpleBuyParams`）或底层 `TradeBuyParams`。不要把 `min_out = 0` 当作常规错误处理。

使用成交后事件储备量；保留 PumpFun quote / creator / vault / token_program / cashback / mayhem。Durable nonce 延长的是交易有效期，**不是**报价有效期。

**Durable nonce（多 SWQoS 生产默认）：** [NONCE_CACHE_CN.md](./NONCE_CACHE_CN.md)

## 参考示例

| Package | 数据源 |
|---------|--------|
| `pumpfun_sniper_trading` | Yellowstone gRPC |
| `pumpfun_copy_trading` | Yellowstone gRPC |
| `pumpfun_shred_sniper` | Jito ShredStream |
| `grpc_event_listen` | 仅监听 |

另见 sol-trade-sdk `docs/LOW_LATENCY_BOTS_CN.md`。
