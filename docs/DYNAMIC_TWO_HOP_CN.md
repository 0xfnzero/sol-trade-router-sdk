# 动态两跳买卖（新增指令，旧 Route 保留）

`ROUTE_DYNAMIC`（tag `5`）既可用于买入 `WSOL -> quote -> 目标 mint`，也可用于卖出 `目标 mint -> quote -> WSOL`。第一跳可由现有 leg 构造器提供。第二跳接受 Raydium LaunchLab `BuyExactIn`，或 Raydium AMM V4/CPMM/CLMM、Meteora DLMM/DAMM v2、Orca Whirlpool 的精确输入 swap。卖出时第一跳可使用 LaunchLab `SellExactIn`，第二跳用 quote/WSOL 池。Meteora DAMM v1/DBC 不在当前 SDK 支持范围。

tag `2` 恢复最初的 50 字节头部（不含 tag，包含 32 字节目标 mint）和 exact-in 总花费相等校验。PR #1 曾将其缩短为 18 字节；该过渡格式不再支持。只有同步更新 SDK 和链上程序后，tag `5` 才能使用。

## 执行语义

1. Router 在第一跳前记录用户 quote 代币账户余额；扣除 Router 输入费后，执行第一跳 CPI。
2. `实际 quote 到账 = 第一跳后余额 - 第一跳前余额`；必须至少达到 `quote_min_out`。
3. Router 只对经过校验的精确输入第二跳指令，在栈上的本地副本中把输入金额替换为实际到账量，然后执行 CPI。Raydium AMM V4 的金额偏移为 1，其余当前支持的格式为 8；原交易数据不改写。
4. 第二跳后，quote 账户余额必须回到第一跳前的值；目标账户实际净增量必须达到 `min_target_out`。任何一步失败，整笔交易回滚。账户原有的 quote 不会被当成本笔可花余额。

LaunchLab 临近毕业等情况下可能无法花完输入；严格“全花完”模式会回滚，不会留下半笔成交。带 Token-2022 转账费的 quote 以用户账户**净到账**量为准。Orca/CLMM 的 tick arrays 与 DLMM 的 bin arrays 仍须由调用方按可能成交范围提供。卖出所得若为 WSOL，Router 不会自动将其解包为原生 SOL。

## Rust 构造示意（不提交）

```rust,ignore
use sol_trade_router_sdk::{
    build_deployed_dynamic_quote_buy, create_ata, create_wsol_ata, wrap_sol,
    Market,
};

// quote_pool、target_pool 为已解析且已验证的当前池快照；payer/fee_recipient 为公钥。
let built = build_deployed_dynamic_quote_buy(
    &payer,
    &fee_recipient, // 必须与链上 Router config 一致
    route_amount_in, // 总 WSOL 输入预算；fee_bps=0 时等于 first_leg_amount_in
    first_leg_amount_in,
    quote_min_out,   // 第一跳净到账下限；每笔交易重新估算
    min_target_out,  // 最终 mint 净到账下限
    &quote_pool,
    &target_pool,    // Market::LaunchLabInner 或 Market::CpmmOuter
)?;

let mut ixs = vec![
    create_wsol_ata(&payer),
    create_ata(&payer, &payer, &built.quote_mint, &built.quote_token_program),
    create_ata(&payer, &payer, &built.output_mint, &built.output_token_program),
];
ixs.extend(wrap_sol(&payer, route_amount_in)); // 已持有 WSOL 时不要重复包装
ixs.push(built.instruction);
// 在这里模拟、签名、发送；此示例本身不会广播。
```

`create_ata` 为幂等指令；热路径可提前准备 WSOL/quote ATA，避免每笔交易支付创建账户的 CU 和租金。若 Router `fee_bps > 0`，还须确保手续费接收者的 WSOL ATA 存在，且第一跳输入金额与链上实际扣费后的预算一致。大账户列表交易可能需要已激活的地址查找表（ALT）。

## 新格式

- data：`tag(5) | amount_in:u64 | min_target_out:u64 | fee_asset(1) | num_legs(2) | expected_output_mint:[u8;32] | quote_min_out:u64 | 两个旧格式 leg 编码`。
- accounts：旧 Route 的六个固定账户后，新增一个可写的用户 quote 账户；之后仍为两个 leg 的账户和 DEX 程序账户。
- 当前仅支持 exact-in 与 SPL/WSOL 输入；不支持在动态模式下使用 exact-out 标志。

本地 `cargo test -p sol-trade-router --offline` 和 `cargo test -p sol-trade-router-sdk --offline` 可验证解析与构造。离线通过不等于真实池状态下可成交；升级前应对每类池做链上模拟。

## 安全协议迁移

安全版动态两跳使用 tag 5；原 tag 3、4 未绑定目标 mint，升级后的程序会拒绝它们。请同时更新 SDK 和链上 Router，并重新构造待发送交易；不要重放旧格式交易。`build_dynamic_route_instruction` 现在在 `min_amount_out` 后接收 `&expected_output_mint`。便捷函数 `build_dynamic_quote_buy` / `build_deployed_dynamic_quote_buy` 会从目标池填写该字段，其调用参数不变。动态最终输出只能是代币账户；SOL 请使用 WSOL。

所有 exact-in 路由都要求手续费与交换的总扣减**等于** `amount_in`；超预算、少报输入或未用完声明金额均会回滚。
