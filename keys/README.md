# Deploy keys (local only — never commit)

Store the program upgrade-authority keypair here. `*.json` must stay out of Git.

## Generate a program keypair

```bash
solana-keygen new --outfile keys/router-keypair.json --no-bip39-passphrase
solana-keygen pubkey keys/router-keypair.json
```

Then sync the pubkey into:

1. `programs/sol-trade-router/src/lib.rs` → `declare_id!(...)`
2. `crates/sdk/src/constants.rs` → `PROGRAM_ID`

## Full deploy steps

See the root README:

- [English — Deploy the Router program](../README.md#-deploy-the-router-program)
- [中文 — 部署 Router 合约](../README_CN.md#-部署-router-合约)

Build: `./scripts/build-program.sh`  
Deploy: `solana program deploy --program-id keys/router-keypair.json <path-to.so>`  
Then immediately send `initialize_config` so you own config authority.

## Security

- Never push `*.json` keypairs to a public repo
- Prefer hardware wallet / multisig for production upgrade authority
- Call `initialize` right after the first deploy
