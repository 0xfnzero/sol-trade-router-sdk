# Durable Nonce Guide (Router)

For **multi-SWQoS / MEV** low-latency bots, prefer **durable nonce** over a recent blockhash. The router `TradingClient` already accepts `TradeBuyParams.durable_nonce` / `TradeSellParams.durable_nonce` (same types as sol-trade-sdk).

## Why nonce first?

| | Durable nonce | Recent blockhash |
|--|---------------|------------------|
| Multi-relay same tx | Required — one identity across SWQoS lanes | Easy to diverge / expire |
| Hot-path fetch | Prefetch pool before subscribe | Background cache still races ~150 slots |
| Validity | Until advanced | ~60–90s |

Nonce extends **transaction** validity, not **quote** validity — still refresh pool state from events.

## API (already in SDK)

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

## Example warm path

Set one or more nonce accounts (comma-separated pool for consecutive trades):

```bash
export NONCE_ACCOUNT=<nonce_pubkey>[,<nonce2>,...]
export PRIVATE_KEY=...
export RPC_URL=...
cargo run -p pumpfun_sniper_trading
```

`examples/common::warm_router_client` will:

1. Prefetch all nonce accounts into a pool
2. On each buy/sell hot path, `take_tx_clock()` pops one **without RPC**
3. Refresh that account in the background for the next trade

Without `NONCE_ACCOUNT`, examples fall back to a blockhash cache and print a warning.

## Flow

1. Create nonce account(s) for the payer: https://solana.com/developers/guides/advanced/introduction-to-durable-nonces
2. Cold: `NoncePool::warm` / `warm_router_client`
3. Hot: `take_tx_clock()` → `durable_nonce: Some(...)`, `recent_blockhash: None`
4. After landing, pool refreshes that account (same as `fetch_nonce_info` again)

Same nonce value is reused across all SWQoS clients for **one** trade; use a **pool** of accounts when you fire many trades back-to-back.

See also [LOW_LATENCY_BOTS.md](./LOW_LATENCY_BOTS.md) and sol-trade-sdk `docs/NONCE_CACHE.md`.
