# Sol Trade Router SDK

Version 0.2.0 provides the Rust client for the matching Pinocchio Router source
in this repository. Deploy that program yourself and pass its program ID with
`RouterTradeConfig::with_program_id` or `RouterClient::with_program_id`.
The historical default program ID is not a compatible deployment for this release.

```toml
[dependencies]
sol-trade-router-sdk = "=0.2.0"
```

Dependencies use published sol-trade-sdk 6.0.0 and sol-parser-sdk 0.7.12. No Git
patches or local sibling checkouts are needed. The SDK preserves a zero-RPC hot
path: load current mint/pool/fee state before building trades.

The paired program supports bound tag-2 output mints, dynamic tags 5/6, and
Pump creator-vault preparation tag 7. Retired dynamic tags 3/4 are rejected.
Keep the configured platform fee and recipient consistent with your deployment.

See the repository README and docs/RELEASE_0.2.0.md for self-deployment and
validation. These tests and preparation steps do not deploy a contract or send
network transactions. Current dependencies support Unix targets; the pinned
Yellowstone 13.5.0 dependency still has an upstream Windows import limitation.
