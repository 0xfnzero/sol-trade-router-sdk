# sol-trade-router-sdk 示例

[English](README.md)

仓库根目录：`cargo run -p <name>`。

超低延迟布局（对齐 sol-trade-sdk）：

1. 订阅前预热 client + **durable nonce 池**（或 blockhash 回退）（`examples/common`）
2. 热路径：`take_tx_clock()` → 映射事件 → buy/sell（无 RPC）

| Package | 数据源 | 行为 |
|---------|--------|------|
| `grpc_event_listen` | Yellowstone gRPC | 仅监听 + `market_from_dex_event` |
| `pumpfun_sniper_trading` | Yellowstone gRPC | 创建者首买狙击 |
| `pumpfun_copy_trading` | Yellowstone gRPC | 跟单首笔 PumpFun |
| `pumpfun_shred_sniper` | Jito ShredStream | 创建者首买狙击 |

## 生产清单

见 [docs/LOW_LATENCY_BOTS_CN.md](../docs/LOW_LATENCY_BOTS_CN.md) 与 [docs/NONCE_CACHE_CN.md](../docs/NONCE_CACHE_CN.md)。

```bash
export PRIVATE_KEY=...
export RPC_URL=https://your-rpc.example
export GRPC_ENDPOINT=https://your-yellowstone.example
# 多 SWQoS / 生产低延迟推荐：
export NONCE_ACCOUNT=<nonce_pubkey>[,<nonce2>,...]
# 可选: WAIT_TX_CONFIRMED=1
# 可选: SENDER_CORES=0
cargo run -p pumpfun_sniper_trading
```

## 安全

- 狙击 / 跟单 / shred 在设置 `PRIVATE_KEY` 后会提交**真实主网交易**。
- 建议私有 RPC + 带鉴权 gRPC。
- Shred 字段不如 gRPC 完整。
- 不要提交私钥。
