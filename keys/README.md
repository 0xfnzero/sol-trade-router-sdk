# Deploy keys（本地专用，勿提交）

本目录用于存放程序升级权限相关的 keypair，**不会也不应进入 Git**。

## 生成新的程序 keypair

```bash
# 需要 Solana CLI
solana-keygen new --outfile keys/router-keypair.json --no-bip39-passphrase

# 查看对应 Program ID
solana-keygen pubkey keys/router-keypair.json
```

生成后请同步修改：

1. `programs/sol-trade-router/src/lib.rs` 中的 `declare_id!(...)`
2. `crates/sdk/src/constants.rs` 中的 `PROGRAM_ID`

## 安全提示

- 切勿将 `*.json` keypair 推送到公开仓库
- 生产环境建议使用硬件钱包 / 多签管理 upgrade authority
- 首次部署后尽快调用 `initialize`，避免他人抢占 config authority
