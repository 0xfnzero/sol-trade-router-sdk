# Durable Nonce 指南（Router）

多 SWQoS / MEV 低延迟 bot **优先用 durable nonce**，而不是 recent blockhash。Router 的 `TradingClient` 已支持 `TradeBuyParams.durable_nonce` / `TradeSellParams.durable_nonce`（类型与 sol-trade-sdk 一致）。

## 为什么优先 nonce？

| | Durable nonce | Recent blockhash |
|--|---------------|------------------|
| 多通道同一笔交易 | 必需 — 各 SWQoS 共用同一交易身份 | 易过期 / 不一致 |
| 热路径取用 | 订阅前预取池 | 后台缓存仍受 ~150 slot 限制 |
| 有效期 | 直到 Advance | 约 60–90 秒 |

Nonce 延长的是**交易**有效期，不是**报价**有效期 — 池子状态仍要从事件刷新。

## SDK API（已具备）

```rust
use sol_trade_router_sdk::{
    fetch_nonce_info, BuyAmount, DexType, GasFeeStrategy, SimpleBuyParams, TradeTokenType,
};

let nonce = fetch_nonce_info(client.get_rpc(), nonce_account)
    .await
    .expect("nonce account");
let gas = GasFeeStrategy::new();

let params = SimpleBuyParams::with_durable_nonce(
    DexType::PumpFun,
    TradeTokenType::SOL,
    mint,
    BuyAmount::ExactInput(buy_lamports),
    extension,
    nonce,
    gas,
)
.grpc_recv_us(event_us)
.wait_tx_confirmed(false);

client.buy_simple(params).await?;
```

## 示例预热

配置一个或多个 nonce 账户（逗号分隔，连续交易用池）：

```bash
export NONCE_ACCOUNT=<nonce_pubkey>[,<nonce2>,...]
export PRIVATE_KEY=...
export RPC_URL=...
cargo run -p pumpfun_sniper_trading
```

`examples/common::warm_router_client` 会：

1. 预取全部 nonce 账户进池
2. 每次 buy/sell 热路径 `take_tx_clock()` **无 RPC** 弹出一个
3. 后台刷新该账户供下一笔使用

未设置 `NONCE_ACCOUNT` 时回退到 blockhash 缓存并打印警告。

## 流程

1. 为 payer 创建 nonce 账户：https://solana.com/developers/guides/advanced/introduction-to-durable-nonces
2. 冷路径：`NoncePool::warm` / `warm_router_client`
3. 热路径：`take_tx_clock()` → `durable_nonce: Some(...)`，`recent_blockhash: None`
4. 成交后池子刷新该账户（等同再次 `fetch_nonce_info`）

同一笔交易在所有 SWQoS 上复用**同一个** nonce；连续多笔交易用**账户池**。

另见 [LOW_LATENCY_BOTS_CN.md](./LOW_LATENCY_BOTS_CN.md) 与 sol-trade-sdk `docs/NONCE_CACHE_CN.md`。
