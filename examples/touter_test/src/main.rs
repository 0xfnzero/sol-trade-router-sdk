use anyhow::{ensure, Context};
use sol_trade_router_sdk::{
    ata, load_routed_market_by_rpc, LoadMarketRequest, Market, PoolGuardPolicy, RouterClient,
    TradeOpts, ORCA_WHIRLPOOL_PROGRAM,
};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_response::transaction::AccountMeta;
use solana_sdk::{
    instruction::Instruction, pubkey, pubkey::Pubkey, signature::Keypair, signer::Signer,
    transaction::Transaction,
};

// 自部署 Router；与 programs/sol-trade-router/src/lib.rs 保持一致。
const PROGRAM_ID: Pubkey = pubkey!("CmNFUmRJL7YcnVn22oZzwG5Xg5WJqbcHEc6BK5mzDNR8");
const INPUT_MINT: Pubkey = pubkey!("DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263");
const OUTPUT_MINT: Pubkey = pubkey!("5HcMuG7toPaAEQLJSsWZVZG9v4wtTUca5MZvBZeoXQxQ");
const SYSTEM_PROGRAM: Pubkey = pubkey!("11111111111111111111111111111111");
// 本示例只接受：Route(tag=2) 中的单个 Orca swap_v2 leg。
const ROUTE_LEG_DATA_OFFSET: usize = 54;
const ORCA_SWAP_V2_DATA_LEN: usize = 43;

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

fn orca_data_range(ix: &Instruction) -> anyhow::Result<std::ops::Range<usize>> {
    let data = &ix.data;
    ensure!(ix.program_id == PROGRAM_ID, "不是自部署 Router 指令");
    ensure!(data.len() >= ROUTE_LEG_DATA_OFFSET, "Router data 过短");
    ensure!(data[0] == 2 && data[18] == 1, "仅支持单跳 Route 指令");
    ensure!(
        &data[19..51] == ORCA_WHIRLPOOL_PROGRAM.as_ref(),
        "内层程序不是 Orca Whirlpool"
    );
    let len = u16::from_le_bytes([data[52], data[53]]) as usize;
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
    amount_in: u64,
    min_out: u64,
    override_hex: Option<&str>,
) -> anyhow::Result<()> {
    let range = orca_data_range(ix)?;
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

    test_in().await.expect("TODO: panic message");
    let env_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.env");
    dotenvy::from_path(&env_path)
        .with_context(|| format!("无法读取 .env 文件：{}", env_path.display()))?;

    let send = match std::env::args().nth(1).as_deref() {
        None => false,
        Some("--send") => true,
        Some(other) => anyhow::bail!("未知参数 {other}；仅支持 --send"),
    };
    let rpc_url = std::env::var("RPC_URL").context("缺少 RPC_URL 环境变量")?;
    let rpc = RpcClient::new(rpc_url);
    let authority = load_authority()?;
    let authority_pubkey = authority.pubkey();
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
    let (config_address, _) = Pubkey::find_program_address(&[b"config"], &PROGRAM_ID);
    let config = rpc
        .get_account(&config_address)
        .await
        .context("读取 Router config 失败")?;
    ensure!(config.owner == PROGRAM_ID, "Router config owner 不匹配");
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
        .with_program_id(PROGRAM_ID)
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
        .find(|ix| ix.program_id == PROGRAM_ID)
        .context("构造结果缺少 Router 指令")?;
    let custom_data = std::env::var("ORCA_SWAP_DATA_HEX").ok();
    customize_orca_data(route, amount_in, min_out, custom_data.as_deref())?;

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
    let simulation = rpc
        .simulate_transaction(&tx)
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
    let signature = rpc.send_and_confirm_transaction(&tx).await?;
    println!("上链成功：{signature}");
    Ok(())
}

async  fn test_in() -> anyhow::Result<()> {
    let env_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.env");
    dotenvy::from_path(&env_path)
        .with_context(|| format!("无法读取 .env 文件：{}", env_path.display()))?;

    let rpc_url = std::env::var("RPC_URL").context("缺少 RPC_URL 环境变量")?;
    let rpc = RpcClient::new(rpc_url);
    let authority = load_authority()?;
    let authority_pubkey = authority.pubkey();

    // 手续费为 0；收款地址暂时用自己的钱包
    let ix = initialize_config(&authority_pubkey, &authority_pubkey, 0);

    let blockhash = rpc.get_latest_blockhash().await?;
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&authority_pubkey),
        &[&authority],
        blockhash,
    );

    let signature = rpc.send_and_confirm_transaction(&tx).await?;
    println!("初始化成功：{signature}");
    Ok(())
}
fn initialize_config(authority: &Pubkey, fee_recipient: &Pubkey, fee_bps: u16) -> Instruction {
    let (config, bump) = Pubkey::find_program_address(&[b"config"], &PROGRAM_ID);
    let mut data = vec![0]; // initialize 指令 tag
    data.extend_from_slice(&fee_bps.to_le_bytes());
    data.push(bump);

    Instruction {
        program_id: PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(*authority, true),
            AccountMeta::new(config, false),
            AccountMeta::new_readonly(*fee_recipient, false),
            AccountMeta::new_readonly(SYSTEM_PROGRAM, false),
        ],
        data,
    }
}

