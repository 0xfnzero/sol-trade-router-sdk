# PumpFun Sniper Trading (Router)

[中文](README_CN.md)

Yellowstone gRPC → Route CPI, **low-latency layout** (warm before subscribe). Prefer durable nonce:

```bash
export NONCE_ACCOUNT=<nonce_pubkey>[,...]
export PRIVATE_KEY=...
export RPC_URL=https://your-rpc.example
export GRPC_ENDPOINT=https://your-yellowstone.example
cargo run -p pumpfun_sniper_trading
```

See [docs/LOW_LATENCY_BOTS.md](../../docs/LOW_LATENCY_BOTS.md) and [docs/NONCE_CACHE.md](../../docs/NONCE_CACHE.md).
