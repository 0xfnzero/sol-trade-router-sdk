# PumpFun ShredStream 狙击（Router）

[English](README.md)

低延迟 ShredStream 狙击 → Route CPI。Shred 字段不全时若走 RPC 回退，则不再是纯低延迟路径。

```bash
export PRIVATE_KEY=...
export RPC_URL=https://your-rpc.example
export SHRED_ENDPOINT=http://127.0.0.1:10800
cargo run -p pumpfun_shred_sniper
```

见 [docs/LOW_LATENCY_BOTS_CN.md](../../docs/LOW_LATENCY_BOTS_CN.md)。
