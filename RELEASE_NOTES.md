# sol-trade-router-sdk v0.5.0

Released November 3, 2026.

## Changes

Use published sol-parser-sdk 0.11.0 and sol-trade-sdk 11.0.1 instead of development Git pins. Adapt examples to status-field removal while preserving verified-market requirements for quote/build and nonce consumption. Refresh installation and SDK navigation versions. The Shred sniper example accepts SHREDSTREAM_X_TOKEN with the updated parser configuration.

## Migration

Use matching parser/trade versions when passing events or snapshots to router APIs. Remove obsolete parse_status references. This publishes the client SDK only; the on-chain router is not deployed or upgraded.

## Validation

Offline native router regressions and package checks passed. Online simulation tests remain opt-in; no transaction was broadcast.
