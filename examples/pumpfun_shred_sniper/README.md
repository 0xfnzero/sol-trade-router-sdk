# PumpFun ShredStream Sniper (Router)

[中文](README_CN.md)

Low-latency ShredStream sniper → Route CPI. Incomplete shred fields may require RPC fallback (not pure low-latency).

```bash
export PRIVATE_KEY=...
export RPC_URL=https://your-rpc.example
export SHRED_ENDPOINT=http://127.0.0.1:10800
cargo run -p pumpfun_shred_sniper
```

See [docs/LOW_LATENCY_BOTS.md](../../docs/LOW_LATENCY_BOTS.md).
