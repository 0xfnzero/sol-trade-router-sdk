# sol-trade-router-sdk Examples

[中文](README_CN.md)

Run from the repository root: `cargo run -p <name>`.

Ultra-low-latency layout (same as sol-trade-sdk):

1. Warm client + **durable nonce pool** (or blockhash fallback) **before** subscribe (`examples/common`)
2. Hot path: `take_tx_clock()` → map event → buy/sell (no RPC)

| Package | Stream | What it does |
|---------|--------|--------------|
| `grpc_event_listen` | Yellowstone gRPC | Listen only + `market_from_dex_event` |
| `pumpfun_sniper_trading` | Yellowstone gRPC | Creator-first buy sniper |
| `pumpfun_copy_trading` | Yellowstone gRPC | Copy first PumpFun trade |
| `pumpfun_shred_sniper` | Jito ShredStream | Creator-first buy sniper |

## Production checklist

See [docs/LOW_LATENCY_BOTS.md](../docs/LOW_LATENCY_BOTS.md) and [docs/NONCE_CACHE.md](../docs/NONCE_CACHE.md).

```bash
export PRIVATE_KEY=...
export RPC_URL=https://your-rpc.example
export GRPC_ENDPOINT=https://your-yellowstone.example
# recommended for multi-SWQoS / production latency:
export NONCE_ACCOUNT=<nonce_pubkey>[,<nonce2>,...]
# optional: WAIT_TX_CONFIRMED=1
# optional: SENDER_CORES=0
cargo run -p pumpfun_sniper_trading
```

## Safety

- Sniper / copy / shred submit **real mainnet transactions** when `PRIVATE_KEY` is set.
- Prefer private RPC + authenticated gRPC.
- Shred fields are incomplete vs gRPC — validate before production sniping.
- Never commit private keys.
