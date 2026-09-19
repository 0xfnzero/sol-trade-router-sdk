# PumpFun 狙击（Router）

[English](README.md)

Yellowstone gRPC → Route CPI，**低延迟布局**（订阅前预热）。

```bash
export PRIVATE_KEY=...
export RPC_URL=https://your-rpc.example
export GRPC_ENDPOINT=https://your-yellowstone.example
cargo run -p pumpfun_sniper_trading
```

见 [docs/LOW_LATENCY_BOTS_CN.md](../../docs/LOW_LATENCY_BOTS_CN.md)。默认 `wait_tx_confirmed=false`；演示确认可设 `WAIT_TX_CONFIRMED=1`。
