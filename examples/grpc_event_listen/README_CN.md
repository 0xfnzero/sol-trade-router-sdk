# gRPC 事件监听（不发交易）

[English](README.md)

仅监听 Yellowstone gRPC：打印 DEX 事件，并尝试 `market_from_dex_event` — **无钱包、不提交交易**。

```bash
export GRPC_ENDPOINT=https://your-yellowstone.example
# 可选: PROTOCOLS=pumpfun,pumpswap,cpmm
cargo run -p grpc_event_listen
```

`PROTOCOLS` 支持：`pumpfun`、`pumpswap`、`cpmm`、`amm_v4`、`clmm`、`whirlpool`、`dlmm`、`damm_v2`、`launchlab`、`stonkfun`。
