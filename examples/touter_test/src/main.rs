use anyhow::{ensure, Context};
use sol_trade_router_sdk::{
    ata, initialize_config, load_routed_market_by_rpc, LoadMarketRequest, Market, PoolGuardPolicy, RouterClient,
    TradeOpts, ORCA_WHIRLPOOL_PROGRAM,
};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    instruction::Instruction,
    pubkey,
    pubkey::Pubkey,
    signature::Keypair,
    signer::Signer,
    transaction::Transaction,
};

// Test fixture only; runtime always requires the self-deployed ID.
#[cfg(test)]
const PROGRAM_ID: Pubkey = pubkey!("CmNFUmRJL7YcnVn22oZzwG5Xg5WJqbcHEc6BK5mzDNR8");
const INPUT_MINT: Pubkey = pubkey!("DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263");
const OUTPUT_MINT: Pubkey = pubkey!("5HcMuG7toPaAEQLJSsWZVZG9v4wtTUca5MZvBZeoXQxQ");
// 本示例只接受：Route(tag=2) 中的单个 Orca swap_v2 leg。
const ROUTE_LEG_DATA_OFFSET: usize = 86;
const ORCA_SWAP_V2_DATA_LEN: usize = 43;

#[derive(Debug, Default, PartialEq, Eq)]
struct RunOptions {
    send: bool,
    initialize_config: bool,
}

impl RunOptions {
    fn parse(args: impl IntoIterator<Item = String>) -> anyhow::Result<Self> {
        let mut options = Self::default();
        for arg in args {
            match arg.as_str() {
                "--send" if !options.send => options.send = true,
                "--initialize-config" if !options.initialize_config => {
                    options.initialize_config = true
                }
                _ => anyhow::bail!("未知或重复参数 {arg}；支持 --send 和 --initialize-config"),
            }
        }
        Ok(options)
    }
}

fn load_authority() -> anyhow::Result<Keypair> {
    let value = std::env::var("PRIVATE_KEY").context("缺少 PRIVATE_KEY 环境变量")?;
    let value = value.trim();
    let bytes: Vec<u8> = if value.starts_with('[') {
        serde_json::from_str(value)?
    } else {
        bs58::decode(value).into_vec()?
    };
    Ok(Keypair::try_from(bytes.as_slice())?)
}

fn required_u64(name: &str) -> anyhow::Result<u64> {
    let value = std::env::var(name).with_context(|| format!("缺少 {name} 环境变量"))?;
    let parsed = value
        .trim()
        .parse::<u64>()
        .with_context(|| format!("{name} 必须是原始单位 u64"))?;
    ensure!(parsed > 0, "{name} 必须大于 0");
    Ok(parsed)
}

fn decode_hex(input: &str) -> anyhow::Result<Vec<u8>> {
    let input = input.trim().strip_prefix("0x").unwrap_or(input.trim());
    ensure!(input.len() % 2 == 0, "ORCA_SWAP_DATA_HEX 长度必须是偶数");
    input
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair)?;
            Ok(u8::from_str_radix(text, 16)?)
        })
        .collect()
}

fn encode_hex(data: &[u8]) -> String {
    data.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn orca_data_range(ix: &Instruction, program_id: &Pubkey) -> anyhow::Result<std::ops::Range<usize>> {
    let data = &ix.data;
    ensure!(ix.program_id == *program_id, "不是自部署 Router 指令");
    ensure!(data.len() >= ROUTE_LEG_DATA_OFFSET, "Router data 过短");
    ensure!(data[0] == 2 && data[18] == 1, "仅支持单跳 Route 指令");
    ensure!(
        &data[19..51] == OUTPUT_MINT.as_ref(),
        "Router 目标 mint 不匹配"
    );
    ensure!(
        &data[51..83] == ORCA_WHIRLPOOL_PROGRAM.as_ref(),
        "内层程序不是 Orca Whirlpool"
    );
    let len = u16::from_le_bytes([data[84], data[85]]) as usize;
    ensure!(
        len == ORCA_SWAP_V2_DATA_LEN,
        "预期 Orca swap_v2 data 为 43 字节，实际 {len}"
    );
    ensure!(
        data.len() == ROUTE_LEG_DATA_OFFSET + len,
        "Router data 尾部长度不匹配"
    );
    Ok(ROUTE_LEG_DATA_OFFSET..data.len())
}

fn customize_orca_data(
    ix: &mut Instruction,
    program_id: &Pubkey,
    amount_in: u64,
    min_out: u64,
    override_hex: Option<&str>,
) -> anyhow::Result<()> {
    let range = orca_data_range(ix, program_id)?;
    let original = ix.data[range.clone()].to_vec();
    println!("默认 ORCA_SWAP_DATA_HEX={}", encode_hex(&original));
    let Some(hex) = override_hex else {
        return Ok(());
    };
    let replacement = decode_hex(hex)?;
    ensure!(
        replacement.len() == ORCA_SWAP_V2_DATA_LEN,
        "ORCA_SWAP_DATA_HEX 必须为 43 字节"
    );
    ensure!(
        replacement[..8] == original[..8],
        "不能更改 swap_v2 指令标识"
    );
    ensure!(
        u64::from_le_bytes(replacement[8..16].try_into()?) == amount_in,
        "自定义 data 的 amount_in 必须等于 QUOTE_AMOUNT_RAW"
    );
    ensure!(
        u64::from_le_bytes(replacement[16..24].try_into()?) >= min_out,
        "自定义 data 的 other_amount_threshold 不能低于 MIN_OUT_RAW"
    );
    ensure!(
        replacement[40] == 1,
        "只支持 exact-in（amount_specified_is_input=1）"
    );
    ensure!(
        replacement[41] == original[41],
        "不能更改本池的 a_to_b 方向"
    );
    ensure!(
        replacement[42] == 0,
        "不支持额外账户（remaining_accounts_info 必须为 None）"
    );
    ix.data[range].copy_from_slice(&replacement);
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Reject invalid arguments before loading a key or contacting RPC.
    let options = RunOptions::parse(std::env::args().skip(1))?;
    let env_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.env");
    dotenvy::from_path(&env_path)
        .with_context(|| format!("无法读取 .env 文件：{}", env_path.display()))?;

    let program_id: Pubkey = std::env::var("ROUTER_PROGRAM_ID")
        .context("缺少 ROUTER_PROGRAM_ID：请填写自行部署的 Router 地址")?
        .parse().context("ROUTER_PROGRAM_ID 不是有效公钥")?;
    ensure!(program_id != Pubkey::default(), "ROUTER_PROGRAM_ID 不能是 System Program");
    let rpc_url = std::env::var("RPC_URL").context("缺少 RPC_URL 环境变量")?;
    let rpc = RpcClient::new(rpc_url);
    let authority = load_authority()?;
    let authority_pubkey = authority.pubkey();
    if options.initialize_config {
        return initialize_router(&rpc, &program_id, &authority, options.send).await;
    }
    let amount_in = required_u64("QUOTE_AMOUNT_RAW")?;
    let min_out = required_u64("MIN_OUT_RAW")?;
    let pool_address: Pubkey = std::env::var("ORCA_POOL")
        .context("缺少 ORCA_POOL：请填写 BONK/目标币 Whirlpool 池地址，不是 BONK/SOL 池")?
        .parse()
        .context("ORCA_POOL 不是有效公钥")?;

    // 先确认连接的集群中，池账户确实由 Orca 程序持有。
    let pool_account = rpc
        .get_account(&pool_address)
        .await
        .context("读取 Orca 池失败")?;
    ensure!(
        pool_account.owner == ORCA_WHIRLPOOL_PROGRAM,
        "池账户 owner 不是 Orca Whirlpool"
    );

    // 读取已初始化的自部署 Router config；本示例只接受 fee_bps=0。
    let (config_address, _) = Pubkey::find_program_address(&[b"config"], &program_id);
    let config = rpc
        .get_account(&config_address)
        .await
        .context("读取 Router config 失败")?;
    ensure!(config.owner == program_id, "Router config owner 不匹配");
    ensure!(
        config.data.len() >= 80 && &config.data[..8] == b"ROUTCFG1",
        "Router config 未初始化或格式不匹配"
    );
    let fee_bps = u16::from_le_bytes([config.data[72], config.data[73]]);
    ensure!(
        fee_bps == 0,
        "Router 链上 fee_bps={fee_bps}，本示例要求为 0"
    );
    ensure!(config.data[75] == 0, "Router 当前已暂停");
    let fee_recipient = Pubkey::new_from_array(config.data[40..72].try_into()?);

    // SDK 冷路径通过 RPC 加载池、vault、mint 所属 Token Program、tick arrays。
    let (_, mut market) = load_routed_market_by_rpc(
        &rpc,
        LoadMarketRequest::OrcaWhirlpool {
            pool: pool_address,
            input_mint: INPUT_MINT,
            output_mint: OUTPUT_MINT,
        },
        &authority_pubkey,
    )
    .await
    .context("加载 Orca 池参数失败")?;
    let input_token_program = match &mut market.market {
        Market::Whirlpool(pool) => {
            ensure!(pool.whirlpool == pool_address, "加载的池地址不匹配");
            ensure!(
                (pool.mint_a == INPUT_MINT && pool.mint_b == OUTPUT_MINT)
                    || (pool.mint_b == INPUT_MINT && pool.mint_a == OUTPUT_MINT),
                "池内 mint 不匹配：实际为 {} / {}，预期为 {} / {}",
                pool.mint_a,
                pool.mint_b,
                INPUT_MINT,
                OUTPUT_MINT
            );
            ensure!(
                pool.tick_arrays.len() >= 3,
                "Orca swap_v2 需要 3 个 tick arrays"
            );
            // 当前 SDK 对集中流动性市场要求绑定本次输入数量；最终最低到账由用户显式给出。
            pool.quoted_amount_in = Some(amount_in);
            if pool.mint_a == INPUT_MINT {
                pool.token_program_a
            } else {
                pool.token_program_b
            }
        }
        _ => anyhow::bail!("加载结果不是 Orca Whirlpool"),
    };
    ensure!(
        market.meme_mint() == OUTPUT_MINT && market.market.quote_mint() == INPUT_MINT,
        "当前 RouterClient 对本池 A/B 顺序的买入映射不匹配；拒绝发送"
    );

    let input_ata = ata(&authority_pubkey, &INPUT_MINT, &input_token_program);
    let input_balance = rpc
        .get_token_account_balance(&input_ata)
        .await
        .with_context(|| format!("读取输入 BONK ATA {input_ata} 失败；请先创建并充值"))?;
    ensure!(
        input_balance.amount.parse::<u64>()? >= amount_in,
        "BONK ATA 余额不足"
    );

    let router = RouterClient::new(authority_pubkey, fee_recipient, 0)
        .with_program_id(program_id)
        .with_pool_guard(PoolGuardPolicy::default().trust(pool_address));
    let trade = router.buy_with_opts(
        amount_in,
        &market,
        TradeOpts::default()
            .buy_with_token(INPUT_MINT)
            .with_min_out(min_out),
    )?;
    let mut instructions = trade.into_instructions();
    let route = instructions
        .iter_mut()
        .find(|ix| ix.program_id == program_id)
        .context("构造结果缺少 Router 指令")?;
    let custom_data = std::env::var("ORCA_SWAP_DATA_HEX").ok();
    customize_orca_data(route, &program_id, amount_in, min_out, custom_data.as_deref())?;

    println!("payer={authority_pubkey}, pool={pool_address}, input_ata={input_ata}");
    println!("BONK 输入原始数量={amount_in}, 目标币最少到账原始数量={min_out}");
    println!("Router data={}", encode_hex(&route.data));

    let blockhash = rpc.get_latest_blockhash().await?;
    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&authority_pubkey),
        &[&authority],
        blockhash,
    );
    simulate_then_maybe_send(&rpc, &tx, options.send).await
}

async fn simulate_then_maybe_send(
    rpc: &RpcClient,
    tx: &Transaction,
    send: bool,
) -> anyhow::Result<()> {
    let simulation = rpc
        .simulate_transaction(tx)
        .await
        .context("模拟交易失败")?
        .value;
    println!(
        "模拟 CU={:?}, err={:?}",
        simulation.units_consumed, simulation.err
    );
    if let Some(err) = simulation.err {
        for log in simulation.logs.unwrap_or_default() {
            println!("{log}");
        }
        anyhow::bail!("模拟未通过：{err:?}");
    }
    if !send {
        println!("仅模拟，未上链。确认参数后加 --send 才会真实广播。");
        return Ok(());
    }
    let signature = rpc.send_and_confirm_transaction(tx).await?;
    println!("上链成功：{signature}");
    Ok(())
}

async fn initialize_router(rpc: &RpcClient, program_id: &Pubkey, authority: &Keypair, send: bool) -> anyhow::Result<()> {
    let authority_pubkey = authority.pubkey();
    let (config_address, _) = Pubkey::find_program_address(&[b"config"], &program_id);
    if let Some(config) = rpc
        .get_account_with_commitment(&config_address, rpc.commitment())
        .await
        .context("读取 Router config 失败")?
        .value
    {
        ensure!(config.owner == *program_id, "Router config owner 不匹配");
        ensure!(
            config.data.len() >= 80 && &config.data[..8] == b"ROUTCFG1",
            "Router config 格式不匹配"
        );
        println!("Router config 已初始化：{config_address}；未发送初始化交易。");
        return Ok(());
    }

    // 手续费为 0；收款地址暂时用自己的钱包
    let ix = initialize_config(program_id, &authority_pubkey, &authority_pubkey, 0);

    let blockhash = rpc.get_latest_blockhash().await?;
    let tx =
        Transaction::new_signed_with_payer(&[ix], Some(&authority_pubkey), &[authority], blockhash);

    simulate_then_maybe_send(rpc, &tx, send).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_and_initialization_modes_never_send_without_the_send_flag() {
        assert_eq!(RunOptions::parse([]).unwrap(), RunOptions::default());
        assert_eq!(
            RunOptions::parse(["--initialize-config".into()]).unwrap(),
            RunOptions {
                initialize_config: true,
                send: false,
            }
        );
        for args in [
            ["--initialize-config", "--send"],
            ["--send", "--initialize-config"],
        ] {
            assert_eq!(
                RunOptions::parse(args.map(String::from)).unwrap(),
                RunOptions {
                    initialize_config: true,
                    send: true,
                }
            );
        }
        assert_eq!(
            RunOptions::parse(["--send".into()]).unwrap(),
            RunOptions {
                initialize_config: false,
                send: true,
            }
        );
        assert!(RunOptions::parse(["--invalid".into()]).is_err());
        assert!(RunOptions::parse(["--send".into(), "--invalid".into()]).is_err());
        assert!(RunOptions::parse(["--send".into(), "--send".into()]).is_err());
    }

    #[test]
    fn orca_overrides_preserve_the_restored_legacy_header_and_target_mint() {
        let mut data = vec![2];
        data.extend_from_slice(&100u64.to_le_bytes());
        data.extend_from_slice(&10u64.to_le_bytes());
        data.extend_from_slice(&[1, 1]);
        data.extend_from_slice(OUTPUT_MINT.as_ref());
        data.extend_from_slice(ORCA_WHIRLPOOL_PROGRAM.as_ref());
        data.push(15);
        data.extend_from_slice(&43u16.to_le_bytes());
        data.extend_from_slice(&[0; 43]);
        let mut ix = Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![],
            data,
        };
        assert_eq!(orca_data_range(&ix, &PROGRAM_ID).unwrap(), 86..129);
        let custom_program = Pubkey::new_unique();
        ix.program_id = custom_program;
        assert!(orca_data_range(&ix, &PROGRAM_ID).is_err());
        assert_eq!(orca_data_range(&ix, &custom_program).unwrap(), 86..129);
        let initialize = initialize_config(&custom_program, &custom_program, &custom_program, 0);
        assert_eq!(initialize.program_id, custom_program);
        assert_eq!(initialize.accounts[1].pubkey, sol_trade_router_sdk::config_pda(&custom_program).0);
        ix.program_id = PROGRAM_ID;
        let header = ix.data[..86].to_vec();
        let mut replacement = ix.data[86..].to_vec();
        replacement[8..16].copy_from_slice(&100u64.to_le_bytes());
        replacement[16..24].copy_from_slice(&10u64.to_le_bytes());
        replacement[40] = 1;
        customize_orca_data(&mut ix, &PROGRAM_ID, 100, 10, Some(&encode_hex(&replacement))).unwrap();
        assert_eq!(ix.data[..86], header);
        ix.data[19] ^= 1;
        assert!(orca_data_range(&ix, &PROGRAM_ID).is_err());
    }
}
