# gRPC Event Listen (no submit)

[中文](README_CN.md)

Listen-only Yellowstone gRPC demo. Prints DEX events and tries `market_from_dex_event` — **no wallet, no transaction**.

```bash
export GRPC_ENDPOINT=https://your-yellowstone.example
# optional: PROTOCOLS=pumpfun,pumpswap,cpmm
cargo run -p grpc_event_listen
```

Supported `PROTOCOLS` tokens: `pumpfun`, `pumpswap`, `cpmm`, `amm_v4`, `clmm`, `whirlpool`, `dlmm`, `damm_v2`, `launchlab`, `stonkfun`.
