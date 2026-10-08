# 动态三跳买卖

`build_three_hop_route_instruction(..., spend_actual_output)` 在 `false` 时保留原 tag 2 固定输入格式：每个 leg 自己携带已确定的输入金额与最小输出。`first_min_out`、`second_min_out` 不写入这条旧指令。

在 `true` 时使用 tag 4。第一跳按客户端写入的输入金额执行；Router 读取第一个中间代币账户的**净新增余额**，覆盖第二跳指令的输入金额；再读取第二个中间代币账户的**净新增余额**，覆盖第三跳的输入金额。两个中间账户执行完毕都必须回到交易前余额，最终账户净新增量必须达到 `min_amount_out`。不足或剩余未花完都会让整笔交易回滚。

| 方向 | 第一跳 | 第二跳（quote 池） | 第三跳 |
| --- | --- | --- | --- |
| 买入 | WSOL → USDC | USDC → quote | quote → LaunchLab mint |
| 卖出 | LaunchLab mint → quote | quote → USDC | USDC → WSOL |

第一跳接受调用方构造的 `Leg`（DEX 程序本身还必须允许 CPI）。动态第二跳接受 Raydium AMM V4/CPMM/CLMM、Meteora DLMM/DAMM v2、Orca Whirlpool 的精确输入 swap；动态第三跳还接受 LaunchLab `BuyExactIn`。上述池 swap 也可用于两跳路由的动态末跳。Meteora DAMM v1/DBC、精确输出、部分成交模式不在此功能范围；当前 DLMM/Orca 构造器也没有携带 Token-2022 transfer-hook 所需的额外账户。

构造时给第二、三跳的输入金额写 `0`，各自的 DEX 最小输出仍应按行情设置；另外给 Router 提供两个中间账户的最小净到账量以及最终最小净到账量。账户顺序为原 Route 的六个固定账户、两个可写中间代币账户、依次排列的三个 leg 账户、DEX 程序账户。买入时中间账户为 USDC ATA、quote ATA；卖出时顺序反过来，为 quote ATA、USDC ATA。

第一跳不得写第二个中间账户。输入费与第一跳共用 `fee_source`，`amount_in` 表示手续费和交换花费的总扣减；非零手续费时第一跳的输入应按扣费后的金额构造。WSOL ATA 的准备/包装与最后是否解包为原生 SOL 由客户端处理，Router 本身只处理代币账户。池状态、tick/bin arrays、优先费及交易模拟仍由调用方准备。

本地单元测试只覆盖格式、账户映射及输入金额替换逻辑，并不证明任一具体池的链上 CPI 一定成功。必须升级链上 Router 后才能使用 tag 4；旧部署会拒绝新指令。
