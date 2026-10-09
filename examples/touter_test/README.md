# Orca Router example

Run from the repository root with a local `.env` containing `RPC_URL`,
`ROUTER_PROGRAM_ID` (your self-deployed matching Router), `PRIVATE_KEY`, `QUOTE_AMOUNT_RAW`, `MIN_OUT_RAW`, and `ORCA_POOL`.
The example uses the program and mint addresses declared in `src/main.rs`.

```sh
# Simulate a swap using an existing config; never initialize or broadcast.
cargo run -p touter_test

# Simulate config initialization, without broadcasting.
cargo run -p touter_test -- --initialize-config

# Explicitly initialize the config after successful simulation.
cargo run -p touter_test -- --initialize-config --send

# Simulate, then broadcast a swap using the existing config.
cargo run -p touter_test -- --send
```

Initialization and swapping are separate operations. Initialization reads an
existing config and returns without sending another transaction. The swap path
requires an initialized, unpaused config with zero fees. Unknown or repeated
arguments are rejected before reading credentials or contacting RPC.

`ORCA_SWAP_DATA_HEX`, when provided, overrides only the inner Orca instruction.
The tag-2 router header includes the expected output mint; the example checks
that mint and preserves the complete header while applying an override.
