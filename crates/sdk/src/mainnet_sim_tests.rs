//! Gated mainnet simulateTransaction suite.
//!
//! Every scenario:
//! 1. `create_wallet()` — ephemeral Keypair
//! 2. Virtually fund from a mainnet whale (`sigVerify=false`)
//! 3. Build DEX legs from live `sol-parser-sdk` events
//! 4. Simulate — soft (balance/slippage) OK, hard (layout) fails
//!
//! ```bash
//! RUN_MAINNET_SIM=1 cargo test -p sol-trade-router-sdk mainnet_sim -- --nocapture --test-threads=1
//! ```

#![cfg(test)]

use sol_parser_sdk::core::events::DexEvent;
use sol_parser_sdk::grpc::types::{EventType, EventTypeFilter};
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    instruction::Instruction,
    pubkey::Pubkey,
    signature::Signature,
    signer::Signer,
};

use crate::ata::{ata, create_ata, create_wsol_ata, wrap_sol};
use crate::constants::*;
use crate::legs::{
    cpmm_swap_leg, launchlab_buy_leg, meteora_damm_v2_swap_leg, meteora_dlmm_swap_leg,
    pumpfun_buy_leg, pumpfun_buy_v2_leg, pumpfun_sell_leg, pumpswap_buy_leg, pumpswap_sell_leg,
    raydium_amm_v4_swap_leg, raydium_clmm_swap_leg, whirlpool_swap_leg,
};
use crate::mainnet_sim::{
    assert_sim_hard, assert_sim_ok, create_wallet, enabled, fixtures, load_cpmm_pool,
    load_pumpswap_pool, mint_token_program_opt, require_coverage, require_rpc, rpc, scan_events,
    simulate_legs_funded, simulate_with_fresh_wallet, SimVerdict,
};
use crate::parser::{
    amm_v4_from_swap, clmm_from_swap, cpmm_from_swap, damm_v2_from_swap, dlmm_from_swap,
    launchlab_from_trade, market_from_dex_event, pumpfun_from_trade, pumpswap_from_buy,
    pumpswap_from_sell, whirlpool_from_swap,
};

fn setup_wsol_and_meme(
    wallet: &Pubkey,
    meme: Pubkey,
    meme_tp: Pubkey,
    wrap_lamports: u64,
) -> Vec<Instruction> {
    let mut ixs = vec![
        create_wsol_ata(wallet),
        create_ata(wallet, wallet, &meme, &meme_tp),
    ];
    ixs.extend(wrap_sol(wallet, wrap_lamports));
    ixs
}

fn setup_wsol(wallet: &Pubkey, wrap_lamports: u64) -> Vec<Instruction> {
    let mut ixs = vec![create_wsol_ata(wallet)];
    ixs.extend(wrap_sol(wallet, wrap_lamports));
    ixs
}

/// Deep-scan fixture + program addresses. Prefer fixtures so coverage is stable.
fn try_event_window(
    client: &RpcClient,
    addresses: &[Pubkey],
    _filter: EventTypeFilter,
    limit: usize,
    handle: impl FnMut(&Signature, &DexEvent) -> bool,
) -> bool {
    scan_events(client, addresses, limit, handle)
}

fn assert_funded_ok(scenario: &str, verdict: Option<SimVerdict>) {
    match verdict {
        None => panic!("[{scenario}] required funder / RPC for coverage, none available"),
        Some(v) => assert_sim_ok(scenario, v),
    }
}

fn assert_funded_hard(scenario: &str, verdict: Option<SimVerdict>) {
    match verdict {
        None => panic!("[{scenario}] required funder / RPC for coverage, none available"),
        Some(v) => assert_sim_hard(scenario, v),
    }
}

// ─── PumpFun ────────────────────────────────────────────────────────────────

#[test]
fn mainnet_sim_creates_fresh_wallet_each_run() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let a = create_wallet();
    let b = create_wallet();
    assert_ne!(a.pubkey(), b.pubkey());
    // Fund-only transfer after virtual whale debit — Ok or Soft.
    match simulate_with_fresh_wallet(&client, &a, vec![]) {
        None => panic!("[wallet] required funder for coverage, none available"),
        Some(SimVerdict::Ok | SimVerdict::Soft(_)) => {
            println!("[wallet] fund-only simulate accepted for {}", a.pubkey());
        }
        Some(SimVerdict::Hard(m)) => panic!("fund-only sim should not HARD: {m}"),
    }
}

#[test]
fn mainnet_sim_pumpfun_buy_v1() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let filter = EventTypeFilter::include_only(vec![EventType::PumpFunTrade]);
    let found = try_event_window(&client, &[PUMPFUN_PROGRAM], filter, 100, |sig, ev| {
        let (DexEvent::PumpFunTrade(e)
        | DexEvent::PumpFunBuy(e)
        | DexEvent::PumpFunBuyExactSolIn(e)) = ev
        else {
            return false;
        };
        let is_v2 = matches!(
            e.ix_name.as_str(),
            "buy_v2" | "sell_v2" | "buy_exact_quote_in_v2"
        );
        if is_v2 || e.mint == Pubkey::default() {
            return false;
        }
        let pool = pumpfun_from_trade(e);
        if pool.uses_v2() {
            return false;
        }
        let wallet = create_wallet();
        let user = wallet.pubkey();
        let wrap = e.sol_amount.max(50_000).min(5_000_000);
        let user_ata = ata(&user, &pool.mint, &pool.mint_token_program);
        let setup = setup_wsol_and_meme(&user, pool.mint, pool.mint_token_program, wrap);
        let leg = pumpfun_buy_leg(&user, &pool, wrap, 0, user_ata);
        println!(
            "[pumpfun_buy_v1] sig={sig} wallet={user} mint={}",
            pool.mint
        );
        assert_funded_ok("pumpfun_buy_v1", simulate_legs_funded(&client, &wallet, setup, &[leg]));
        true
    });
    require_coverage("pumpfun_buy_v1", found);
}

#[test]
fn mainnet_sim_pumpfun_buy_v2() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let filter = EventTypeFilter::include_only(vec![EventType::PumpFunTrade]);
    // Prefer native V2 trades; else force V2 leg on a live curve (layout coverage).
    let found = try_event_window(&client, &[PUMPFUN_PROGRAM], filter, 150, |sig, ev| {
        let (DexEvent::PumpFunTrade(e)
        | DexEvent::PumpFunBuy(e)
        | DexEvent::PumpFunBuyExactSolIn(e)) = ev
        else {
            return false;
        };
        if e.mint == Pubkey::default() {
            return false;
        }
        let mut pool = pumpfun_from_trade(e);
        pool.use_v2 = true;
        let wallet = create_wallet();
        let user = wallet.pubkey();
        let wrap = e.sol_amount.max(100_000).min(5_000_000);
        let setup = setup_wsol_and_meme(&user, pool.mint, pool.mint_token_program, wrap);
        let leg = pumpfun_buy_v2_leg(&user, &pool, wrap, 0);
        println!(
            "[pumpfun_buy_v2] sig={sig} wallet={user} mint={} forced_v2={}",
            pool.mint,
            !matches!(
                e.ix_name.as_str(),
                "buy_v2" | "sell_v2" | "buy_exact_quote_in_v2"
            )
        );
        assert_funded_ok(
            "pumpfun_buy_v2",
            simulate_legs_funded(&client, &wallet, setup, &[leg]),
        );
        true
    });
    require_coverage("pumpfun_buy_v2", found);
}

#[test]
fn mainnet_sim_pumpfun_sell_soft_without_balance() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let filter = EventTypeFilter::include_only(vec![EventType::PumpFunTrade]);
    let found = try_event_window(&client, &[PUMPFUN_PROGRAM], filter, 100, |sig, ev| {
        let (DexEvent::PumpFunTrade(e) | DexEvent::PumpFunSell(e)) = ev else {
            return false;
        };
        if e.is_buy || e.mint == Pubkey::default() {
            return false;
        }
        let pool = pumpfun_from_trade(e);
        let wallet = create_wallet();
        let user = wallet.pubkey();
        let user_ata = ata(&user, &pool.mint, &pool.mint_token_program);
        let setup = vec![create_ata(
            &user,
            &user,
            &pool.mint,
            &pool.mint_token_program,
        )];
        let leg = pumpfun_sell_leg(&user, &pool, e.token_amount.max(1), 0, user_ata);
        println!("[pumpfun_sell] sig={sig} wallet={user} (expect soft: no tokens)");
        assert_funded_ok(
            "pumpfun_sell",
            simulate_legs_funded(&client, &wallet, setup, &[leg]),
        );
        true
    });
    require_coverage("pumpfun_sell", found);
}

// ─── PumpSwap (PumpFun 外盘) ────────────────────────────────────────────────

#[test]
fn mainnet_sim_pumpswap_buy() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    // Prefer on-chain fixture snapshot — no recent-event dependency.
    if let Some(pool) = load_pumpswap_pool(&client, &fixtures::PUMPSWAP_POOL) {
        let wallet = create_wallet();
        let user = wallet.pubkey();
        let quote_in = 2_000_000u64;
        let setup = setup_wsol_and_meme(&user, pool.base_mint, pool.base_token_program, quote_in);
        let leg = pumpswap_buy_leg(&user, &pool, quote_in, 0).expect("pumpswap buy leg");
        println!(
            "[pumpswap_buy] account-load wallet={user} pool={}",
            pool.pool
        );
        assert_funded_ok(
            "pumpswap_buy",
            simulate_legs_funded(&client, &wallet, setup, &[leg]),
        );
        return;
    }
    let filter = EventTypeFilter::include_only(vec![EventType::PumpSwapTrade]);
    let found = try_event_window(
        &client,
        &[
            fixtures::PUMPSWAP_POOL,
            fixtures::PUMPSWAP_SEED_POOL,
            PUMPSWAP_PROGRAM,
        ],
        filter,
        80,
        |sig, ev| {
            let DexEvent::PumpSwapBuy(e) = ev else {
                return false;
            };
            if e.pool == Pubkey::default() || e.user == Pubkey::default() {
                return false;
            }
            let pool = pumpswap_from_buy(e);
            let wallet = create_wallet();
            let user = wallet.pubkey();
            let quote_in = e.quote_amount_in.max(50_000).min(2_000_000);
            let setup =
                setup_wsol_and_meme(&user, pool.base_mint, pool.base_token_program, quote_in);
            let Ok(leg) = pumpswap_buy_leg(&user, &pool, quote_in, 0) else {
                return false;
            };
            println!(
                "[pumpswap_buy] sig={sig} wallet={user} pool={}",
                pool.pool
            );
            assert_funded_ok(
                "pumpswap_buy",
                simulate_legs_funded(&client, &wallet, setup, &[leg]),
            );
            true
        },
    );
    require_coverage("pumpswap_buy", found);
}

#[test]
fn mainnet_sim_pumpswap_sell() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let filter = EventTypeFilter::include_only(vec![EventType::PumpSwapTrade]);
    let found = try_event_window(&client, &[fixtures::PUMPSWAP_POOL, fixtures::PUMPSWAP_SEED_POOL, PUMPSWAP_PROGRAM], filter, 80, |sig, ev| {
        let DexEvent::PumpSwapSell(e) = ev else {
            return false;
        };
        if e.pool == Pubkey::default() {
            return false;
        }
        let pool = pumpswap_from_sell(e);
        let wallet = create_wallet();
        let user = wallet.pubkey();
        let setup = vec![
            create_ata(&user, &user, &pool.base_mint, &pool.base_token_program),
            create_ata(&user, &user, &pool.quote_mint, &pool.quote_token_program),
        ];
        let Ok(leg) = pumpswap_sell_leg(&user, &pool, e.base_amount_in.max(1), 0) else {
            return false;
        };
        println!("[pumpswap_sell] sig={sig} wallet={user} (expect soft)");
        assert_funded_ok(
            "pumpswap_sell",
            simulate_legs_funded(&client, &wallet, setup, &[leg]),
        );
        true
    });
    require_coverage("pumpswap_sell", found);
}

// ─── LaunchLab / CPMM ───────────────────────────────────────────────────────

#[test]
fn mainnet_sim_launchlab_buy() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let filter = EventTypeFilter::include_only(vec![EventType::RaydiumLaunchlabTrade]);
    let found = try_event_window(&client, &[fixtures::CURVE_POOL, LAUNCHLAB_PROGRAM], filter, 100, |sig, ev| {
        let DexEvent::RaydiumLaunchlabTrade(e) = ev else {
            return false;
        };
        if !e.is_buy {
            return false;
        }
        let Some(pool) = launchlab_from_trade(e) else {
            return false;
        };
        let wallet = create_wallet();
        let user = wallet.pubkey();
        let amount = e.amount_in.max(50_000).min(2_000_000);
        let base_ata = ata(&user, &pool.base_mint, &pool.base_token_program);
        let quote_ata = ata(&user, &pool.quote_mint, &pool.quote_token_program);
        let mut setup = vec![
            create_ata(&user, &user, &pool.base_mint, &pool.base_token_program),
            create_ata(&user, &user, &pool.quote_mint, &pool.quote_token_program),
        ];
        if pool.is_sol_quote() {
            setup = setup_wsol_and_meme(&user, pool.base_mint, pool.base_token_program, amount);
        }
        let leg = launchlab_buy_leg(&user, &pool, amount, 0, base_ata, quote_ata);
        println!(
            "[launchlab_buy] sig={sig} wallet={user} pool={}",
            pool.pool_state
        );
        assert_funded_ok(
            "launchlab_buy",
            simulate_legs_funded(&client, &wallet, setup, &[leg]),
        );
        true
    });
    require_coverage("launchlab_buy", found);
}

#[test]
fn mainnet_sim_cpmm_swap() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    if let Some(pool) = load_cpmm_pool(&client, &fixtures::WSOL_STONK_CPMM) {
        let wallet = create_wallet();
        let user = wallet.pubkey();
        let amount = 100_000u64;
        let out = pool.meme_mint();
        let out_tp = pool.token_program_for(&out).unwrap_or(TOKEN_PROGRAM);
        let setup = setup_wsol_and_meme(&user, out, out_tp, amount);
        let leg = cpmm_swap_leg(
            &user,
            &pool,
            amount,
            0,
            WSOL_MINT,
            out,
            ata(&user, &WSOL_MINT, &TOKEN_PROGRAM),
            ata(&user, &out, &out_tp),
        )
        .expect("cpmm leg");
        println!(
            "[cpmm_swap] account-load wallet={user} pool={}",
            pool.pool_state
        );
        assert_funded_ok(
            "cpmm_swap",
            simulate_legs_funded(&client, &wallet, setup, &[leg]),
        );
        return;
    }
    let filter = EventTypeFilter::include_only(vec![EventType::RaydiumCpmmSwap]);
    let found = try_event_window(
        &client,
        &[
            fixtures::WSOL_STONK_CPMM,
            fixtures::WSOL_CARDS_CPMM,
            RAYDIUM_CPMM_PROGRAM,
        ],
        filter,
        80,
        |sig, ev| {
            let DexEvent::RaydiumCpmmSwap(e) = ev else {
                return false;
            };
            let Some(pool) = cpmm_from_swap(e) else {
                return false;
            };
            let wallet = create_wallet();
            let user = wallet.pubkey();
            let amount = e.input_amount.max(50_000).min(2_000_000);
            let in_mint = e.input_token_mint;
            let out_mint = e.output_token_mint;
            let in_tp = pool.token_program_for(&in_mint).unwrap_or(TOKEN_PROGRAM);
            let out_tp = pool.token_program_for(&out_mint).unwrap_or(TOKEN_PROGRAM);
            let mut setup = vec![
                create_ata(&user, &user, &in_mint, &in_tp),
                create_ata(&user, &user, &out_mint, &out_tp),
            ];
            if in_mint == WSOL_MINT {
                setup = setup_wsol_and_meme(&user, out_mint, out_tp, amount);
            }
            let Ok(leg) = cpmm_swap_leg(
                &user,
                &pool,
                amount,
                0,
                in_mint,
                out_mint,
                ata(&user, &in_mint, &in_tp),
                ata(&user, &out_mint, &out_tp),
            ) else {
                return false;
            };
            println!("[cpmm_swap] sig={sig} wallet={user} pool={}", pool.pool_state);
            assert_funded_ok(
                "cpmm_swap",
                simulate_legs_funded(&client, &wallet, setup, &[leg]),
            );
            true
        },
    );
    require_coverage("cpmm_swap", found);
}

// ─── Raydium AMM V4 / CLMM / Whirlpool / DLMM / DAMM ─────────────────────────

#[test]
fn mainnet_sim_raydium_amm_v4() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let filter = EventTypeFilter::include_only(vec![EventType::RaydiumAmmV4Swap]);
    let found = try_event_window(&client, &[fixtures::AMM_V4_WSOL_USDC, fixtures::AMM_V4_WSOL_USDT, RAYDIUM_AMM_V4_PROGRAM], filter, 100, |sig, ev| {
        let DexEvent::RaydiumAmmV4Swap(e) = ev else {
            return false;
        };
        let Some(mut pool) = amm_v4_from_swap(e) else {
            return false;
        };
        // Prefer SOL-paired swaps for fresh-wallet funding.
        if pool.coin_mint == Pubkey::default() {
            pool.coin_mint = WSOL_MINT; // best-effort; soft if wrong
        }
        let wallet = create_wallet();
        let user = wallet.pubkey();
        let amount = e.amount_in.max(50_000).min(2_000_000);
        let input = if pool.pc_mint == WSOL_MINT || pool.coin_mint == WSOL_MINT {
            WSOL_MINT
        } else {
            return false;
        };
        let setup = setup_wsol(&user, amount);
        let Ok(leg) = raydium_amm_v4_swap_leg(&user, &pool, amount, 0, input) else {
            return false;
        };
        println!("[raydium_amm_v4] sig={sig} wallet={user} amm={}", pool.amm);
        assert_funded_ok(
            "raydium_amm_v4",
            simulate_legs_funded(&client, &wallet, setup, &[leg]),
        );
        true
    });
    require_coverage("raydium_amm_v4", found);
}

#[test]
fn mainnet_sim_raydium_clmm() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let filter = EventTypeFilter::include_only(vec![EventType::RaydiumClmmSwap]);
    let found = try_event_window(&client, &[RAYDIUM_CLMM_PROGRAM], filter, 120, |sig, ev| {
        let DexEvent::RaydiumClmmSwap(e) = ev else {
            return false;
        };
        let Some(mut pool) = clmm_from_swap(e) else {
            return false;
        };
        let input = e.input_mint;
        let output = e.output_mint;
        // Fresh wallets are SOL-funded; only simulate WSOL→meme CLMM legs.
        if input != WSOL_MINT {
            return false;
        }
        let Some(out_tp) = mint_token_program_opt(&client, &output) else {
            return false;
        };
        let in_tp = TOKEN_PROGRAM;
        if input == pool.token_0_mint {
            pool.token_0_program = in_tp;
            pool.token_1_program = out_tp;
        } else {
            pool.token_0_program = out_tp;
            pool.token_1_program = in_tp;
        }
        let wallet = create_wallet();
        let user = wallet.pubkey();
        let amount = if e.zero_for_one {
            e.amount_0
        } else {
            e.amount_1
        }
        .max(50_000)
        .min(2_000_000);
        let setup = setup_wsol_and_meme(&user, output, out_tp, amount);
        let Ok(leg) = raydium_clmm_swap_leg(&user, &pool, amount, 0, input) else {
            return false;
        };
        println!(
            "[raydium_clmm] sig={sig} wallet={user} pool={} out_tp={out_tp}",
            pool.pool_state
        );
        assert_funded_ok(
            "raydium_clmm",
            simulate_legs_funded(&client, &wallet, setup, &[leg]),
        );
        true
    });
    require_coverage("raydium_clmm", found);
}

#[test]
fn mainnet_sim_orca_whirlpool() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let filter = EventTypeFilter::include_only(vec![EventType::OrcaWhirlpoolSwap]);
    let found = try_event_window(&client, &[ORCA_WHIRLPOOL_PROGRAM], filter, 120, |sig, ev| {
        let DexEvent::OrcaWhirlpoolSwap(e) = ev else {
            return false;
        };
        let Some(mut pool) = whirlpool_from_swap(e) else {
            return false;
        };
        let input = if e.a_to_b { pool.mint_a } else { pool.mint_b };
        let output = if e.a_to_b { pool.mint_b } else { pool.mint_a };
        if input != WSOL_MINT {
            return false;
        }
        let Some(out_tp) = mint_token_program_opt(&client, &output) else {
            return false;
        };
        pool.token_program_a = if pool.mint_a == WSOL_MINT {
            TOKEN_PROGRAM
        } else {
            out_tp
        };
        pool.token_program_b = if pool.mint_b == WSOL_MINT {
            TOKEN_PROGRAM
        } else {
            out_tp
        };
        let wallet = create_wallet();
        let user = wallet.pubkey();
        let amount = e.input_amount.max(50_000).min(2_000_000);
        let setup = setup_wsol_and_meme(&user, output, out_tp, amount);
        let Ok(leg) = whirlpool_swap_leg(&user, &pool, amount, 0, input) else {
            return false;
        };
        println!(
            "[orca_whirlpool] sig={sig} wallet={user} pool={}",
            pool.whirlpool
        );
        assert_funded_ok(
            "orca_whirlpool",
            simulate_legs_funded(&client, &wallet, setup, &[leg]),
        );
        true
    });
    require_coverage("orca_whirlpool", found);
}

#[test]
fn mainnet_sim_meteora_dlmm() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let filter = EventTypeFilter::include_only(vec![EventType::MeteoraDlmmSwap]);
    let found = try_event_window(&client, &[METEORA_DLMM_PROGRAM], filter, 120, |sig, ev| {
        let DexEvent::MeteoraDlmmSwap(e) = ev else {
            return false;
        };
        let Some(pool) = dlmm_from_swap(e) else {
            return false;
        };
        let wallet = create_wallet();
        let user = wallet.pubkey();
        let amount = e.amount_in.max(50_000).min(2_000_000);
        let input = if e.swap_for_y {
            pool.token_x_mint
        } else {
            pool.token_y_mint
        };
        let output = if e.swap_for_y {
            pool.token_y_mint
        } else {
            pool.token_x_mint
        };
        let in_tp = if e.swap_for_y {
            pool.token_x_program
        } else {
            pool.token_y_program
        };
        let out_tp = if e.swap_for_y {
            pool.token_y_program
        } else {
            pool.token_x_program
        };
        let mut setup = vec![
            create_ata(&user, &user, &input, &in_tp),
            create_ata(&user, &user, &output, &out_tp),
        ];
        if input == WSOL_MINT {
            setup = setup_wsol_and_meme(&user, output, out_tp, amount);
        }
        let Ok(leg) = meteora_dlmm_swap_leg(&user, &pool, amount, 0, input) else {
            return false;
        };
        println!(
            "[meteora_dlmm] sig={sig} wallet={user} pool={}",
            pool.lb_pair
        );
        assert_funded_ok(
            "meteora_dlmm",
            simulate_legs_funded(&client, &wallet, setup, &[leg]),
        );
        true
    });
    require_coverage("meteora_dlmm", found);
}

#[test]
fn mainnet_sim_meteora_damm_v2() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let filter = EventTypeFilter::include_only(vec![EventType::MeteoraDammV2Swap]);
    let found = try_event_window(&client, &[fixtures::METEORA_DAMM_V2_POOL, METEORA_DAMM_V2_PROGRAM], filter, 100, |sig, ev| {
        let DexEvent::MeteoraDammV2Swap(e) = ev else {
            return false;
        };
        let Some(pool) = damm_v2_from_swap(e) else {
            return false;
        };
        if pool.swap_mode != 0 {
            return false;
        }
        let wallet = create_wallet();
        let user = wallet.pubkey();
        let amount = e.amount_in.max(50_000).min(2_000_000);
        let input = e.token_a_mint; // best-effort; soft if direction wrong
        let setup = if input == WSOL_MINT || pool.token_a_mint == WSOL_MINT || pool.token_b_mint == WSOL_MINT
        {
            let out = if pool.token_a_mint == WSOL_MINT {
                pool.token_b_mint
            } else {
                pool.token_a_mint
            };
            let out_tp = if pool.token_a_mint == WSOL_MINT {
                pool.token_b_program
            } else {
                pool.token_a_program
            };
            setup_wsol_and_meme(&user, out, out_tp, amount)
        } else {
            vec![
                create_ata(&user, &user, &pool.token_a_mint, &pool.token_a_program),
                create_ata(&user, &user, &pool.token_b_mint, &pool.token_b_program),
            ]
        };
        let in_mint = if pool.token_a_mint == WSOL_MINT || pool.token_b_mint == WSOL_MINT {
            WSOL_MINT
        } else {
            input
        };
        let Ok(leg) = meteora_damm_v2_swap_leg(&user, &pool, amount, 0, in_mint) else {
            return false;
        };
        println!(
            "[meteora_damm_v2] sig={sig} wallet={user} pool={}",
            pool.pool
        );
        assert_funded_ok(
            "meteora_damm_v2",
            simulate_legs_funded(&client, &wallet, setup, &[leg]),
        );
        true
    });
    require_coverage("meteora_damm_v2", found);
}

// ─── Fault injection (layout must HARD) ─────────────────────────────────────

#[test]
fn mainnet_fault_wrong_discriminator_is_hard() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let pool = load_cpmm_pool(&client, &fixtures::WSOL_STONK_CPMM)
        .expect("cpmm fixture for fault proof");
    let wallet = create_wallet();
    let user = wallet.pubkey();
    let amount = 100_000u64;
    let setup = setup_wsol_and_meme(
        &user,
        pool.meme_mint(),
        pool.token_program_for(&pool.meme_mint()).unwrap_or(TOKEN_PROGRAM),
        amount,
    );
    let mut leg = cpmm_swap_leg(
        &user,
        &pool,
        amount,
        0,
        WSOL_MINT,
        pool.meme_mint(),
        ata(&user, &WSOL_MINT, &TOKEN_PROGRAM),
        ata(
            &user,
            &pool.meme_mint(),
            &pool.token_program_for(&pool.meme_mint()).unwrap_or(TOKEN_PROGRAM),
        ),
    )
    .expect("leg");
    if leg.data.len() >= 8 {
        leg.data[..8].fill(0xff);
    }
    println!("[fault_wrong_disc] account-load wallet={user}");
    assert_funded_hard(
        "fault_wrong_disc",
        simulate_legs_funded(&client, &wallet, setup, &[leg]),
    );
}

#[test]
fn mainnet_fault_wrong_pool_account_is_hard() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let pool = load_pumpswap_pool(&client, &fixtures::PUMPSWAP_POOL)
        .expect("pumpswap fixture for fault proof");
    let wallet = create_wallet();
    let user = wallet.pubkey();
    let amount = 100_000u64;
    let setup = setup_wsol_and_meme(&user, pool.base_mint, pool.base_token_program, amount);
    let mut leg = pumpswap_buy_leg(&user, &pool, amount, 0).expect("leg");
    if !leg.accounts.is_empty() {
        leg.accounts[0].pubkey = Pubkey::new_unique();
    }
    println!("[fault_pumpswap_pool] account-load wallet={user}");
    assert_funded_hard(
        "fault_pumpswap_pool",
        simulate_legs_funded(&client, &wallet, setup, &[leg]),
    );
}

// ─── Parser smoke on live events ────────────────────────────────────────────

#[test]
fn mainnet_parser_market_from_dex_event_coverage() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let mut hits = 0usize;
    let programs = [
        PUMPFUN_PROGRAM,
        PUMPSWAP_PROGRAM,
        LAUNCHLAB_PROGRAM,
        RAYDIUM_CPMM_PROGRAM,
        RAYDIUM_CLMM_PROGRAM,
        ORCA_WHIRLPOOL_PROGRAM,
        METEORA_DLMM_PROGRAM,
        METEORA_DAMM_V2_PROGRAM,
        RAYDIUM_AMM_V4_PROGRAM,
        fixtures::PUMPSWAP_POOL,
        fixtures::WSOL_STONK_CPMM,
        fixtures::AMM_V4_WSOL_USDC,
    ];
    for program in programs {
        let found = scan_events(&client, &[program], 40, |sig, ev| {
            if market_from_dex_event(ev).is_some() {
                hits += 1;
                println!("[parser] sig={sig} → Market");
                true
            } else {
                false
            }
        });
        if found {
            continue;
        }
    }
    println!("[parser] market_from_dex_event hits={hits}");
    require_coverage("parser", hits > 0);
}

// ─── Fixture-address sims (stable pools; scan that address's recent txs) ─────

#[test]
fn mainnet_fixture_pumpswap_pool_buy() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let pool = load_pumpswap_pool(&client, &fixtures::PUMPSWAP_POOL)
        .expect("fixture PumpSwap pool must load from account");
    assert_eq!(pool.base_mint, fixtures::PUMPSWAP_BASE);
    let wallet = create_wallet();
    let user = wallet.pubkey();
    let quote_in = 2_000_000u64;
    let setup = setup_wsol_and_meme(&user, pool.base_mint, pool.base_token_program, quote_in);
    let leg = pumpswap_buy_leg(&user, &pool, quote_in, 0).expect("leg");
    println!("[fixture_pumpswap] account-load wallet={user}");
    assert_funded_ok(
        "fixture_pumpswap",
        simulate_legs_funded(&client, &wallet, setup, &[leg]),
    );
}

#[test]
fn mainnet_fixture_cpmm_wsol_stonk() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let pool = load_cpmm_pool(&client, &fixtures::WSOL_STONK_CPMM)
        .expect("fixture CPMM pool must load from account");
    let wallet = create_wallet();
    let user = wallet.pubkey();
    let amount = 100_000u64;
    let out = pool.meme_mint();
    let out_tp = pool.token_program_for(&out).unwrap_or(TOKEN_PROGRAM);
    let setup = setup_wsol_and_meme(&user, out, out_tp, amount);
    let leg = cpmm_swap_leg(
        &user,
        &pool,
        amount,
        0,
        WSOL_MINT,
        out,
        ata(&user, &WSOL_MINT, &TOKEN_PROGRAM),
        ata(&user, &out, &out_tp),
    )
    .expect("leg");
    println!("[fixture_cpmm] account-load wallet={user}");
    assert_funded_ok(
        "fixture_cpmm",
        simulate_legs_funded(&client, &wallet, setup, &[leg]),
    );
}

#[test]
fn mainnet_fixture_meteora_damm_v2() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let filter = EventTypeFilter::include_only(vec![EventType::MeteoraDammV2Swap]);
    let found = try_event_window(
        &client,
        &[fixtures::METEORA_DAMM_V2_POOL, METEORA_DAMM_V2_PROGRAM],
        filter,
        120,
        |sig, ev| {
            let DexEvent::MeteoraDammV2Swap(e) = ev else {
                return false;
            };
            let Some(pool) = damm_v2_from_swap(e) else {
                return false;
            };
            if pool.swap_mode != 0 {
                return false;
            }
            let wallet = create_wallet();
            let user = wallet.pubkey();
            let amount = e.amount_in.max(50_000).min(2_000_000);
            let in_mint = if pool.token_a_mint == WSOL_MINT || pool.token_b_mint == WSOL_MINT {
                WSOL_MINT
            } else {
                pool.token_a_mint
            };
            let setup = if in_mint == WSOL_MINT {
                let out = if pool.token_a_mint == WSOL_MINT {
                    pool.token_b_mint
                } else {
                    pool.token_a_mint
                };
                let out_tp = if pool.token_a_mint == WSOL_MINT {
                    pool.token_b_program
                } else {
                    pool.token_a_program
                };
                setup_wsol_and_meme(&user, out, out_tp, amount)
            } else {
                vec![
                    create_ata(&user, &user, &pool.token_a_mint, &pool.token_a_program),
                    create_ata(&user, &user, &pool.token_b_mint, &pool.token_b_program),
                ]
            };
            let Ok(leg) = meteora_damm_v2_swap_leg(&user, &pool, amount, 0, in_mint) else {
                return false;
            };
            println!("[fixture_damm_v2] sig={sig} wallet={user}");
            assert_funded_ok(
                "fixture_damm_v2",
                simulate_legs_funded(&client, &wallet, setup, &[leg]),
            );
            true
        },
    );
    require_coverage("fixture_damm_v2", found);
}
