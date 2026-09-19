//! Listen-only Yellowstone gRPC example.
//!
//! Prints PumpFun / PumpSwap / Raydium CPMM events and, when possible, builds a
//! `RoutedMarket` via `market_from_dex_event` — no wallet, no submit.
//!
//! Env:
//! - `GRPC_ENDPOINT` (default publicnode)
//! - optional `GRPC_AUTH_TOKEN`
//! - `PROTOCOLS` comma list: `pumpfun,pumpswap,cpmm` (default `pumpfun`)

use anyhow::Result;
use sol_parser_sdk::grpc::{
    AccountFilter, ClientConfig, EventTypeFilter, OrderMode, Protocol, TransactionFilter,
    YellowstoneGrpc,
};
use sol_parser_sdk::DexEvent;
use sol_trade_router_sdk::market_from_dex_event;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("gRPC event listen (sol-parser-sdk → market_from_dex_event)...");

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
        grpc_endpoint.clone(),
        std::env::var("GRPC_AUTH_TOKEN").ok(),
        config,
    )?;

    let protocols = parse_protocols();
    println!("endpoint={grpc_endpoint}");
    println!("protocols={protocols:?}\n");

    let transaction_filter = TransactionFilter::for_protocols(&protocols);
    let account_filter = AccountFilter::for_protocols(&protocols);
    // None = all event types for the selected protocols
    let event_filter: Option<EventTypeFilter> = None;

    let queue = grpc
        .subscribe_dex_events(vec![transaction_filter], vec![account_filter], event_filter)
        .await?;

    println!("Listening (Ctrl+C to stop)...\n");

    loop {
        if let Some(event) = queue.pop() {
            print_event(&event);
            match market_from_dex_event(&event) {
                Some(market) => println!("  → Market: {market:?}\n"),
                None => println!("  → market_from_dex_event: unsupported / incomplete event\n"),
            }
        } else {
            tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
        }
    }
}

fn parse_protocols() -> Vec<Protocol> {
    let raw = std::env::var("PROTOCOLS").unwrap_or_else(|_| "pumpfun".to_string());
    let mut out = Vec::new();
    for part in raw.split(',') {
        match part.trim().to_ascii_lowercase().as_str() {
            "pumpfun" | "pump" => out.push(Protocol::PumpFun),
            "pumpswap" | "pamm" => out.push(Protocol::PumpSwap),
            "cpmm" | "raydium_cpmm" => out.push(Protocol::RaydiumCpmm),
            "amm" | "amm_v4" | "raydium_amm" => out.push(Protocol::RaydiumAmmV4),
            "clmm" => out.push(Protocol::RaydiumClmm),
            "whirlpool" | "orca" => out.push(Protocol::OrcaWhirlpool),
            "dlmm" => out.push(Protocol::MeteoraDlmm),
            "damm" | "damm_v2" => out.push(Protocol::MeteoraDammV2),
            "launchlab" | "bonk" => out.push(Protocol::LaunchLab),
            "stonkfun" => out.push(Protocol::StonkFun),
            other if !other.is_empty() => eprintln!("unknown protocol in PROTOCOLS: {other}"),
            _ => {}
        }
    }
    if out.is_empty() {
        out.push(Protocol::PumpFun);
    }
    out
}

fn print_event(event: &DexEvent) {
    match event {
        DexEvent::PumpFunCreate(e) => {
            println!("[PumpFunCreate] mint={} name={}", e.mint, e.name);
        }
        DexEvent::PumpFunCreateV2(e) => {
            println!("[PumpFunCreateV2] mint={} name={}", e.mint, e.name);
        }
        DexEvent::PumpFunBuy(e) | DexEvent::PumpFunBuyExactSolIn(e) => {
            println!(
                "[PumpFunBuy] mint={} is_created_buy={} sol={} tokens={}",
                e.mint, e.is_created_buy, e.sol_amount, e.token_amount
            );
        }
        DexEvent::PumpFunSell(e) => {
            println!(
                "[PumpFunSell] mint={} sol={} tokens={}",
                e.mint, e.sol_amount, e.token_amount
            );
        }
        DexEvent::PumpFunTrade(e) => {
            println!(
                "[PumpFunTrade] mint={} is_buy={} is_created_buy={} sol={}",
                e.mint, e.is_buy, e.is_created_buy, e.sol_amount
            );
        }
        DexEvent::PumpSwapBuy(e) => {
            println!("[PumpSwapBuy] pool={} base_amount_out={}", e.pool, e.base_amount_out);
        }
        DexEvent::PumpSwapSell(e) => {
            println!("[PumpSwapSell] pool={} base_amount_in={}", e.pool, e.base_amount_in);
        }
        DexEvent::RaydiumCpmmSwap(e) => {
            println!(
                "[RaydiumCpmmSwap] pool={} amount_in={} amount_out={}",
                e.pool_id, e.input_amount, e.output_amount
            );
        }
        other => {
            println!("[event] {}", std::any::type_name_of_val(other));
        }
    }
}
