# Low-Latency Bot Integration Checklist (Router)

Before subscription, initialize and warm `TradingClient` (Route CPI), RPC and SWQoS clients, a background blockhash cache or durable nonce pool, known ATAs, and ALTs. Restore signature/instruction deduplication and position state before accepting events.

The event hot path should be limited to:

```text
filter -> deduplicate -> reject stale event -> map post-trade state -> Trade*/Simple*Params -> sign -> submit (Route CPI)
```

Do **not** initialize clients, synchronously fetch a blockhash, query balances, or search for pools in this path. An RPC fallback is valid for incomplete shred data but is no longer a purely low-latency path.

This crate mirrors [sol-trade-sdk](https://github.com/0xfnzero/sol-trade-sdk) low-latency rules; the only execution difference is Pinocchio **Route CPI** instead of direct DEX instructions.

## Cold-path warm (before subscribe)

| Step | API |
|------|-----|
| Client + SWQoS + clock + `fast_init` | `TradingClient::new` / `from_infrastructure` |
| Background WSOL ATA | `TradeConfig::create_wsol_ata_on_startup(true)` (default) |
| Dedicated sender threads | `with_dedicated_sender_threads(Some(...))` |
| Blockhash / **durable nonce** | Prefer `NONCE_ACCOUNT` pool ([NONCE_CACHE.md](./NONCE_CACHE.md)); blockhash cache only as fallback |
| Known ATAs / ALT | `prepare_buy_atas` / `warm_ata_cache` / ALT accounts |
| Gas | pre-built `GasFeeStrategy` |
| Risk | `with_risk_gate` with **immutable local snapshots** (no RPC inside gate) |

Reference warm helper: `examples/common` (`warm_router_client` + `take_tx_clock`).

## Hot path

- Prefer **`durable_nonce`** from a prefetched pool (multi-SWQoS); else cached `recent_blockhash`
- Map `DexEvent` → `PumpFunParams::from_dev_trade` / `from_trade` / `market_from_dex_event`
- Set `grpc_recv_us` from event metadata for end-to-end timing
- Prefer `wait_tx_confirmed=false`; monitor signatures externally
- Buy: `create_input_token_ata=false` when WSOL is pre-warmed; meme ATA may still be created in-tx for unknown mints
- Sell: **no** `from_mint_by_rpc` on the hot path — use event / local reserves; refresh delayed sells from newer events
- Sniper fill priority: `use_exact_sol_amount: Some(false)` (max-input style)

## Submit and confirmation latency

When `log_enabled`, the client prints `[SDK] Buy/Sell` timing (`build_instructions`, `before_submit`, per-channel `submit_done`, optional `confirmed`). Semantics match sol-trade-sdk: `start_to_submit` is from `grpc_recv_us` (or local now) through submit.

| Symptom | Action |
|---------|--------|
| `confirm` slow | `wait_tx_confirmed=false` + external monitor |
| `submit` slow | paid RPC / SWQoS; raise CU price / tip |
| Example slower than stream bot | you still fetch pool/balance on the hot path — move to warm/refresher |

## Trade intent

Same as sol-trade-sdk: `BuyAmount::ExactInput` / `WithMaxInput` / `ExactOutput` via `SimpleBuyParams`, or low-level `TradeBuyParams` flags. Never use `min_out = 0` as routine error handling.

Use post-trade event reserves. Preserve PumpFun quote mint, creator/vault, token program, cashback, and mayhem. Durable nonce extends transaction validity, **not** quote validity.

**Durable nonce (production default for multi-SWQoS):** [NONCE_CACHE.md](./NONCE_CACHE.md)

## Reference examples

| Package | Stream |
|---------|--------|
| `pumpfun_sniper_trading` | Yellowstone gRPC |
| `pumpfun_copy_trading` | Yellowstone gRPC |
| `pumpfun_shred_sniper` | Jito ShredStream |
| `grpc_event_listen` | listen-only (no submit) |

See also sol-trade-sdk `docs/LOW_LATENCY_BOTS.md`.
