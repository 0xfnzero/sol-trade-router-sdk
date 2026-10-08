# Router security regression checks

Build the program, then execute its actual SBF binary in a local Solana VM:

```sh
cargo build-sbf --tools-version v1.53 \
  --manifest-path programs/sol-trade-router/Cargo.toml --features bpf-entrypoint
python3 -m venv /tmp/router-security-venv
/tmp/router-security-venv/bin/pip install solders==0.29.0
/tmp/router-security-venv/bin/python scripts/test-router-security.py target/deploy/sol_trade_router.so
```

The script never contacts RPC, loads wallet environment variables, or broadcasts
a transaction. It creates ephemeral keys, valid token accounts and mints, and
an initialized config with a 100-bps fee in LiteSVM. The separate fee recipient
lets it verify both the user's debit and the fee credit. Real SPL Token and
System Program CPIs exercise the router independently of DEX pricing.

| Scenario | Required result after the fix |
| --- | --- |
| 10,000 declared; 9,900 transferred; 100 fee | Success; total debit 10,000 |
| 1 declared; 10,000 transferred; fee rounds to 0 | Error 16; all token changes rolled back |
| 100 declared; 10,000 transferred; fee 1 | Error 16; all token changes rolled back |
| Actual output mint differs from the declared target | Error 18 before CPI |
| Native SOL received by the signer reaches the minimum | Success |
| Native SOL received is one lamport below the minimum | Error 10; token changes rolled back |

To reproduce PR #1's behavior against a saved binary for that version, use
`--pr1-format`. It asserts the known regressions rather than expecting the fixed
results, and also checks that the pre-PR tag-2 encoding is rejected.

The Rust tests additionally freeze a pre-PR tag-2 byte fixture, check output
mint validation for SPL Token and Token-2022, verify headers for all route modes,
reject retired tags 3/4, and check the example's explicit send flags. New dynamic
routes use tags 5/6 and require an expected output mint.

These checks do not prove execution against every live DEX pool. The native SOL
VM check exercises settlement and slippage using a System Program CPI, not a
complete PumpFun trade. The affected SDK path is PumpFun's native-SOL settlement;
its WSOL/v2 path is separate.

Ordinary `cargo test` marks network cases as ignored. Run `scripts/check.sh
--mainnet` explicitly, with `SOLANA_RPC_URL` configured. Live success cases now
require successful bank execution; missing coverage, RPC errors, slippage and
balance errors fail the test. Unfunded-input cases require a specific balance
failure. Set `ROUTER_TEST_EVIDENCE_DIR` to retain public signed messages, original
transactions and simulation responses. Ephemeral trade authorities sign and are
verified locally; public virtual funders remain unsigned and RPC `sigVerify` is
false. Fully signed offline legacy/v0/v1 cases separately verify all signatures.

Upgrade the chain program and SDK together before using the repaired protocol.
The original tag-2 header is restored; the shorter PR #1 tag-2 format is retired.
Program deployment is a separate action from merging these source changes.

Recent protocol ABI changes, upgraded dependency pins and deployment evidence
are tracked in [Protocol version review](PROTOCOL_VERSION_REVIEW.md).
