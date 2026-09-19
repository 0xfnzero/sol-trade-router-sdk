//! Mainnet simulate suite for **RouterClient / TradingClient** paths.
//!
//! Each scenario:
//! 1. `create_wallet()` — ephemeral Keypair (never a real private key)
//! 2. Virtually fund from a mainnet whale (`sigVerify=false`)
//! 3. Build Route ix via `RouterClient` (assert `PROGRAM_ID`)
//! 4. Simulate Route tx (Soft if router program not deployed yet)
//! 5. Also simulate equivalent direct DEX legs (real layout coverage)
//!
//! ```bash
//! RUN_MAINNET_SIM=1 cargo test -p sol-trade-router-sdk mainnet_router -- --nocapture --test-threads=1
//! ```

#![cfg(test)]

use sol_parser_sdk::core::events::DexEvent;
use solana_sdk::{pubkey::Pubkey, signer::Signer};
use sol_trade_sdk::trading::core::params::{
    DexParamEnum, RaydiumCpmmParams, StonkFunViaSolParams,
};

use crate::adapter::to_routed_market_for_user;
use crate::ata::{ata, create_ata, create_wsol_ata, wrap_sol, AtaPolicy};
use crate::constants::*;
use crate::legs::{
    cpmm_swap_exact_out_leg, cpmm_swap_leg, pumpswap_buy_exact_out_leg, pumpswap_buy_leg,
    raydium_amm_v4_swap_leg,
};
use crate::mainnet_sim::{
    assert_route_ix, assert_sim_ok, create_wallet, create_wallets, enabled, fill_amm_v4_mints,
    fixtures, load_cpmm_pool, load_pumpswap_pool, require_rpc, rpc, router_program_deployed,
    scan_events, simulate_built_trade, simulate_legs_funded, soft_coverage, SimVerdict,
};
use crate::market::{Market, RoutedMarket};
use crate::parser::{amm_v4_from_swap, launchlab_from_trade, pumpswap_from_buy};
use crate::pool_guard::PoolGuardPolicy;
use crate::trade::{RouterClient, TradeOpts};
use crate::transfer_fee::TokenTransferFee;

fn fee_recipient() -> Pubkey {
    // Distinct from payer; fee_bps=0 so ATA is unused on-chain.
    Pubkey::new_unique()
}

fn router_for(wallet: &Pubkey) -> RouterClient {
    RouterClient::new(*wallet, fee_recipient(), 0).with_pool_guard(PoolGuardPolicy::disabled())
}

fn setup_wsol_and_meme(
    wallet: &Pubkey,
    meme: Pubkey,
    meme_tp: Pubkey,
    wrap_lamports: u64,
) -> Vec<solana_sdk::instruction::Instruction> {
    let mut ixs = vec![
        create_wsol_ata(wallet),
        create_ata(wallet, wallet, &meme, &meme_tp),
    ];
    ixs.extend(wrap_sol(wallet, wrap_lamports));
    ixs
}

fn assert_funded_ok(scenario: &str, verdict: Option<SimVerdict>) {
    match verdict {
        None => panic!("[{scenario}] required funder / RPC for coverage, none available"),
        Some(v) => assert_sim_ok(scenario, v),
    }
}

/// Compact ATA policy for Route simulate: meme + WSOL in-tx (no unused quote create).
fn route_buy_ata() -> AtaPolicy {
    AtaPolicy::for_buy().with_create_wsol(true)
}

/// ViaSol needs stock/quote ATA as well.
fn viasol_buy_ata() -> AtaPolicy {
    AtaPolicy {
        create_meme: true,
        create_wsol: true,
        create_quote: true,
        ..AtaPolicy::default()
    }
}

fn cpmm_to_params(pool: &crate::market::CpmmPool) -> RaydiumCpmmParams {
    RaydiumCpmmParams {
        pool_state: pool.pool_state,
        amm_config: pool.amm_config,
        base_mint: pool.base_mint,
        quote_mint: pool.quote_mint,
        base_reserve: pool.base_reserve,
        quote_reserve: pool.quote_reserve,
        base_vault: pool.base_vault,
        quote_vault: pool.quote_vault,
        base_token_program: pool.base_token_program,
        quote_token_program: pool.quote_token_program,
        observation_state: pool.observation_state,
        trade_fee_rate: pool.trade_fee_rate,
        protocol_fee_rate: 0,
        fund_fee_rate: 0,
        creator_fee_rate: pool.creator_fee_rate,
        creator_fee_on: pool.creator_fee_on,
        enable_creator_fee: pool.enable_creator_fee,
        base_transfer_fee: sol_trade_sdk::trading::core::params::TokenTransferFee {
            basis_points: pool.base_transfer_fee.basis_points,
            maximum_fee: pool.base_transfer_fee.maximum_fee,
        },
        quote_transfer_fee: sol_trade_sdk::trading::core::params::TokenTransferFee {
            basis_points: pool.quote_transfer_fee.basis_points,
            maximum_fee: pool.quote_transfer_fee.maximum_fee,
        },
    }
}

// ─── Wallet creation ─────────────────────────────────────────────────────────

#[test]
fn mainnet_router_creates_many_unique_wallets_and_funds() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let wallets = create_wallets(5);
    assert_eq!(wallets.len(), 5);
    for (i, w) in wallets.iter().enumerate() {
        for (j, o) in wallets.iter().enumerate() {
            if i != j {
                assert_ne!(w.pubkey(), o.pubkey());
            }
        }
        // Fund-only simulate proves whale debit + fresh pubkey works.
        assert_funded_ok(
            &format!("wallet_fund_{i}"),
            crate::mainnet_sim::simulate_with_fresh_wallet(&client, w, vec![]),
        );
    }
    let deployed = router_program_deployed(&client);
    println!("[mainnet_router] router_program_deployed={deployed}");
}

// ─── CPMM via RouterClient ───────────────────────────────────────────────────

#[test]
fn mainnet_router_cpmm_wsol_stonk_buy() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let pool = load_cpmm_pool(&client, &fixtures::WSOL_STONK_CPMM)
        .expect("load WSOL_STONK_CPMM fixture");
    let meme = pool.meme_mint();
    let meme_tp = pool
        .token_program_for(&meme)
        .unwrap_or(TOKEN_PROGRAM);
    let amount = 50_000u64;

    let wallet = create_wallet();
    let user = wallet.pubkey();
    let market = RoutedMarket::new(Market::CpmmOuter(pool.clone()));
    let router = router_for(&user);
    let built = router
        .buy_with_opts(
            amount,
            &market,
            TradeOpts::default()
                .buy_with_wsol()
                .with_ata(route_buy_ata()),
        )
        .expect("router cpmm buy build");
    assert_route_ix(&built, "router_cpmm_stonk");

    // Route path (Soft OK if program undeployed).
    assert_funded_ok(
        "router_cpmm_stonk_route",
        simulate_built_trade(&client, &wallet, built.clone()),
    );

    // Direct DEX layout proof with a second fresh wallet.
    let wallet2 = create_wallet();
    let user2 = wallet2.pubkey();
    let setup = setup_wsol_and_meme(&user2, meme, meme_tp, amount);
    let leg = cpmm_swap_leg(
        &user2,
        &pool,
        amount,
        0,
        WSOL_MINT,
        meme,
        ata(&user2, &WSOL_MINT, &TOKEN_PROGRAM),
        ata(&user2, &meme, &meme_tp),
    )
    .expect("cpmm leg");
    assert_funded_ok(
        "router_cpmm_stonk_direct",
        simulate_legs_funded(&client, &wallet2, setup, &[leg]),
    );
}

#[test]
fn mainnet_router_cpmm_exact_out_buy() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let pool = load_cpmm_pool(&client, &fixtures::WSOL_STONK_CPMM)
        .expect("load WSOL_STONK_CPMM fixture");
    let meme = pool.meme_mint();
    let meme_tp = pool
        .token_program_for(&meme)
        .unwrap_or(TOKEN_PROGRAM);
    let max_in = 200_000u64;
    // Small exact-out target relative to reserves.
    let amount_out = (pool.quote_reserve.min(pool.base_reserve) / 1_000_000).max(1).min(1_000);

    let wallet = create_wallet();
    let user = wallet.pubkey();
    let market = RoutedMarket::new(Market::CpmmOuter(pool.clone()));
    let built = router_for(&user)
        .buy_with_opts(
            max_in,
            &market,
            TradeOpts::default()
                .buy_with_wsol()
                .with_fixed_output(amount_out)
                .with_ata(route_buy_ata()),
        )
        .expect("router cpmm exact-out build");
    assert_route_ix(&built, "router_cpmm_exact_out");
    assert_funded_ok(
        "router_cpmm_exact_out_route",
        simulate_built_trade(&client, &wallet, built),
    );

    let wallet2 = create_wallet();
    let user2 = wallet2.pubkey();
    let setup = setup_wsol_and_meme(&user2, meme, meme_tp, max_in);
    let leg = cpmm_swap_exact_out_leg(
        &user2,
        &pool,
        max_in,
        amount_out,
        WSOL_MINT,
        meme,
        ata(&user2, &WSOL_MINT, &TOKEN_PROGRAM),
        ata(&user2, &meme, &meme_tp),
    )
    .expect("exact-out leg");
    assert_eq!(&leg.data[..8], &CPMM_SWAP_BASE_OUT);
    assert_funded_ok(
        "router_cpmm_exact_out_direct",
        simulate_legs_funded(&client, &wallet2, setup, &[leg]),
    );
}

#[test]
fn mainnet_router_cpmm_buy_sell_roundtrip() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let pool = load_cpmm_pool(&client, &fixtures::WSOL_CARDS_CPMM)
        .expect("load WSOL_CARDS_CPMM fixture");
    let amount = 50_000u64;

    let wallet = create_wallet();
    let user = wallet.pubkey();
    let market = RoutedMarket::new(Market::CpmmOuter(pool.clone()));
    let router = router_for(&user);
    let buy = router
        .buy_with_opts(
            amount,
            &market,
            TradeOpts::default()
                .buy_with_wsol()
                .with_ata(route_buy_ata()),
        )
        .expect("buy");
    assert_route_ix(&buy, "router_cpmm_cards_buy");

    // Estimate a tiny sell size from quote (floor 1).
    let expected_out = crate::quote::cpmm_out(&pool, amount, pool.base_mint == WSOL_MINT)
        .unwrap_or(1)
        .max(1);
    let sell_amt = (expected_out / 4).max(1);
    let sell = router
        .sell_with_opts(
            sell_amt,
            &market,
            TradeOpts::default().sell_to_wsol(),
        )
        .expect("sell");
    assert_route_ix(&sell, "router_cpmm_cards_sell");

    // Single-wallet buy then sell in one simulate (buy credits ATA before sell spends).
    let mut ixs = buy.into_instructions();
    // Drop sell setup that would re-create WSOL; keep sell route + cleanup.
    if let Some(route) = sell.route {
        ixs.push(route);
    }
    ixs.extend(sell.cleanup);
    assert_funded_ok(
        "router_cpmm_cards_roundtrip",
        crate::mainnet_sim::simulate_with_fresh_wallet(&client, &wallet, ixs),
    );
}

// ─── PumpSwap via RouterClient ───────────────────────────────────────────────

#[test]
fn mainnet_router_pumpswap_buy() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let pool = load_pumpswap_pool(&client, &fixtures::PUMPSWAP_POOL)
        .or_else(|| load_pumpswap_pool(&client, &fixtures::PUMPSWAP_SEED_POOL))
        .expect("load pumpswap fixture");
    assert_eq!(pool.quote_mint, WSOL_MINT);
    let amount = 100_000u64;

    let wallet = create_wallet();
    let user = wallet.pubkey();
    let market = RoutedMarket::pumpswap(pool.clone());
    let built = router_for(&user)
        .buy_with_opts(
            amount,
            &market,
            TradeOpts::default()
                .buy_with_wsol()
                .with_ata(route_buy_ata()),
        )
        .expect("pumpswap buy");
    assert_route_ix(&built, "router_pumpswap");
    assert_funded_ok(
        "router_pumpswap_route",
        simulate_built_trade(&client, &wallet, built),
    );

    let wallet2 = create_wallet();
    let user2 = wallet2.pubkey();
    let setup = setup_wsol_and_meme(&user2, pool.base_mint, pool.base_token_program, amount);
    let leg = pumpswap_buy_leg(&user2, &pool, amount, 0).expect("ps leg");
    assert_funded_ok(
        "router_pumpswap_direct",
        simulate_legs_funded(&client, &wallet2, setup, &[leg]),
    );
}

#[test]
fn mainnet_router_pumpswap_exact_out_buy() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let pool = load_pumpswap_pool(&client, &fixtures::PUMPSWAP_POOL)
        .expect("load pumpswap fixture");
    let max_quote = 500_000u64;
    let base_out = (pool.base_reserve / 1_000_000).max(1).min(10_000);

    let wallet = create_wallet();
    let user = wallet.pubkey();
    let market = RoutedMarket::pumpswap(pool.clone());
    let built = router_for(&user)
        .buy_with_opts(
            max_quote,
            &market,
            TradeOpts::default()
                .buy_with_wsol()
                .with_fixed_output(base_out)
                .with_ata(route_buy_ata()),
        )
        .expect("ps exact-out");
    assert_route_ix(&built, "router_pumpswap_exact_out");
    assert_funded_ok(
        "router_pumpswap_exact_out_route",
        simulate_built_trade(&client, &wallet, built),
    );

    let wallet2 = create_wallet();
    let user2 = wallet2.pubkey();
    let setup = setup_wsol_and_meme(&user2, pool.base_mint, pool.base_token_program, max_quote);
    let leg = pumpswap_buy_exact_out_leg(&user2, &pool, base_out, max_quote).expect("ps eo");
    assert_eq!(&leg.data[..8], &PUMPSWAP_BUY);
    assert_funded_ok(
        "router_pumpswap_exact_out_direct",
        simulate_legs_funded(&client, &wallet2, setup, &[leg]),
    );
}

// ─── ViaSol (graduated CPMM + SOL hop) ───────────────────────────────────────

#[test]
fn mainnet_router_stonk_viasol_graduated() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let meme_pool = load_cpmm_pool(&client, &fixtures::GRAD_POOL)
        .expect("load GRAD_POOL (graduated StonkFun CPMM)");
    let bridge = load_cpmm_pool(&client, &fixtures::WSOL_STONK_CPMM)
        .expect("load WSOL_STONK_CPMM bridge");
    let amount = 100_000u64;

    let wallet = create_wallet();
    let user = wallet.pubkey();
    let market = RoutedMarket::with_bridge(Market::CpmmOuter(meme_pool.clone()), bridge.clone());
    let built = router_for(&user)
        .buy_with_opts(
            amount,
            &market,
            TradeOpts::default()
                .buy_with_sol()
                .with_ata(viasol_buy_ata()),
        )
        .expect("viasol graduated build");
    assert_route_ix(&built, "router_viasol_grad");
    // Multi-leg Route — Soft if undeployed.
    assert_funded_ok(
        "router_viasol_grad_route",
        simulate_built_trade(&client, &wallet, built),
    );

    // Prove hop1 (SOL→stock) layout with direct CPMM.
    let wallet2 = create_wallet();
    let user2 = wallet2.pubkey();
    let stock = fixtures::GRAD_QUOTE_STONK;
    let mut setup = vec![create_wsol_ata(&user2)];
    setup.extend(wrap_sol(&user2, amount));
    setup.push(create_ata(&user2, &user2, &stock, &TOKEN_PROGRAM));
    let hop1 = cpmm_swap_leg(
        &user2,
        &bridge,
        amount,
        0,
        WSOL_MINT,
        stock,
        ata(&user2, &WSOL_MINT, &TOKEN_PROGRAM),
        ata(&user2, &stock, &TOKEN_PROGRAM),
    )
    .expect("hop1");
    assert_funded_ok(
        "router_viasol_grad_hop1_direct",
        simulate_legs_funded(&client, &wallet2, setup, &[hop1]),
    );
}

#[test]
fn mainnet_router_stonk_viasol_curve_from_events() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let bridge = match load_cpmm_pool(&client, &fixtures::WSOL_CARDS_CPMM) {
        Some(p) => p,
        None => {
            println!("[router_viasol_curve] skip: bridge fixture unavailable");
            return;
        }
    };
    let found = scan_events(
        &client,
        &[fixtures::CURVE_POOL, LAUNCHLAB_PROGRAM],
        80,
        |_sig, ev| {
            let DexEvent::RaydiumLaunchlabTrade(e) = ev else {
                return false;
            };
            if e.pool_state != fixtures::CURVE_POOL && e.pool_state == Pubkey::default() {
                return false;
            }
            let Some(inner) = launchlab_from_trade(e) else {
                return false;
            };
            if inner.quote_mint != fixtures::CURVE_QUOTE_CARDS
                && inner.quote_mint != bridge.quote_mint
                && inner.quote_mint != bridge.base_mint
            {
                // Still ok if quote matches bridge non-WSOL side.
                let stock = if bridge.base_mint == WSOL_MINT {
                    bridge.quote_mint
                } else {
                    bridge.base_mint
                };
                if inner.quote_mint != stock {
                    return false;
                }
            }
            let wallet = create_wallet();
            let user = wallet.pubkey();
            let market =
                RoutedMarket::with_bridge(Market::LaunchLabInner(inner.clone()), bridge.clone());
            let built = match router_for(&user).buy_with_opts(
                100_000,
                &market,
                TradeOpts::default()
                    .buy_with_sol()
                    .with_ata(viasol_buy_ata()),
            ) {
                Ok(b) => b,
                Err(err) => {
                    println!("[router_viasol_curve] build err={err}");
                    return false;
                }
            };
            assert_route_ix(&built, "router_viasol_curve");
            assert_funded_ok(
                "router_viasol_curve_route",
                simulate_built_trade(&client, &wallet, built),
            );
            true
        },
    );
    // Event coverage is best-effort — fixture pool may be quiet.
    if !found {
        println!("[router_viasol_curve] no matching LaunchLab events (soft skip)");
    }
}

// ─── Adapter DexParamEnum → RouterClient ─────────────────────────────────────

#[test]
fn mainnet_router_adapter_cpmm_params_roundtrip() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let pool = load_cpmm_pool(&client, &fixtures::WSOL_STONK_CPMM).expect("cpmm");
    let meme = pool.meme_mint();
    let params = DexParamEnum::RaydiumCpmm(cpmm_to_params(&pool));

    let wallet = create_wallet();
    let user = wallet.pubkey();
    let market = to_routed_market_for_user(&params, meme, &user).expect("adapter");
    let built = router_for(&user)
        .buy_with_opts(
            50_000,
            &market,
            TradeOpts::default()
                .buy_with_wsol()
                .with_ata(route_buy_ata()),
        )
        .expect("build from adapted market");
    assert_route_ix(&built, "adapter_cpmm");
    assert_funded_ok(
        "adapter_cpmm_route",
        simulate_built_trade(&client, &wallet, built),
    );
}

#[test]
fn mainnet_router_adapter_viasol_params() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let meme_pool = load_cpmm_pool(&client, &fixtures::GRAD_POOL).expect("grad");
    let bridge = load_cpmm_pool(&client, &fixtures::WSOL_STONK_CPMM).expect("bridge");
    let via = StonkFunViaSolParams::graduated_with_cpmm(
        cpmm_to_params(&meme_pool),
        cpmm_to_params(&bridge),
    );
    let wallet = create_wallet();
    let user = wallet.pubkey();
    let market = to_routed_market_for_user(
        &DexParamEnum::StonkFunViaSol(via),
        fixtures::GRAD_MEME_KNOTS,
        &user,
    )
    .expect("adapter viasol");
    assert!(market.bridge.is_some());
    let built = router_for(&user)
        .buy_with_opts(
            80_000,
            &market,
            TradeOpts::default()
                .buy_with_sol()
                .with_ata(viasol_buy_ata()),
        )
        .expect("viasol from params");
    assert_route_ix(&built, "adapter_viasol");
    assert_funded_ok(
        "adapter_viasol_route",
        simulate_built_trade(&client, &wallet, built),
    );
}

// ─── AmmV4 / PumpFun live-event Router builds ────────────────────────────────

#[test]
fn mainnet_router_amm_v4_from_live_events() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let found = scan_events(
        &client,
        &[
            fixtures::AMM_V4_WSOL_USDC,
            fixtures::AMM_V4_WSOL_USDT,
            RAYDIUM_AMM_V4_PROGRAM,
        ],
        80,
        |sig, ev| {
            let DexEvent::RaydiumAmmV4Swap(e) = ev else {
                return false;
            };
            let Some(mut pool) = amm_v4_from_swap(e) else {
                return false;
            };
            if !fill_amm_v4_mints(&client, &mut pool) {
                return false;
            }
            if pool.coin_mint != WSOL_MINT && pool.pc_mint != WSOL_MINT {
                return false;
            }
            let amount = e.amount_in.max(100_000).min(2_000_000);
            let wallet = create_wallet();
            let user = wallet.pubkey();
            let market = RoutedMarket::raydium_amm_v4(pool.clone());
            let built = match router_for(&user).buy_with_opts(
                amount,
                &market,
                TradeOpts::default()
                    .buy_with_wsol()
                    .with_ata(route_buy_ata()),
            ) {
                Ok(b) => b,
                Err(err) => {
                    println!("[router_amm_v4] build err={err}");
                    return false;
                }
            };
            assert_route_ix(&built, "router_amm_v4");
            println!("[router_amm_v4] sig={sig} wallet={user}");
            assert_funded_ok(
                "router_amm_v4_route",
                simulate_built_trade(&client, &wallet, built),
            );

            let wallet2 = create_wallet();
            let user2 = wallet2.pubkey();
            let out = if pool.coin_mint == WSOL_MINT {
                pool.pc_mint
            } else {
                pool.coin_mint
            };
            let mut setup = vec![create_wsol_ata(&user2)];
            setup.extend(wrap_sol(&user2, amount));
            setup.push(create_ata(&user2, &user2, &out, &pool.token_program));
            let Ok(leg) = raydium_amm_v4_swap_leg(&user2, &pool, amount, 0, WSOL_MINT) else {
                return false;
            };
            assert_funded_ok(
                "router_amm_v4_direct",
                simulate_legs_funded(&client, &wallet2, setup, &[leg]),
            );
            true
        },
    );
    soft_coverage("router_amm_v4", found);
}

#[test]
fn mainnet_router_pumpswap_from_live_buy_events() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let found = scan_events(&client, &[PUMPSWAP_PROGRAM, fixtures::PUMPSWAP_POOL], 60, |sig, ev| {
        let DexEvent::PumpSwapBuy(e) = ev else {
            return false;
        };
        if e.quote_mint != WSOL_MINT || e.base_mint == Pubkey::default() {
            return false;
        }
        let pool = pumpswap_from_buy(e);
        let amount = e.quote_amount_in.max(50_000).min(500_000);
        let wallet = create_wallet();
        let user = wallet.pubkey();
        let market = RoutedMarket::pumpswap(pool.clone());
        let built = match router_for(&user).buy_with_opts(
            amount,
            &market,
            TradeOpts::default()
                .buy_with_wsol()
                .with_ata(route_buy_ata()),
        ) {
            Ok(b) => b,
            Err(err) => {
                println!("[router_ps_live] build err={err}");
                return false;
            }
        };
        assert_route_ix(&built, "router_ps_live");
        println!("[router_ps_live] sig={sig} wallet={user}");
        assert_funded_ok(
            "router_ps_live_route",
            simulate_built_trade(&client, &wallet, built),
        );
        true
    });
    soft_coverage("router_ps_live", found);
}

#[test]
fn mainnet_router_amm_v4_from_fixture_pool() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let pool = match crate::mainnet_sim::load_amm_v4_pool(&client, &fixtures::AMM_V4_WSOL_USDC)
        .or_else(|| crate::mainnet_sim::load_amm_v4_pool(&client, &fixtures::AMM_V4_WSOL_USDT))
    {
        Some(p) => p,
        None => {
            println!("[router_amm_v4_fixture] soft skip: fixture AmmInfo unavailable");
            return;
        }
    };
    assert!(pool.coin_mint == WSOL_MINT || pool.pc_mint == WSOL_MINT);
    let amount = 200_000u64;
    let wallet = create_wallet();
    let user = wallet.pubkey();
    let market = RoutedMarket::raydium_amm_v4(pool.clone());
    let built = router_for(&user)
        .buy_with_opts(
            amount,
            &market,
            TradeOpts::default()
                .buy_with_wsol()
                .with_ata(route_buy_ata()),
        )
        .expect("amm_v4 fixture buy");
    assert_route_ix(&built, "router_amm_v4_fixture");
    assert_funded_ok(
        "router_amm_v4_fixture_route",
        simulate_built_trade(&client, &wallet, built),
    );

    let wallet2 = create_wallet();
    let user2 = wallet2.pubkey();
    let out = if pool.coin_mint == WSOL_MINT {
        pool.pc_mint
    } else {
        pool.coin_mint
    };
    let mut setup = vec![create_wsol_ata(&user2)];
    setup.extend(wrap_sol(&user2, amount));
    setup.push(create_ata(&user2, &user2, &out, &pool.token_program));
    let leg = raydium_amm_v4_swap_leg(&user2, &pool, amount, 0, WSOL_MINT).expect("amm leg");
    assert_funded_ok(
        "router_amm_v4_fixture_direct",
        simulate_legs_funded(&client, &wallet2, setup, &[leg]),
    );
}

#[test]
fn mainnet_router_multi_wallet_cpmm_parallel_builds() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let pool = load_cpmm_pool(&client, &fixtures::WSOL_STONK_CPMM)
        .expect("load WSOL_STONK_CPMM");
    let wallets = create_wallets(3);
    for (i, wallet) in wallets.iter().enumerate() {
        let user = wallet.pubkey();
        let market = RoutedMarket::new(Market::CpmmOuter(pool.clone()));
        let built = router_for(&user)
            .buy_with_opts(
                40_000 + i as u64 * 1_000,
                &market,
                TradeOpts::default()
                    .buy_with_wsol()
                    .with_ata(route_buy_ata()),
            )
            .expect("multi wallet buy");
        assert_route_ix(&built, &format!("router_multi_cpmm_{i}"));
        assert_funded_ok(
            &format!("router_multi_cpmm_{i}"),
            simulate_built_trade(&client, wallet, built),
        );
    }
}

#[test]
fn mainnet_router_trading_client_build_and_simulate() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let pool = load_cpmm_pool(&client, &fixtures::WSOL_CARDS_CPMM)
        .expect("load WSOL_CARDS_CPMM");
    let wallet = create_wallet();
    let payer = std::sync::Arc::new(wallet.insecure_clone());
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio rt");
    let trading = rt.block_on(async {
        let trade_cfg = sol_trade_sdk::common::TradeConfig::new(
            crate::mainnet_sim::rpc_url(),
            vec![],
            solana_commitment_config::CommitmentConfig::confirmed(),
        );
        let cfg = crate::client::RouterTradeConfig::new(trade_cfg, fee_recipient(), 0)
            .with_pool_guard(PoolGuardPolicy::disabled());
        crate::client::TradingClient::new(payer.clone(), cfg).await
    });
    let market = RoutedMarket::new(Market::CpmmOuter(pool));
    let built = trading
        .build_buy_from_market(
            60_000,
            &market,
            TradeOpts::default()
                .buy_with_wsol()
                .with_ata(route_buy_ata()),
        )
        .expect("TradingClient build_buy_from_market");
    assert_route_ix(&built, "trading_client_cpmm");
    assert_funded_ok(
        "trading_client_cpmm_route",
        simulate_built_trade(&client, payer.as_ref(), built),
    );
}

#[test]
fn mainnet_router_cpmm_sell_route_simulate() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let pool = load_cpmm_pool(&client, &fixtures::WSOL_STONK_CPMM)
        .expect("load WSOL_STONK_CPMM");
    let meme = pool.meme_mint();
    let meme_tp = pool.token_program_for(&meme).unwrap_or(TOKEN_PROGRAM);
    let buy_amt = 80_000u64;
    let wallet = create_wallet();
    let user = wallet.pubkey();
    let market = RoutedMarket::new(Market::CpmmOuter(pool.clone()));
    let router = router_for(&user);
    let buy = router
        .buy_with_opts(
            buy_amt,
            &market,
            TradeOpts::default()
                .buy_with_wsol()
                .with_ata(route_buy_ata()),
        )
        .expect("buy before sell");
    let expected = crate::quote::cpmm_out(&pool, buy_amt, pool.base_mint == WSOL_MINT)
        .unwrap_or(1)
        .max(1);
    let sell_amt = (expected / 5).max(1);
    let sell = router
        .sell_with_opts(sell_amt, &market, TradeOpts::default().sell_to_wsol())
        .expect("sell route");
    assert_route_ix(&sell, "router_cpmm_sell");
    // Buy credits meme ATA, then sell Route spends it — one funded simulate.
    let mut ixs = buy.into_instructions();
    if let Some(route) = sell.route {
        ixs.push(route);
    }
    ixs.extend(sell.cleanup);
    assert_funded_ok(
        "router_cpmm_buy_then_sell",
        crate::mainnet_sim::simulate_with_fresh_wallet(&client, &wallet, ixs),
    );
    let _ = (meme, meme_tp);
}

// ─── Broader Router DEX coverage ─────────────────────────────────────────────

#[test]
fn mainnet_router_fee_bps_encodes_and_simulates() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let pool = load_cpmm_pool(&client, &fixtures::WSOL_STONK_CPMM).expect("cpmm");
    let wallet = create_wallet();
    let user = wallet.pubkey();
    let fee_recv = fee_recipient();
    let fee_bps = 100u16;
    let amount_in = 100_000u64;
    let router =
        RouterClient::new(user, fee_recv, fee_bps).with_pool_guard(PoolGuardPolicy::disabled());
    let market = RoutedMarket::new(Market::CpmmOuter(pool));
    let built = router
        .buy_with_opts(
            amount_in,
            &market,
            TradeOpts::default()
                .buy_with_wsol()
                .with_ata(route_buy_ata()),
        )
        .expect("fee buy");
    assert_route_ix(&built, "router_fee_bps");
    let route = built.route.as_ref().expect("route");
    // On-chain fee: amount_in is the full budget; program skims fee_bps from it.
    let encoded_in = u64::from_le_bytes(route.data[1..9].try_into().unwrap());
    assert_eq!(encoded_in, amount_in);
    assert!(crate::quote::fee_amount(amount_in, fee_bps) > 0);
    let fee_ata = ata(&fee_recv, &WSOL_MINT, &TOKEN_PROGRAM);
    assert!(
        route.accounts.iter().any(|a| a.pubkey == fee_ata || a.pubkey == fee_recv),
        "fee recipient must appear in Route accounts"
    );
    assert_funded_ok(
        "router_fee_bps_route",
        simulate_built_trade(&client, &wallet, built),
    );
}

#[test]
fn mainnet_router_pool_guard_rejects_untrusted_amm() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let pool = load_cpmm_pool(&client, &fixtures::WSOL_STONK_CPMM).expect("cpmm");
    let wallet = create_wallet();
    let user = wallet.pubkey();
    let strict = RouterClient::new(user, fee_recipient(), 0)
        .with_pool_guard(PoolGuardPolicy::stonk_strict());
    let market = RoutedMarket::new(Market::CpmmOuter(pool.clone()));
    assert!(
        strict
            .buy_with_opts(50_000, &market, TradeOpts::default().buy_with_wsol())
            .is_err(),
        "stonk_strict must reject unlisted CPMM"
    );
    let trusted = RouterClient::new(user, fee_recipient(), 0).with_pool_guard(
        PoolGuardPolicy::stonk_strict().trust(pool.pool_state),
    );
    let built = trusted
        .buy_with_opts(
            50_000,
            &market,
            TradeOpts::default()
                .buy_with_wsol()
                .with_ata(route_buy_ata()),
        )
        .expect("trusted cpmm");
    assert_route_ix(&built, "router_pool_guard_trusted");
    assert_funded_ok(
        "router_pool_guard_trusted",
        simulate_built_trade(&client, &wallet, built),
    );
}

#[test]
fn mainnet_router_amm_v4_exact_out() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let pool = match crate::mainnet_sim::load_amm_v4_pool(&client, &fixtures::AMM_V4_WSOL_USDC) {
        Some(p) => p,
        None => {
            soft_coverage("router_amm_v4_exact_out", false);
            return;
        }
    };
    let max_in = 500_000u64;
    let out = crate::quote::raydium_amm_v4_out(&pool, max_in / 4, pool.coin_mint == WSOL_MINT)
        .unwrap_or(1)
        .max(1)
        .min(pool.pc_reserve.max(pool.coin_reserve) / 1_000_000)
        .max(1);
    let wallet = create_wallet();
    let user = wallet.pubkey();
    let market = RoutedMarket::raydium_amm_v4(pool.clone());
    let built = router_for(&user)
        .buy_with_opts(
            max_in,
            &market,
            TradeOpts::default()
                .buy_with_wsol()
                .with_fixed_output(out)
                .with_ata(route_buy_ata()),
        )
        .expect("amm_v4 exact-out");
    assert_route_ix(&built, "router_amm_v4_exact_out");
    assert_funded_ok(
        "router_amm_v4_exact_out_route",
        simulate_built_trade(&client, &wallet, built),
    );
}

#[test]
fn mainnet_router_pumpswap_buy_sell_roundtrip() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let pool = match load_pumpswap_pool(&client, &fixtures::PUMPSWAP_POOL)
        .or_else(|| load_pumpswap_pool(&client, &fixtures::PUMPSWAP_SEED_POOL))
    {
        Some(p) => p,
        None => {
            soft_coverage("router_ps_roundtrip", false);
            return;
        }
    };
    let buy_amt = 150_000u64;
    let wallet = create_wallet();
    let user = wallet.pubkey();
    let market = RoutedMarket::pumpswap(pool.clone());
    let router = router_for(&user);
    let buy = router
        .buy_with_opts(
            buy_amt,
            &market,
            TradeOpts::default()
                .buy_with_wsol()
                .with_ata(route_buy_ata()),
        )
        .expect("ps buy");
    assert_route_ix(&buy, "router_ps_rt_buy");
    let base_out = crate::quote::pumpswap_buy_base_out(&pool, buy_amt)
        .unwrap_or(1)
        .max(1);
    let sell_amt = (base_out / 4).max(1);
    let sell = router
        .sell_with_opts(sell_amt, &market, TradeOpts::default().sell_to_wsol())
        .expect("ps sell");
    assert_route_ix(&sell, "router_ps_rt_sell");
    let mut ixs = buy.into_instructions();
    if let Some(route) = sell.route {
        ixs.push(route);
    }
    ixs.extend(sell.cleanup);
    assert_funded_ok(
        "router_ps_buy_sell_roundtrip",
        crate::mainnet_sim::simulate_with_fresh_wallet(&client, &wallet, ixs),
    );
}

#[test]
fn mainnet_router_pumpfun_from_live_events() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let found = scan_events(&client, &[PUMPFUN_PROGRAM], 100, |sig, ev| {
        let (DexEvent::PumpFunTrade(e)
        | DexEvent::PumpFunBuy(e)
        | DexEvent::PumpFunBuyExactSolIn(e)) = ev
        else {
            return false;
        };
        if !e.is_buy || e.mint == Pubkey::default() {
            return false;
        }
        if e.real_token_reserves == 0 || e.virtual_token_reserves == 0 {
            return false;
        }
        let pool = crate::parser::pumpfun_from_trade(e);
        if pool.mint == Pubkey::default() {
            return false;
        }
        let amount = e.sol_amount.max(50_000).min(1_000_000);
        let wallet = create_wallet();
        let user = wallet.pubkey();
        let market = RoutedMarket::new(Market::PumpFunInner(pool.clone()));
        let opts = if pool.uses_v2() && pool.is_native_sol_quote() {
            TradeOpts::default()
                .buy_with_wsol()
                .with_ata(route_buy_ata())
        } else if pool.is_native_sol_quote() {
            TradeOpts::default()
                .buy_with_sol()
                .with_ata(route_buy_ata())
        } else {
            return false;
        };
        let built = match router_for(&user).buy_with_opts(amount, &market, opts) {
            Ok(b) => b,
            Err(err) => {
                println!("[router_pumpfun] build err={err}");
                return false;
            }
        };
        assert_route_ix(&built, "router_pumpfun");
        println!("[router_pumpfun] sig={sig} wallet={user} v2={}", pool.uses_v2());
        assert_funded_ok(
            "router_pumpfun_route",
            simulate_built_trade(&client, &wallet, built),
        );
        true
    });
    soft_coverage("router_pumpfun", found);
}

#[test]
fn mainnet_router_damm_v2_from_live_events() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let found = scan_events(
        &client,
        &[fixtures::METEORA_DAMM_V2_POOL, METEORA_DAMM_V2_PROGRAM],
        60,
        |sig, ev| {
            let DexEvent::MeteoraDammV2Swap(e) = ev else {
                return false;
            };
            let Some(pool) = crate::parser::damm_v2_from_swap(e) else {
                return false;
            };
            if pool.swap_mode != 0 {
                return false;
            }
            if pool.token_a_mint != WSOL_MINT && pool.token_b_mint != WSOL_MINT {
                return false;
            }
            let amount = e.amount_in.max(50_000).min(1_000_000);
            // Bind quote to observed swap so concentrated_min_out works.
            let mut pool = pool;
            pool.quoted_amount_in = Some(amount);
            pool.expected_out = Some(e.output_amount.max(1));
            let wallet = create_wallet();
            let user = wallet.pubkey();
            let market = RoutedMarket::new(Market::MeteoraDammV2(pool));
            let built = match router_for(&user).buy_with_opts(
                amount,
                &market,
                TradeOpts::default()
                    .buy_with_wsol()
                    .with_ata(route_buy_ata()),
            ) {
                Ok(b) => b,
                Err(err) => {
                    println!("[router_damm_v2] build err={err}");
                    return false;
                }
            };
            assert_route_ix(&built, "router_damm_v2");
            println!("[router_damm_v2] sig={sig} wallet={user}");
            assert_funded_ok(
                "router_damm_v2_route",
                simulate_built_trade(&client, &wallet, built),
            );
            true
        },
    );
    soft_coverage("router_damm_v2", found);
}

#[test]
fn mainnet_router_clmm_from_live_events() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let found = scan_events(&client, &[RAYDIUM_CLMM_PROGRAM], 120, |sig, ev| {
        let DexEvent::RaydiumClmmSwap(e) = ev else {
            return false;
        };
        let Some(mut pool) = crate::parser::clmm_from_swap(e) else {
            return false;
        };
        // Accept any WSOL-paired pool; always spend WSOL for fresh-wallet funding.
        if pool.token_0_mint != WSOL_MINT && pool.token_1_mint != WSOL_MINT {
            return false;
        }
        if !crate::mainnet_sim::fill_clmm_token_programs(&client, &mut pool) {
            return false;
        }
        let amount = pool
            .quoted_amount_in
            .unwrap_or(e.amount_0.max(e.amount_1))
            .max(50_000)
            .min(1_000_000);
        if let (Some(qin), Some(qout)) = (pool.quoted_amount_in, pool.expected_out) {
            if qin > 0 && amount != qin {
                pool.expected_out =
                    Some(((qout as u128) * amount as u128 / qin as u128).max(1) as u64);
            }
        }
        pool.quoted_amount_in = Some(amount);
        pool.expected_out = pool.expected_out.or(Some(1));
        let wallet = create_wallet();
        let user = wallet.pubkey();
        let market = RoutedMarket::new(Market::RaydiumClmm(pool));
        let built = match router_for(&user).buy_with_opts(
            amount,
            &market,
            TradeOpts::default()
                .buy_with_wsol()
                .with_ata(route_buy_ata()),
        ) {
            Ok(b) => b,
            Err(err) => {
                println!("[router_clmm] build err={err}");
                return false;
            }
        };
        assert_route_ix(&built, "router_clmm");
        println!("[router_clmm] sig={sig} wallet={user}");
        assert_funded_ok(
            "router_clmm_route",
            simulate_built_trade(&client, &wallet, built),
        );
        true
    });
    soft_coverage("router_clmm", found);
}

#[test]
fn mainnet_router_whirlpool_from_live_events() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let found = scan_events(&client, &[ORCA_WHIRLPOOL_PROGRAM], 120, |sig, ev| {
        let DexEvent::OrcaWhirlpoolSwap(e) = ev else {
            return false;
        };
        let Some(mut pool) = crate::parser::whirlpool_from_swap(e) else {
            return false;
        };
        if pool.mint_a != WSOL_MINT && pool.mint_b != WSOL_MINT {
            return false;
        }
        if !crate::mainnet_sim::fill_whirlpool_token_programs(&client, &mut pool) {
            return false;
        }
        let amount = e.input_amount.max(50_000).min(1_000_000);
        if let (Some(qin), Some(qout)) = (pool.quoted_amount_in, pool.expected_out) {
            if qin > 0 && amount != qin {
                pool.expected_out =
                    Some(((qout as u128) * amount as u128 / qin as u128).max(1) as u64);
            }
        }
        pool.quoted_amount_in = Some(amount);
        pool.expected_out = pool.expected_out.or(Some(e.output_amount.max(1)));
        let wallet = create_wallet();
        let user = wallet.pubkey();
        let market = RoutedMarket::new(Market::Whirlpool(pool));
        let built = match router_for(&user).buy_with_opts(
            amount,
            &market,
            TradeOpts::default()
                .buy_with_wsol()
                .with_ata(route_buy_ata()),
        ) {
            Ok(b) => b,
            Err(err) => {
                println!("[router_whirlpool] build err={err}");
                return false;
            }
        };
        assert_route_ix(&built, "router_whirlpool");
        println!("[router_whirlpool] sig={sig} wallet={user}");
        assert_funded_ok(
            "router_whirlpool_route",
            simulate_built_trade(&client, &wallet, built),
        );
        true
    });
    soft_coverage("router_whirlpool", found);
}

#[test]
fn mainnet_router_dlmm_from_live_events() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let found = scan_events(&client, &[METEORA_DLMM_PROGRAM], 50, |sig, ev| {
        let DexEvent::MeteoraDlmmSwap(e) = ev else {
            return false;
        };
        let Some(mut pool) = crate::parser::dlmm_from_swap(e) else {
            return false;
        };
        let input = if e.swap_for_y {
            pool.token_x_mint
        } else {
            pool.token_y_mint
        };
        if input != WSOL_MINT {
            return false;
        }
        let amount = e.amount_in.max(50_000).min(1_000_000);
        pool.quoted_amount_in = Some(amount);
        pool.expected_out = Some(e.amount_out.max(1));
        let wallet = create_wallet();
        let user = wallet.pubkey();
        let market = RoutedMarket::new(Market::MeteoraDlmm(pool));
        let built = match router_for(&user).buy_with_opts(
            amount,
            &market,
            TradeOpts::default()
                .buy_with_wsol()
                .with_ata(route_buy_ata()),
        ) {
            Ok(b) => b,
            Err(err) => {
                println!("[router_dlmm] build err={err}");
                return false;
            }
        };
        assert_route_ix(&built, "router_dlmm");
        println!("[router_dlmm] sig={sig} wallet={user}");
        assert_funded_ok(
            "router_dlmm_route",
            simulate_built_trade(&client, &wallet, built),
        );
        true
    });
    soft_coverage("router_dlmm", found);
}

#[test]
fn mainnet_router_adapter_amm_v4_params() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let Some(pool) = crate::mainnet_sim::load_amm_v4_pool(&client, &fixtures::AMM_V4_WSOL_USDT)
    else {
        soft_coverage("adapter_amm_v4", false);
        return;
    };
    let params = sol_trade_sdk::trading::core::params::RaydiumAmmV4Params {
        amm: pool.amm,
        coin_mint: pool.coin_mint,
        pc_mint: pool.pc_mint,
        token_coin: pool.token_coin,
        token_pc: pool.token_pc,
        amm_open_orders: pool.amm_open_orders,
        amm_target_orders: pool.amm_target_orders,
        serum_program: pool.serum_program,
        serum_market: pool.serum_market,
        serum_bids: pool.serum_bids,
        serum_asks: pool.serum_asks,
        serum_event_queue: pool.serum_event_queue,
        serum_coin_vault_account: pool.serum_coin_vault_account,
        serum_pc_vault_account: pool.serum_pc_vault_account,
        serum_vault_signer: pool.serum_vault_signer,
        coin_reserve: pool.coin_reserve,
        pc_reserve: pool.pc_reserve,
    };
    let wallet = create_wallet();
    let user = wallet.pubkey();
    let mint = if pool.coin_mint == WSOL_MINT {
        pool.pc_mint
    } else {
        pool.coin_mint
    };
    let market =
        to_routed_market_for_user(&DexParamEnum::RaydiumAmmV4(params), mint, &user)
            .expect("adapter amm_v4");
    let built = router_for(&user)
        .buy_with_opts(
            100_000,
            &market,
            TradeOpts::default()
                .buy_with_wsol()
                .with_ata(route_buy_ata()),
        )
        .expect("adapter amm buy");
    assert_route_ix(&built, "adapter_amm_v4");
    assert_funded_ok(
        "adapter_amm_v4_route",
        simulate_built_trade(&client, &wallet, built),
    );
}

#[test]
fn mainnet_router_load_market_by_rpc_cpmm() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let wallet = create_wallet();
    let user = wallet.pubkey();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("rt");
    let async_rpc = sol_trade_sdk::common::SolanaRpcClient::new_with_commitment(
        crate::mainnet_sim::rpc_url(),
        solana_commitment_config::CommitmentConfig::confirmed(),
    );
    let result = rt.block_on(async {
        crate::adapter::load_routed_market_by_rpc(
            &async_rpc,
            crate::adapter::LoadMarketRequest::RaydiumCpmm {
                pool: fixtures::WSOL_STONK_CPMM,
            },
            &user,
        )
        .await
    });
    let (_ext, market) = match result {
        Ok(v) => v,
        Err(err) => {
            println!("[load_by_rpc_cpmm] soft skip: {err}");
            soft_coverage("load_by_rpc_cpmm", false);
            return;
        }
    };
    let built = router_for(&user)
        .buy_with_opts(
            50_000,
            &market,
            TradeOpts::default()
                .buy_with_wsol()
                .with_ata(route_buy_ata()),
        )
        .expect("rpc-loaded cpmm buy");
    assert_route_ix(&built, "load_by_rpc_cpmm");
    assert_funded_ok(
        "load_by_rpc_cpmm_route",
        simulate_built_trade(&client, &wallet, built),
    );
}

#[test]
fn mainnet_router_trading_client_sell_build() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let pool = load_cpmm_pool(&client, &fixtures::WSOL_CARDS_CPMM).expect("cards");
    let wallet = create_wallet();
    let payer = std::sync::Arc::new(wallet.insecure_clone());
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("rt");
    let trading = rt.block_on(async {
        let trade_cfg = sol_trade_sdk::common::TradeConfig::new(
            crate::mainnet_sim::rpc_url(),
            vec![],
            solana_commitment_config::CommitmentConfig::confirmed(),
        );
        let cfg = crate::client::RouterTradeConfig::new(trade_cfg, fee_recipient(), 0)
            .with_pool_guard(PoolGuardPolicy::disabled());
        crate::client::TradingClient::new(payer.clone(), cfg).await
    });
    let market = RoutedMarket::new(Market::CpmmOuter(pool));
    let sell = trading
        .build_sell_from_market(
            1_000,
            &market,
            TradeOpts::default().sell_to_wsol(),
        )
        .expect("TradingClient sell");
    assert_route_ix(&sell, "trading_client_sell");
    // Sell alone soft-fails without meme balance — accepted Soft.
    assert_funded_ok(
        "trading_client_sell_route",
        simulate_built_trade(&client, payer.as_ref(), sell),
    );
}

#[test]
fn mainnet_router_prepare_atas_then_minimal_route() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let pool = load_cpmm_pool(&client, &fixtures::WSOL_STONK_CPMM).expect("stonk");
    let wallet = create_wallet();
    let user = wallet.pubkey();
    let market = RoutedMarket::new(Market::CpmmOuter(pool.clone()));
    let router = router_for(&user);
    let mut prep = router.prepare_buy_atas(&market, crate::asset::BuyWith::Wsol);
    prep.extend(wrap_sol(&user, 80_000));
    let built = router
        .buy_with_opts(
            80_000,
            &market,
            TradeOpts::default()
                .buy_with_wsol()
                .with_ata(AtaPolicy::none().with_create_meme(true)),
        )
        .expect("minimal ata buy");
    assert_route_ix(&built, "router_prep_atas");
    let mut ixs = prep;
    ixs.extend(built.into_instructions());
    assert_funded_ok(
        "router_prep_atas_route",
        crate::mainnet_sim::simulate_with_fresh_wallet(&client, &wallet, ixs),
    );
}

#[test]
fn mainnet_router_fault_bad_route_program_is_soft_or_hard() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let pool = load_cpmm_pool(&client, &fixtures::WSOL_STONK_CPMM).expect("stonk");
    let wallet = create_wallet();
    let user = wallet.pubkey();
    let market = RoutedMarket::new(Market::CpmmOuter(pool));
    let mut built = router_for(&user)
        .buy_with_opts(
            50_000,
            &market,
            TradeOpts::default()
                .buy_with_wsol()
                .with_ata(route_buy_ata()),
        )
        .expect("build");
    if let Some(route) = built.route.as_mut() {
        // Point Route at System Program — must not execute as Route CPI.
        route.program_id = Pubkey::default(); // invalid / non-router program
    }
    // Soft (wrong program / undeployed semantics) or Hard (invalid instruction) both prove detection.
    match crate::mainnet_sim::simulate_with_fresh_wallet(
        &client,
        &wallet,
        built.into_instructions(),
    ) {
        None => panic!("funder required"),
        Some(SimVerdict::Ok) => panic!("mutated route program must not succeed"),
        Some(SimVerdict::Soft(m)) | Some(SimVerdict::Hard(m)) => {
            println!("[router_fault_bad_program] rejected as expected: {m}");
        }
    }
}

// ─── Broad router coverage wave ──────────────────────────────────────────────

#[test]
fn mainnet_router_launchlab_from_curve_events() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let found = scan_events(
        &client,
        &[fixtures::CURVE_POOL, fixtures::GRAD_POOL, LAUNCHLAB_PROGRAM],
        100,
        |sig, ev| {
            let DexEvent::RaydiumLaunchlabTrade(e) = ev else {
                return false;
            };
            if !e.is_buy {
                return false;
            }
            let Some(pool) = crate::parser::launchlab_from_trade(e) else {
                return false;
            };
            let amount = e.amount_in.max(50_000).min(500_000);
            let wallet = create_wallet();
            let user = wallet.pubkey();
            let market = RoutedMarket::new(Market::LaunchLabInner(pool.clone()));
            let opts = if pool.is_sol_quote() {
                TradeOpts::default()
                    .buy_with_sol()
                    .with_ata(route_buy_ata())
            } else if pool.quote_mint == WSOL_MINT {
                TradeOpts::default()
                    .buy_with_wsol()
                    .with_ata(route_buy_ata())
            } else {
                return false;
            };
            let built = match router_for(&user).buy_with_opts(amount, &market, opts) {
                Ok(b) => b,
                Err(err) => {
                    println!("[router_launchlab] build err={err}");
                    return false;
                }
            };
            assert_route_ix(&built, "router_launchlab");
            println!("[router_launchlab] sig={sig} wallet={user}");
            assert_funded_ok(
                "router_launchlab_route",
                simulate_built_trade(&client, &wallet, built),
            );
            true
        },
    );
    soft_coverage("router_launchlab", found);
}

#[test]
fn mainnet_router_pumpfun_sell_soft() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let found = scan_events(&client, &[PUMPFUN_PROGRAM], 50, |sig, ev| {
        let (DexEvent::PumpFunTrade(e) | DexEvent::PumpFunSell(e)) = ev else {
            return false;
        };
        if e.is_buy || e.mint == Pubkey::default() {
            return false;
        }
        let pool = crate::parser::pumpfun_from_trade(e);
        if pool.uses_v2() || !pool.is_native_sol_quote() {
            return false;
        }
        let wallet = create_wallet();
        let user = wallet.pubkey();
        let market = RoutedMarket::new(Market::PumpFunInner(pool));
        let built = match router_for(&user).sell_to_sol(e.token_amount.max(1), &market) {
            Ok(b) => b,
            Err(err) => {
                println!("[router_pumpfun_sell] build err={err}");
                return false;
            }
        };
        assert_route_ix(&built, "router_pumpfun_sell");
        println!("[router_pumpfun_sell] sig={sig} wallet={user} (expect soft)");
        assert_funded_ok(
            "router_pumpfun_sell",
            simulate_built_trade(&client, &wallet, built),
        );
        true
    });
    soft_coverage("router_pumpfun_sell", found);
}

#[test]
fn mainnet_router_pumpswap_sell_soft() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let Some(pool) = load_pumpswap_pool(&client, &fixtures::PUMPSWAP_SEED_POOL)
        .or_else(|| load_pumpswap_pool(&client, &fixtures::PUMPSWAP_POOL))
    else {
        soft_coverage("router_pumpswap_sell", false);
        return;
    };
    if pool.quote_mint != WSOL_MINT {
        soft_coverage("router_pumpswap_sell", false);
        return;
    }
    let wallet = create_wallet();
    let user = wallet.pubkey();
    let market = RoutedMarket::pumpswap(pool);
    let built = match router_for(&user).sell_to_wsol(1_000, &market) {
        Ok(b) => b,
        Err(err) => {
            println!("[router_pumpswap_sell] build err={err}");
            soft_coverage("router_pumpswap_sell", false);
            return;
        }
    };
    assert_route_ix(&built, "router_pumpswap_sell");
    println!("[router_pumpswap_sell] wallet={user} (expect soft: no base)");
    assert_funded_ok(
        "router_pumpswap_sell",
        simulate_built_trade(&client, &wallet, built),
    );
}

#[test]
fn mainnet_router_load_market_by_rpc_amm_v4_and_damm() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let wallet = create_wallet();
    let user = wallet.pubkey();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("rt");
    let async_rpc = sol_trade_sdk::common::SolanaRpcClient::new_with_commitment(
        crate::mainnet_sim::rpc_url(),
        solana_commitment_config::CommitmentConfig::confirmed(),
    );

    // AMM V4
    let amm_ok = {
        let result = rt.block_on(async {
            crate::adapter::load_routed_market_by_rpc(
                &async_rpc,
                crate::adapter::LoadMarketRequest::RaydiumAmmV4 {
                    amm: fixtures::AMM_V4_WSOL_USDC,
                },
                &user,
            )
            .await
        });
        match result {
            Ok((_ext, market)) => {
                let built = router_for(&user)
                    .buy_with_opts(
                        100_000,
                        &market,
                        TradeOpts::default()
                            .buy_with_wsol()
                            .with_ata(route_buy_ata()),
                    )
                    .expect("amm rpc buy");
                assert_route_ix(&built, "load_by_rpc_amm_v4");
                assert_funded_ok(
                    "load_by_rpc_amm_v4",
                    simulate_built_trade(&client, &wallet, built),
                );
                true
            }
            Err(err) => {
                println!("[load_by_rpc_amm_v4] soft: {err}");
                false
            }
        }
    };

    // DAMM V2
    let damm_ok = {
        let wallet2 = create_wallet();
        let user2 = wallet2.pubkey();
        let result = rt.block_on(async {
            crate::adapter::load_routed_market_by_rpc(
                &async_rpc,
                crate::adapter::LoadMarketRequest::MeteoraDammV2 {
                    pool: fixtures::METEORA_DAMM_V2_POOL,
                },
                &user2,
            )
            .await
        });
        match result {
            Ok((_ext, mut market)) => {
                // Bind a conservative quote for concentrated venues.
                if let Market::MeteoraDammV2(ref mut p) = market.market {
                    p.quoted_amount_in = Some(50_000);
                    p.expected_out = Some(1);
                }
                match router_for(&user2).buy_with_opts(
                    50_000,
                    &market,
                    TradeOpts::default()
                        .buy_with_wsol()
                        .with_ata(route_buy_ata()),
                ) {
                    Ok(built) => {
                        assert_route_ix(&built, "load_by_rpc_damm");
                        assert_funded_ok(
                            "load_by_rpc_damm",
                            simulate_built_trade(&client, &wallet2, built),
                        );
                        true
                    }
                    Err(err) => {
                        println!("[load_by_rpc_damm] build soft: {err}");
                        false
                    }
                }
            }
            Err(err) => {
                println!("[load_by_rpc_damm] soft: {err}");
                false
            }
        }
    };

    soft_coverage("load_by_rpc_amm_damm", amm_ok || damm_ok);
}

#[test]
fn mainnet_router_clmm_reverse_sell_soft() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let found = scan_events(&client, &[RAYDIUM_CLMM_PROGRAM], 120, |sig, ev| {
        let DexEvent::RaydiumClmmSwap(e) = ev else {
            return false;
        };
        let Some(mut pool) = crate::parser::clmm_from_swap(e) else {
            return false;
        };
        if pool.token_0_mint != WSOL_MINT && pool.token_1_mint != WSOL_MINT {
            return false;
        }
        if !crate::mainnet_sim::fill_clmm_token_programs(&client, &mut pool) {
            return false;
        }
        let amount = 1_000u64;
        pool.quoted_amount_in = Some(amount);
        pool.expected_out = Some(1);
        let wallet = create_wallet();
        let user = wallet.pubkey();
        let market = RoutedMarket::new(Market::RaydiumClmm(pool));
        let built = match router_for(&user).sell_to_wsol(amount, &market) {
            Ok(b) => b,
            Err(err) => {
                println!("[router_clmm_sell] build err={err}");
                return false;
            }
        };
        assert_route_ix(&built, "router_clmm_sell");
        println!("[router_clmm_sell] sig={sig} wallet={user} (expect soft)");
        assert_funded_ok(
            "router_clmm_sell",
            simulate_built_trade(&client, &wallet, built),
        );
        true
    });
    soft_coverage("router_clmm_sell", found);
}

#[test]
fn mainnet_router_cpmm_cards_direct_and_route() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let Some(pool) = load_cpmm_pool(&client, &fixtures::WSOL_CARDS_CPMM) else {
        soft_coverage("router_cards_dual", false);
        return;
    };
    let meme = pool.meme_mint();
    let meme_tp = pool.token_program_for(&meme).unwrap_or(TOKEN_PROGRAM);
    let amount = 75_000u64;

    // Direct legs
    {
        let wallet = create_wallet();
        let user = wallet.pubkey();
        let setup = setup_wsol_and_meme(&user, meme, meme_tp, amount);
        let leg = cpmm_swap_leg(
            &user,
            &pool,
            amount,
            0,
            WSOL_MINT,
            meme,
            ata(&user, &WSOL_MINT, &TOKEN_PROGRAM),
            ata(&user, &meme, &meme_tp),
        )
        .expect("direct");
        assert_funded_ok(
            "router_cards_direct",
            simulate_legs_funded(&client, &wallet, setup, &[leg]),
        );
    }
    // Route
    {
        let wallet = create_wallet();
        let user = wallet.pubkey();
        let market = RoutedMarket::new(Market::CpmmOuter(pool));
        let built = router_for(&user)
            .buy_with_opts(
                amount,
                &market,
                TradeOpts::default()
                    .buy_with_wsol()
                    .with_ata(route_buy_ata()),
            )
            .expect("route");
        assert_route_ix(&built, "router_cards_route");
        assert_funded_ok(
            "router_cards_route",
            simulate_built_trade(&client, &wallet, built),
        );
    }
}

// Silence unused import if TokenTransferFee only used via struct update elsewhere.
#[allow(dead_code)]
fn _token_transfer_fee_zero() -> TokenTransferFee {
    TokenTransferFee::default()
}
