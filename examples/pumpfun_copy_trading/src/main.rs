//! PumpFun copy-trading — **production low-latency layout**.
//!
//! Warm before subscribe; hot path uses cached blockhash + event fields only.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use anyhow::{anyhow, Result};
use router_examples_common::warm_router_client;
use sol_parser_sdk::grpc::{
    AccountFilter, ClientConfig, EventType, EventTypeFilter, OrderMode, Protocol,
    TransactionFilter, YellowstoneGrpc,
};
use sol_parser_sdk::DexEvent;
use sol_trade_router_sdk::{
    pumpfun_buy_token_out, DexParamEnum, DexType, PumpFunParams, TradeBuyParams, TradeSellParams,
    TradeTokenType,
};

static ALREADY_EXECUTED: AtomicBool = AtomicBool::new(false);

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("PumpFun copy trading (gRPC → Route CPI, low-latency warm path)...");

    let warm = Arc::new(warm_router_client().await?);

    let config = ClientConfig {
        enable_metrics: false,
        connection_timeout_ms: 10000,
        request_timeout_ms: 30000,
        enable_tls: true,
        order_mode: OrderMode::Unordered,
        ..Default::default()
    };
    let grpc_endpoint = std::env::var("GRPC_ENDPOINT")
        .unwrap_or_else(|_| "https://solana-yellowstone-grpc.publicnode.com:443".to_string());
    let grpc = YellowstoneGrpc::new_with_config(
        grpc_endpoint,
        std::env::var("GRPC_AUTH_TOKEN").ok(),
        config,
    )?;

    let protocols = vec![Protocol::PumpFun];
    let queue = grpc
        .subscribe_dex_events(
            vec![TransactionFilter::for_protocols(&protocols)],
            vec![AccountFilter::for_protocols(&protocols)],
            Some(EventTypeFilter::include_only(vec![
                EventType::PumpFunBuy,
                EventType::PumpFunSell,
                EventType::PumpFunBuyExactSolIn,
                EventType::PumpFunTrade,
            ])),
        )
        .await?;

    println!("Subscribed. Waiting for one PumpFun trade (once)...\n");

    loop {
        if let Some(event) = queue.pop() {
            let run = match &event {
                DexEvent::PumpFunBuy(e)
                | DexEvent::PumpFunSell(e)
                | DexEvent::PumpFunBuyExactSolIn(e)
                | DexEvent::PumpFunTrade(e) => {
                    if !ALREADY_EXECUTED.swap(true, Ordering::SeqCst) {
                        Some(e.clone())
                    } else {
                        None
                    }
                }
                _ => None,
            };
            if let Some(e) = run {
                let warm = warm.clone();
                tokio::spawn(async move {
                    if let Err(err) = copy_hot_path(warm, e).await {
                        eprintln!("copy-trade error: {err:?}");
                        std::process::exit(1);
                    }
                    std::process::exit(0);
                });
                break;
            }
        } else {
            tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
        }
    }

    tokio::signal::ctrl_c().await?;
    Ok(())
}

async fn copy_hot_path(
    warm: Arc<router_examples_common::WarmContext>,
    e: sol_parser_sdk::core::events::PumpFunTradeEvent,
) -> Result<()> {
    let clock = warm.take_tx_clock().await?;
    let slippage_basis_points = Some(100u64);
    let virtual_quote = if e.virtual_quote_reserves != 0 {
        e.virtual_quote_reserves
    } else {
        e.virtual_sol_reserves
    };
    let real_quote = if e.virtual_quote_reserves != 0 {
        e.real_quote_reserves
    } else {
        e.real_sol_reserves
    };

    let extension = PumpFunParams::from_trade(
        e.bonding_curve,
        e.associated_bonding_curve,
        e.mint,
        e.quote_mint,
        e.creator,
        e.creator_vault,
        e.virtual_token_reserves,
        virtual_quote,
        e.real_token_reserves,
        real_quote,
        None,
        e.fee_recipient,
        e.token_program,
        e.is_cashback_coin,
        Some(e.mayhem_mode),
    );

    let buy_params = TradeBuyParams {
        dex_type: DexType::PumpFun,
        input_token_type: TradeTokenType::SOL,
        mint: e.mint,
        input_token_amount: warm.buy_sol_lamports,
        slippage_basis_points,
        recent_blockhash: clock.recent_blockhash(),
        extension_params: DexParamEnum::PumpFun(extension.clone()),
        address_lookup_table_accounts: Vec::new(),
        wait_tx_confirmed: warm.wait_tx_confirmed,
        wait_for_all_submits: false,
        create_input_token_ata: false,
        close_input_token_ata: false,
        create_mint_ata: true,
        durable_nonce: clock.durable_nonce(),
        fixed_output_token_amount: None,
        gas_fee_strategy: warm.gas.clone(),
        simulate: false,
        use_exact_sol_amount: None,
        grpc_recv_us: Some(e.metadata.grpc_recv_us),
    };
    let (ok, sigs, err, _) = warm.client.buy(buy_params).await?;
    if !ok {
        return Err(anyhow!("buy failed: {:?}; sigs: {:?}", err, sigs));
    }

    let pool = sol_trade_router_sdk::pumpfun_from_trade(&e);
    let amount_token = pumpfun_buy_token_out(&pool, warm.buy_sol_lamports);
    if amount_token == 0 {
        return Err(anyhow!("quoted token out is 0"));
    }

    let sell_clock = warm.take_tx_clock().await?;

    let sell_params = TradeSellParams {
        dex_type: DexType::PumpFun,
        output_token_type: TradeTokenType::SOL,
        mint: e.mint,
        input_token_amount: amount_token,
        slippage_basis_points,
        recent_blockhash: sell_clock.recent_blockhash(),
        with_tip: false,
        extension_params: DexParamEnum::PumpFun(extension),
        address_lookup_table_accounts: Vec::new(),
        wait_tx_confirmed: warm.wait_tx_confirmed,
        wait_for_all_submits: false,
        create_output_token_ata: false,
        close_output_token_ata: false,
        close_mint_token_ata: false,
        durable_nonce: sell_clock.durable_nonce(),
        fixed_output_token_amount: None,
        gas_fee_strategy: warm.gas.clone(),
        simulate: false,
        grpc_recv_us: Some(e.metadata.grpc_recv_us),
    };
    let (ok, sigs, err, _) = warm.client.sell(sell_params).await?;
    if !ok {
        return Err(anyhow!("sell failed: {:?}; sigs: {:?}", err, sigs));
    }

    println!(
        "Copy-trade buy+sell submitted via Route CPI (clock={})",
        if sell_clock.is_nonce() {
            "durable_nonce"
        } else {
            "blockhash"
        }
    );
    Ok(())
}
