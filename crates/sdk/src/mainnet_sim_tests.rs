//! Gated mainnet simulateTransaction suite.
//!
//! Every scenario:
//! 1. `create_wallet()` / `create_wallets(n)` — ephemeral Keypair(s)
//! 2. Virtually fund from a mainnet whale (`sigVerify=false`)
//! 3. Build DEX legs from live `sol-parser-sdk` events **or** RouterClient Route ix
//! 4. Simulate — soft (balance/slippage/undeployed router) OK, hard (layout) fails
//!
//! ```bash
//! RUN_MAINNET_SIM=1 cargo test -p sol-trade-router-sdk mainnet_ -- --nocapture --test-threads=1
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
    cpmm_swap_exact_out_leg, cpmm_swap_leg, launchlab_buy_leg, launchlab_sell_leg,
    meteora_damm_v2_swap_leg, meteora_dlmm_swap_leg, pumpfun_buy_leg, pumpfun_buy_v2_leg,
    pumpfun_sell_leg, pumpfun_sell_v2_leg, pumpswap_buy_exact_out_leg, pumpswap_buy_leg,
    pumpswap_sell_leg, raydium_amm_v4_swap_exact_out_leg, raydium_amm_v4_swap_leg,
    raydium_clmm_swap_leg, whirlpool_swap_leg,
};
use crate::mainnet_sim::{
    assert_sim_hard, assert_sim_ok, create_wallet, enabled, fill_amm_v4_mints,
    fill_clmm_token_programs, fill_whirlpool_token_programs, fixtures, load_amm_v4_pool,
    load_cpmm_pool, load_pumpswap_pool, require_rpc, rpc, scan_events, soft_coverage,
    simulate_direct_legs, simulate_legs_funded, simulate_with_fresh_wallet, SimVerdict,
};
use crate::parser::{
    amm_v4_from_swap, clmm_from_swap, cpmm_from_swap, damm_v2_from_swap, dlmm_from_swap,
    launchlab_from_trade, market_from_dex_event, market_from_dex_event_checked,
    pumpfun_from_trade, pumpswap_from_buy, pumpswap_from_sell, whirlpool_from_swap,
};
use crate::pool_guard::PoolGuardPolicy;
use crate::quote::pumpswap_buy_base_out;

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

/// PumpFun V1 `buy_exact_sol_in` spends native SOL — do not wrap into WSOL first.
fn setup_meme_only(wallet: &Pubkey, meme: Pubkey, meme_tp: Pubkey) -> Vec<Instruction> {
    vec![create_ata(wallet, wallet, &meme, &meme_tp)]
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

/// Fault injection: Soft (e.g. tickarray) or Hard both prove we hit the DEX; Ok is a miss.
fn assert_funded_fault(scenario: &str, verdict: Option<SimVerdict>) {
    match verdict {
        None => panic!("[{scenario}] required funder / RPC for coverage, none available"),
        Some(SimVerdict::Ok) => panic!("[{scenario}] expected Soft/Hard fault, got Ok"),
        Some(SimVerdict::Soft(m) | SimVerdict::Hard(m)) => {
            println!("[{scenario}] fault accepted: {m}");
        }
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
        // Skip empty / graduated curves — BuyZeroAmount is expected, not a layout signal.
        if e.real_token_reserves == 0 || e.virtual_token_reserves == 0 {
            return false;
        }
        let wallet = create_wallet();
        let user = wallet.pubkey();
        // Native SOL buy: spend observed size (floor 0.001 SOL) — do not wrap WSOL.
        let lamports = e.sol_amount.max(1_000_000).min(20_000_000);
        let user_ata = ata(&user, &pool.mint, &pool.mint_token_program);
        let setup = setup_meme_only(&user, pool.mint, pool.mint_token_program);
        let leg = pumpfun_buy_leg(&user, &pool, lamports, 0, user_ata);
        println!(
            "[pumpfun_buy_v1] sig={sig} wallet={user} mint={} lamports={lamports} real_tok={}",
            pool.mint, e.real_token_reserves
        );
        assert_funded_ok("pumpfun_buy_v1", simulate_legs_funded(&client, &wallet, setup, &[leg]));
        true
    });
    soft_coverage("pumpfun_buy_v1", found);
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
        if e.real_token_reserves == 0 || e.virtual_token_reserves == 0 {
            return false;
        }
        let mut pool = pumpfun_from_trade(e);
        pool.use_v2 = true;
        let wallet = create_wallet();
        let user = wallet.pubkey();
        let wrap = e.sol_amount.max(1_000_000).min(20_000_000);
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
    soft_coverage("pumpfun_buy_v2", found);
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
        // Prefer the instruction family from the observed trade to avoid
        // UnsupportedQuoteMint when event quote_mint lags the on-chain curve.
        let force_v2 = matches!(e.ix_name.as_str(), "sell_v2" | "buy_v2" | "buy_exact_quote_in_v2")
            || pool.uses_v2();
        let wallet = create_wallet();
        let user = wallet.pubkey();
        let user_ata = ata(&user, &pool.mint, &pool.mint_token_program);
        let mut setup = vec![create_ata(
            &user,
            &user,
            &pool.mint,
            &pool.mint_token_program,
        )];
        // V2 WSOL-ATA settlement needs a quote ATA; V1 sells native SOL.
        let leg = if force_v2 {
            if pool.quote_mint == WSOL_MINT || pool.quote_mint == Pubkey::default() {
                setup.push(create_wsol_ata(&user));
            } else {
                setup.push(create_ata(
                    &user,
                    &user,
                    &pool.quote_mint,
                    &pool.quote_token_program,
                ));
            }
            pumpfun_sell_v2_leg(&user, &pool, e.token_amount.max(1), 0)
        } else {
            pumpfun_sell_leg(&user, &pool, e.token_amount.max(1), 0, user_ata)
        };
        println!(
            "[pumpfun_sell] sig={sig} wallet={user} v2={force_v2} ix={} (expect soft: no tokens)",
            e.ix_name
        );
        assert_funded_ok(
            "pumpfun_sell",
            simulate_legs_funded(&client, &wallet, setup, &[leg]),
        );
        true
    });
    soft_coverage("pumpfun_sell", found);
}

// ─── PumpSwap (PumpFun 外盘) ────────────────────────────────────────────────

/// Try fixture pools with both exact-quote and exact-out buy paths.
fn try_pumpswap_fixture_buy(client: &RpcClient, pool_key: &Pubkey, label: &str) -> bool {
    let Some(pool) = load_pumpswap_pool(client, pool_key) else {
        println!("[{label}] skip: load failed for {pool_key}");
        return false;
    };
    if pool.quote_mint != WSOL_MINT {
        println!("[{label}] skip: non-WSOL quote");
        return false;
    }
    let quote_in = 50_000_000u64; // 0.05 SOL — large enough vs virtual_quote dust
    let Ok(base_out) = pumpswap_buy_base_out(&pool, quote_in) else {
        println!(
            "[{label}] skip: quote math failed base_res={} quote_res={} virt={}",
            pool.base_reserve, pool.quote_reserve, pool.virtual_quote_reserves
        );
        return false;
    };
    if base_out == 0 {
        println!("[{label}] skip: quoted base_out=0 (pool not buyable at this size)");
        return false;
    }
    let wallet = create_wallet();
    let user = wallet.pubkey();
    let setup = setup_wsol_and_meme(&user, pool.base_mint, pool.base_token_program, quote_in);

    // Path A: classic buy (exact base out) — more reliable when fee tiers are quirky.
    let Ok(leg_eo) = pumpswap_buy_exact_out_leg(&user, &pool, base_out.max(1) / 2, quote_in) else {
        return false;
    };
    println!(
        "[{label}] exact-out wallet={user} pool={} base={} max_quote={quote_in} cashback={}",
        pool.pool,
        base_out / 2,
        pool.is_cashback_coin
    );
    assert_funded_ok(
        label,
        simulate_legs_funded(client, &wallet, setup.clone(), &[leg_eo]),
    );

    // Path B: buy_exact_quote_in (same wallet funding pattern, fresh wallet).
    let wallet2 = create_wallet();
    let user2 = wallet2.pubkey();
    let setup2 = setup_wsol_and_meme(&user2, pool.base_mint, pool.base_token_program, quote_in);
    let Ok(leg_eq) = pumpswap_buy_leg(&user2, &pool, quote_in, 0) else {
        return false;
    };
    println!("[{label}] exact-quote wallet={user2} quote_in={quote_in}");
    assert_funded_ok(
        &format!("{label}_eq"),
        simulate_legs_funded(client, &wallet2, setup2, &[leg_eq]),
    );
    true
}

#[test]
fn mainnet_sim_pumpswap_buy() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    // Prefer seed pool (usually healthier liquidity) then cashback fixture.
    if try_pumpswap_fixture_buy(&client, &fixtures::PUMPSWAP_SEED_POOL, "pumpswap_buy_seed")
        || try_pumpswap_fixture_buy(&client, &fixtures::PUMPSWAP_POOL, "pumpswap_buy_fixture")
    {
        return;
    }
    let filter = EventTypeFilter::include_only(vec![EventType::PumpSwapTrade]);
    let found = try_event_window(
        &client,
        &[
            fixtures::PUMPSWAP_SEED_POOL,
            fixtures::PUMPSWAP_POOL,
            PUMPSWAP_PROGRAM,
        ],
        filter,
        80,
        |sig, ev| {
            let DexEvent::PumpSwapBuy(e) = ev else {
                return false;
            };
            if e.pool == Pubkey::default() || e.quote_mint != WSOL_MINT {
                return false;
            }
            let pool = pumpswap_from_buy(e);
            let wallet = create_wallet();
            let user = wallet.pubkey();
            let quote_in = e.quote_amount_in.max(1_000_000).min(50_000_000);
            let setup =
                setup_wsol_and_meme(&user, pool.base_mint, pool.base_token_program, quote_in);
            // Prefer exact-out when event carries base_amount_out.
            let leg = if e.base_amount_out > 0 {
                pumpswap_buy_exact_out_leg(
                    &user,
                    &pool,
                    (e.base_amount_out / 2).max(1),
                    quote_in.saturating_mul(2),
                )
            } else {
                pumpswap_buy_leg(&user, &pool, quote_in, 0)
            };
            let Ok(leg) = leg else {
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
    soft_coverage("pumpswap_buy", found);
}

#[test]
fn mainnet_sim_pumpswap_buy_sell_roundtrip() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let pool = load_pumpswap_pool(&client, &fixtures::PUMPSWAP_SEED_POOL)
        .or_else(|| load_pumpswap_pool(&client, &fixtures::PUMPSWAP_POOL));
    let Some(pool) = pool.filter(|p| p.quote_mint == WSOL_MINT) else {
        soft_coverage("pumpswap_roundtrip", false);
        return;
    };
    let quote_in = 20_000_000u64;
    let Ok(base_out) = pumpswap_buy_base_out(&pool, quote_in) else {
        soft_coverage("pumpswap_roundtrip", false);
        return;
    };
    if base_out < 2 {
        soft_coverage("pumpswap_roundtrip", false);
        return;
    }
    let buy_base = base_out / 2;
    let wallet = create_wallet();
    let user = wallet.pubkey();
    let setup = setup_wsol_and_meme(&user, pool.base_mint, pool.base_token_program, quote_in);
    let buy = pumpswap_buy_exact_out_leg(&user, &pool, buy_base, quote_in).expect("buy");
    let sell = pumpswap_sell_leg(&user, &pool, buy_base, 0).expect("sell");
    println!(
        "[pumpswap_roundtrip] wallet={user} pool={} buy_base={buy_base}",
        pool.pool
    );
    assert_funded_ok(
        "pumpswap_roundtrip",
        simulate_legs_funded(&client, &wallet, setup, &[buy, sell]),
    );
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
    soft_coverage("pumpswap_sell", found);
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
    soft_coverage("launchlab_buy", found);
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
    soft_coverage("cpmm_swap", found);
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
        if !fill_amm_v4_mints(&client, &mut pool) {
            return false;
        }
        // Prefer SOL-paired swaps for fresh-wallet funding.
        if pool.coin_mint != WSOL_MINT && pool.pc_mint != WSOL_MINT {
            return false;
        }
        let wallet = create_wallet();
        let user = wallet.pubkey();
        let amount = e.amount_in.max(100_000).min(5_000_000);
        let input = WSOL_MINT;
        let out_mint = if pool.coin_mint == WSOL_MINT {
            pool.pc_mint
        } else {
            pool.coin_mint
        };
        let mut setup = setup_wsol(&user, amount);
        setup.push(create_ata(
            &user,
            &user,
            &out_mint,
            &pool.token_program,
        ));
        let Ok(leg) = raydium_amm_v4_swap_leg(&user, &pool, amount, 0, input) else {
            return false;
        };
        println!(
            "[raydium_amm_v4] sig={sig} wallet={user} amm={} tp={}",
            pool.amm, pool.token_program
        );
        assert_funded_ok(
            "raydium_amm_v4",
            simulate_legs_funded(&client, &wallet, setup, &[leg]),
        );
        true
    });
    soft_coverage("raydium_amm_v4", found);
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
        if !fill_clmm_token_programs(&client, &mut pool) {
            return false;
        }
        let out_tp = if output == pool.token_0_mint {
            pool.token_0_program
        } else {
            pool.token_1_program
        };
        let wallet = create_wallet();
        let user = wallet.pubkey();
        let amount = if e.zero_for_one {
            e.amount_0
        } else {
            e.amount_1
        }
        .max(100_000)
        .min(5_000_000);
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
    soft_coverage("raydium_clmm", found);
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
        if !fill_whirlpool_token_programs(&client, &mut pool) {
            return false;
        }
        let out_tp = if output == pool.mint_a {
            pool.token_program_a
        } else {
            pool.token_program_b
        };
        let wallet = create_wallet();
        let user = wallet.pubkey();
        let amount = e.input_amount.max(100_000).min(5_000_000);
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
    soft_coverage("orca_whirlpool", found);
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
    soft_coverage("meteora_dlmm", found);
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
    soft_coverage("meteora_damm_v2", found);
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
    soft_coverage("parser", hits > 0);
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
    let covered = try_pumpswap_fixture_buy(&client, &fixtures::PUMPSWAP_SEED_POOL, "fixture_pumpswap_seed")
        || try_pumpswap_fixture_buy(&client, &fixtures::PUMPSWAP_POOL, "fixture_pumpswap");
    soft_coverage("fixture_pumpswap", covered);
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
    soft_coverage("fixture_damm_v2", found);
}

// ─── Reverse / exact-out / broader fault coverage ───────────────────────────

#[test]
fn mainnet_sim_cpmm_exact_out_and_reverse() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let pool = load_cpmm_pool(&client, &fixtures::WSOL_STONK_CPMM)
        .expect("cpmm fixture");
    let meme = pool.meme_mint();
    let meme_tp = pool.token_program_for(&meme).unwrap_or(TOKEN_PROGRAM);
    let wallet = create_wallet();
    let user = wallet.pubkey();
    let max_in = 2_000_000u64;
    let amount_out = 1_000u64;
    let setup = setup_wsol_and_meme(&user, meme, meme_tp, max_in);
    let leg = cpmm_swap_exact_out_leg(
        &user,
        &pool,
        max_in,
        amount_out,
        WSOL_MINT,
        meme,
        ata(&user, &WSOL_MINT, &TOKEN_PROGRAM),
        ata(&user, &meme, &meme_tp),
    )
    .expect("cpmm exact-out");
    println!("[cpmm_exact_out] wallet={user} out={amount_out}");
    assert_funded_ok(
        "cpmm_exact_out",
        simulate_legs_funded(&client, &wallet, setup, &[leg]),
    );

    // Reverse: meme→WSOL without meme balance → soft insufficient funds.
    let wallet2 = create_wallet();
    let user2 = wallet2.pubkey();
    let setup2 = vec![
        create_ata(&user2, &user2, &meme, &meme_tp),
        create_wsol_ata(&user2),
    ];
    let leg2 = cpmm_swap_leg(
        &user2,
        &pool,
        1_000,
        0,
        meme,
        WSOL_MINT,
        ata(&user2, &meme, &meme_tp),
        ata(&user2, &WSOL_MINT, &TOKEN_PROGRAM),
    )
    .expect("cpmm reverse");
    println!("[cpmm_reverse] wallet={user2} (expect soft: no meme)");
    assert_funded_ok(
        "cpmm_reverse",
        simulate_legs_funded(&client, &wallet2, setup2, &[leg2]),
    );
}

#[test]
fn mainnet_sim_raydium_amm_v4_exact_out() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let filter = EventTypeFilter::include_only(vec![EventType::RaydiumAmmV4Swap]);
    let found = try_event_window(
        &client,
        &[
            fixtures::AMM_V4_WSOL_USDC,
            fixtures::AMM_V4_WSOL_USDT,
            RAYDIUM_AMM_V4_PROGRAM,
        ],
        filter,
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
            let out_mint = if pool.coin_mint == WSOL_MINT {
                pool.pc_mint
            } else {
                pool.coin_mint
            };
            let wallet = create_wallet();
            let user = wallet.pubkey();
            let max_in = 5_000_000u64;
            let amount_out = 1_000u64;
            let mut setup = setup_wsol(&user, max_in);
            setup.push(create_ata(&user, &user, &out_mint, &pool.token_program));
            let Ok(leg) =
                raydium_amm_v4_swap_exact_out_leg(&user, &pool, amount_out, max_in, WSOL_MINT)
            else {
                return false;
            };
            println!(
                "[amm_v4_exact_out] sig={sig} wallet={user} amm={}",
                pool.amm
            );
            assert_funded_ok(
                "amm_v4_exact_out",
                simulate_legs_funded(&client, &wallet, setup, &[leg]),
            );
            true
        },
    );
    soft_coverage("amm_v4_exact_out", found);
}

#[test]
fn mainnet_sim_raydium_clmm_reverse_soft() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let filter = EventTypeFilter::include_only(vec![EventType::RaydiumClmmSwap]);
    let found = try_event_window(&client, &[RAYDIUM_CLMM_PROGRAM], filter, 100, |sig, ev| {
        let DexEvent::RaydiumClmmSwap(e) = ev else {
            return false;
        };
        let Some(mut pool) = clmm_from_swap(e) else {
            return false;
        };
        // Reverse of WSOL→meme: meme→WSOL without balance.
        let input = e.output_mint;
        let output = e.input_mint;
        if output != WSOL_MINT || input == WSOL_MINT {
            return false;
        }
        if !fill_clmm_token_programs(&client, &mut pool) {
            return false;
        }
        let in_tp = if input == pool.token_0_mint {
            pool.token_0_program
        } else {
            pool.token_1_program
        };
        let wallet = create_wallet();
        let user = wallet.pubkey();
        let setup = vec![
            create_ata(&user, &user, &input, &in_tp),
            create_wsol_ata(&user),
        ];
        let Ok(leg) = raydium_clmm_swap_leg(&user, &pool, 1_000, 0, input) else {
            return false;
        };
        println!("[clmm_reverse] sig={sig} wallet={user} (expect soft)");
        assert_funded_ok(
            "clmm_reverse",
            simulate_legs_funded(&client, &wallet, setup, &[leg]),
        );
        true
    });
    soft_coverage("clmm_reverse", found);
}

#[test]
fn mainnet_sim_orca_whirlpool_reverse_soft() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let filter = EventTypeFilter::include_only(vec![EventType::OrcaWhirlpoolSwap]);
    let found = try_event_window(&client, &[ORCA_WHIRLPOOL_PROGRAM], filter, 100, |sig, ev| {
        let DexEvent::OrcaWhirlpoolSwap(e) = ev else {
            return false;
        };
        let Some(mut pool) = whirlpool_from_swap(e) else {
            return false;
        };
        let input = if e.a_to_b { pool.mint_b } else { pool.mint_a };
        let output = if e.a_to_b { pool.mint_a } else { pool.mint_b };
        if output != WSOL_MINT || input == WSOL_MINT {
            return false;
        }
        if !fill_whirlpool_token_programs(&client, &mut pool) {
            return false;
        }
        let in_tp = if input == pool.mint_a {
            pool.token_program_a
        } else {
            pool.token_program_b
        };
        let wallet = create_wallet();
        let user = wallet.pubkey();
        let setup = vec![
            create_ata(&user, &user, &input, &in_tp),
            create_wsol_ata(&user),
        ];
        let Ok(leg) = whirlpool_swap_leg(&user, &pool, 1_000, 0, input) else {
            return false;
        };
        println!("[whirlpool_reverse] sig={sig} wallet={user} (expect soft)");
        assert_funded_ok(
            "whirlpool_reverse",
            simulate_legs_funded(&client, &wallet, setup, &[leg]),
        );
        true
    });
    soft_coverage("whirlpool_reverse", found);
}

#[test]
fn mainnet_fault_amm_v4_wrong_token_program_is_hard() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let filter = EventTypeFilter::include_only(vec![EventType::RaydiumAmmV4Swap]);
    let found = try_event_window(
        &client,
        &[fixtures::AMM_V4_WSOL_USDC, RAYDIUM_AMM_V4_PROGRAM],
        filter,
        60,
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
            // Corrupt token program → IncorrectProgramId / 0x26 HARD.
            pool.token_program = TOKEN_2022_PROGRAM;
            let wallet = create_wallet();
            let user = wallet.pubkey();
            let amount = 100_000u64;
            let out = if pool.coin_mint == WSOL_MINT {
                pool.pc_mint
            } else {
                pool.coin_mint
            };
            // ATA must still use real SPL owner for vaults; only ix account[0] is wrong.
            let mut setup = setup_wsol(&user, amount);
            setup.push(create_ata(&user, &user, &out, &TOKEN_PROGRAM));
            let Ok(mut leg) = raydium_amm_v4_swap_leg(&user, &pool, amount, 0, WSOL_MINT) else {
                return false;
            };
            // Force account[0] Token-2022 while user ATAs remain SPL.
            leg.accounts[0].pubkey = TOKEN_2022_PROGRAM;
            println!("[fault_amm_v4_tp] sig={sig} wallet={user}");
            assert_funded_hard(
                "fault_amm_v4_tp",
                simulate_legs_funded(&client, &wallet, setup, &[leg]),
            );
            true
        },
    );
    soft_coverage("fault_amm_v4_tp", found);
}

#[test]
fn mainnet_fault_clmm_empty_ticks_is_hard_or_reject() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let filter = EventTypeFilter::include_only(vec![EventType::RaydiumClmmSwap]);
    let found = try_event_window(&client, &[RAYDIUM_CLMM_PROGRAM], filter, 150, |sig, ev| {
        let DexEvent::RaydiumClmmSwap(e) = ev else {
            return false;
        };
        let Some(mut pool) = clmm_from_swap(e) else {
            return false;
        };
        // Any WSOL-paired direction — prune-friendly (input need not be WSOL).
        if pool.token_0_mint != WSOL_MINT && pool.token_1_mint != WSOL_MINT {
            return false;
        }
        if !fill_clmm_token_programs(&client, &mut pool) {
            return false;
        }
        let input = WSOL_MINT;
        let output = if pool.token_0_mint == WSOL_MINT {
            pool.token_1_mint
        } else {
            pool.token_0_mint
        };
        let out_tp = if output == pool.token_0_mint {
            pool.token_0_program
        } else {
            pool.token_1_program
        };
        // Corrupt tick arrays → Soft(tickarray) or Hard — both count.
        pool.tick_arrays = vec![Pubkey::new_unique(); pool.tick_arrays.len().max(1)];
        let wallet = create_wallet();
        let user = wallet.pubkey();
        let amount = 100_000u64;
        let setup = setup_wsol_and_meme(&user, output, out_tp, amount);
        let Ok(leg) = raydium_clmm_swap_leg(&user, &pool, amount, 0, input) else {
            println!("[fault_clmm_ticks] sig={sig} builder rejected (ok)");
            return true;
        };
        println!("[fault_clmm_ticks] sig={sig} wallet={user}");
        assert_funded_fault(
            "fault_clmm_ticks",
            simulate_legs_funded(&client, &wallet, setup, &[leg]),
        );
        true
    });
    soft_coverage("fault_clmm_ticks", found);
}

#[test]
fn mainnet_sim_multi_wallet_parallel_funding() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let pool = load_cpmm_pool(&client, &fixtures::WSOL_STONK_CPMM).expect("cpmm");
    let meme = pool.meme_mint();
    let meme_tp = pool.token_program_for(&meme).unwrap_or(TOKEN_PROGRAM);
    for i in 0..3 {
        let wallet = create_wallet();
        let user = wallet.pubkey();
        let amount = 100_000u64 * (i + 1);
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
        .expect("leg");
        println!("[multi_wallet] i={i} wallet={user} amount={amount}");
        assert_funded_ok(
            &format!("multi_wallet_{i}"),
            simulate_legs_funded(&client, &wallet, setup, &[leg]),
        );
    }
}

// ─── Extra coverage: sell / reverse / fault / parser sweep ───────────────────

#[test]
fn mainnet_sim_launchlab_sell_soft_without_balance() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let filter = EventTypeFilter::include_only(vec![EventType::RaydiumLaunchlabTrade]);
    let found = try_event_window(
        &client,
        &[fixtures::CURVE_POOL, LAUNCHLAB_PROGRAM],
        filter,
        100,
        |sig, ev| {
            let DexEvent::RaydiumLaunchlabTrade(e) = ev else {
                return false;
            };
            if e.is_buy {
                return false;
            }
            let Some(pool) = launchlab_from_trade(e) else {
                return false;
            };
            let wallet = create_wallet();
            let user = wallet.pubkey();
            let base_ata = ata(&user, &pool.base_mint, &pool.base_token_program);
            let quote_ata = ata(&user, &pool.quote_mint, &pool.quote_token_program);
            let setup = vec![
                create_ata(&user, &user, &pool.base_mint, &pool.base_token_program),
                create_ata(&user, &user, &pool.quote_mint, &pool.quote_token_program),
            ];
            let leg = launchlab_sell_leg(
                &user,
                &pool,
                e.amount_in.max(1),
                0,
                base_ata,
                quote_ata,
            );
            println!("[launchlab_sell] sig={sig} wallet={user} (expect soft)");
            assert_funded_ok(
                "launchlab_sell",
                simulate_legs_funded(&client, &wallet, setup, &[leg]),
            );
            true
        },
    );
    soft_coverage("launchlab_sell", found);
}

#[test]
fn mainnet_sim_meteora_dlmm_reverse_soft() {
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
        // Prefer meme→WSOL without balance.
        let (input, output) = if pool.token_x_mint == WSOL_MINT {
            (pool.token_y_mint, pool.token_x_mint)
        } else if pool.token_y_mint == WSOL_MINT {
            (pool.token_x_mint, pool.token_y_mint)
        } else {
            return false;
        };
        let in_tp = if input == pool.token_x_mint {
            pool.token_x_program
        } else {
            pool.token_y_program
        };
        let wallet = create_wallet();
        let user = wallet.pubkey();
        let setup = vec![
            create_ata(&user, &user, &input, &in_tp),
            create_wsol_ata(&user),
        ];
        let Ok(leg) = meteora_dlmm_swap_leg(&user, &pool, 1_000, 0, input) else {
            return false;
        };
        println!("[dlmm_reverse] sig={sig} wallet={user} out={output} (expect soft)");
        assert_funded_ok(
            "dlmm_reverse",
            simulate_legs_funded(&client, &wallet, setup, &[leg]),
        );
        true
    });
    soft_coverage("dlmm_reverse", found);
}

#[test]
fn mainnet_sim_meteora_damm_v2_reverse_soft() {
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
        100,
        |sig, ev| {
            let DexEvent::MeteoraDammV2Swap(e) = ev else {
                return false;
            };
            let Some(pool) = damm_v2_from_swap(e) else {
                return false;
            };
            let (input, in_tp) = if pool.token_a_mint == WSOL_MINT {
                (pool.token_b_mint, pool.token_b_program)
            } else if pool.token_b_mint == WSOL_MINT {
                (pool.token_a_mint, pool.token_a_program)
            } else {
                return false;
            };
            let wallet = create_wallet();
            let user = wallet.pubkey();
            let setup = vec![
                create_ata(&user, &user, &input, &in_tp),
                create_wsol_ata(&user),
            ];
            let Ok(leg) = meteora_damm_v2_swap_leg(&user, &pool, 1_000, 0, input) else {
                return false;
            };
            println!("[damm_v2_reverse] sig={sig} wallet={user} (expect soft)");
            assert_funded_ok(
                "damm_v2_reverse",
                simulate_legs_funded(&client, &wallet, setup, &[leg]),
            );
            true
        },
    );
    soft_coverage("damm_v2_reverse", found);
}

#[test]
fn mainnet_sim_amm_v4_reverse_soft() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let filter = EventTypeFilter::include_only(vec![EventType::RaydiumAmmV4Swap]);
    let found = try_event_window(
        &client,
        &[
            fixtures::AMM_V4_WSOL_USDC,
            fixtures::AMM_V4_WSOL_USDT,
            RAYDIUM_AMM_V4_PROGRAM,
        ],
        filter,
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
            let input = if pool.coin_mint == WSOL_MINT {
                pool.pc_mint
            } else {
                pool.coin_mint
            };
            let wallet = create_wallet();
            let user = wallet.pubkey();
            let setup = vec![
                create_ata(&user, &user, &input, &pool.token_program),
                create_wsol_ata(&user),
            ];
            let Ok(leg) = raydium_amm_v4_swap_leg(&user, &pool, 1_000, 0, input) else {
                return false;
            };
            println!("[amm_v4_reverse] sig={sig} wallet={user} (expect soft)");
            assert_funded_ok(
                "amm_v4_reverse",
                simulate_legs_funded(&client, &wallet, setup, &[leg]),
            );
            true
        },
    );
    soft_coverage("amm_v4_reverse", found);
}

#[test]
fn mainnet_fault_pumpfun_wrong_fee_recipient_is_fault() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let filter = EventTypeFilter::include_only(vec![EventType::PumpFunTrade]);
    let found = try_event_window(&client, &[PUMPFUN_PROGRAM], filter, 80, |sig, ev| {
        let (DexEvent::PumpFunTrade(e)
        | DexEvent::PumpFunBuy(e)
        | DexEvent::PumpFunBuyExactSolIn(e)) = ev
        else {
            return false;
        };
        if e.mint == Pubkey::default() || e.real_token_reserves == 0 {
            return false;
        }
        let mut pool = pumpfun_from_trade(e);
        if pool.uses_v2() {
            return false;
        }
        pool.fee_recipient = Pubkey::new_unique();
        let wallet = create_wallet();
        let user = wallet.pubkey();
        let lamports = 1_000_000u64;
        let user_ata = ata(&user, &pool.mint, &pool.mint_token_program);
        let setup = setup_meme_only(&user, pool.mint, pool.mint_token_program);
        let leg = pumpfun_buy_leg(&user, &pool, lamports, 0, user_ata);
        println!("[fault_pumpfun_fee] sig={sig} wallet={user}");
        assert_funded_fault(
            "fault_pumpfun_fee",
            simulate_legs_funded(&client, &wallet, setup, &[leg]),
        );
        true
    });
    soft_coverage("fault_pumpfun_fee", found);
}

#[test]
fn mainnet_sim_pumpfun_buy_then_sell_roundtrip_legs() {
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
        if e.mint == Pubkey::default()
            || e.real_token_reserves == 0
            || e.virtual_token_reserves == 0
        {
            return false;
        }
        let pool = pumpfun_from_trade(e);
        if pool.uses_v2() {
            return false;
        }
        let wallet = create_wallet();
        let user = wallet.pubkey();
        let lamports = e.sol_amount.max(1_000_000).min(10_000_000);
        let user_ata = ata(&user, &pool.mint, &pool.mint_token_program);
        let setup = setup_meme_only(&user, pool.mint, pool.mint_token_program);
        let buy = pumpfun_buy_leg(&user, &pool, lamports, 0, user_ata);
        // Sell dust after buy in same tx — Soft (slippage / reserves) is fine.
        let sell = pumpfun_sell_leg(&user, &pool, 1, 0, user_ata);
        println!("[pumpfun_roundtrip] sig={sig} wallet={user} lamports={lamports}");
        assert_funded_ok(
            "pumpfun_roundtrip",
            simulate_legs_funded(&client, &wallet, setup, &[buy, sell]),
        );
        true
    });
    soft_coverage("pumpfun_roundtrip", found);
}

#[test]
fn mainnet_fixture_amm_v4_usdc_and_usdt() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let mut any = false;
    for (label, pool_key) in [
        ("amm_v4_usdc", fixtures::AMM_V4_WSOL_USDC),
        ("amm_v4_usdt", fixtures::AMM_V4_WSOL_USDT),
    ] {
        let filter = EventTypeFilter::include_only(vec![EventType::RaydiumAmmV4Swap]);
        let found = try_event_window(&client, &[pool_key], filter, 40, |sig, ev| {
            let DexEvent::RaydiumAmmV4Swap(e) = ev else {
                return false;
            };
            let Some(mut pool) = amm_v4_from_swap(e) else {
                return false;
            };
            if !fill_amm_v4_mints(&client, &mut pool) {
                return false;
            }
            let wallet = create_wallet();
            let user = wallet.pubkey();
            let amount = 200_000u64;
            let out = if pool.coin_mint == WSOL_MINT {
                pool.pc_mint
            } else {
                pool.coin_mint
            };
            let mut setup = setup_wsol(&user, amount);
            setup.push(create_ata(&user, &user, &out, &pool.token_program));
            let Ok(leg) = raydium_amm_v4_swap_leg(&user, &pool, amount, 0, WSOL_MINT) else {
                return false;
            };
            println!("[{label}] sig={sig} wallet={user}");
            assert_funded_ok(label, simulate_legs_funded(&client, &wallet, setup, &[leg]));
            true
        });
        soft_coverage(label, found);
        any |= found;
    }
    soft_coverage("fixture_amm_v4_pair", any);
}

// ─── Broad coverage wave ─────────────────────────────────────────────────────

#[test]
fn mainnet_fixture_cpmm_wsol_cards_buy() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let Some(pool) = load_cpmm_pool(&client, &fixtures::WSOL_CARDS_CPMM) else {
        soft_coverage("fixture_cpmm_cards", false);
        return;
    };
    let meme = pool.meme_mint();
    let meme_tp = pool.token_program_for(&meme).unwrap_or(TOKEN_PROGRAM);
    let wallet = create_wallet();
    let user = wallet.pubkey();
    let amount = 150_000u64;
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
    .expect("cards leg");
    println!("[fixture_cpmm_cards] wallet={user} pool={}", pool.pool_state);
    assert_funded_ok(
        "fixture_cpmm_cards",
        simulate_direct_legs(&client, &wallet, setup, &[leg]),
    );
}

#[test]
fn mainnet_fixture_amm_v4_account_load_buy_and_exact_out() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let Some(pool) = load_amm_v4_pool(&client, &fixtures::AMM_V4_WSOL_USDC)
        .or_else(|| load_amm_v4_pool(&client, &fixtures::AMM_V4_WSOL_USDT))
    else {
        soft_coverage("fixture_amm_v4_account", false);
        return;
    };
    let out = if pool.coin_mint == WSOL_MINT {
        pool.pc_mint
    } else {
        pool.coin_mint
    };
    // Exact-in
    {
        let wallet = create_wallet();
        let user = wallet.pubkey();
        let amount = 250_000u64;
        let mut setup = setup_wsol(&user, amount);
        setup.push(create_ata(&user, &user, &out, &pool.token_program));
        let leg = raydium_amm_v4_swap_leg(&user, &pool, amount, 0, WSOL_MINT).expect("amm in");
        println!("[fixture_amm_v4_account] exact-in wallet={user} amm={}", pool.amm);
        assert_funded_ok(
            "fixture_amm_v4_account_in",
            simulate_legs_funded(&client, &wallet, setup, &[leg]),
        );
    }
    // Exact-out
    {
        let wallet = create_wallet();
        let user = wallet.pubkey();
        let amount_out = 10_000u64;
        let max_in = 5_000_000u64;
        let mut setup = setup_wsol(&user, max_in);
        setup.push(create_ata(&user, &user, &out, &pool.token_program));
        let leg =
            raydium_amm_v4_swap_exact_out_leg(&user, &pool, amount_out, max_in, WSOL_MINT)
                .expect("amm eo");
        println!("[fixture_amm_v4_account] exact-out wallet={user}");
        assert_funded_ok(
            "fixture_amm_v4_account_eo",
            simulate_legs_funded(&client, &wallet, setup, &[leg]),
        );
    }
}

#[test]
fn mainnet_fixture_cpmm_amount_sweep() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let Some(pool) = load_cpmm_pool(&client, &fixtures::WSOL_STONK_CPMM) else {
        soft_coverage("cpmm_amount_sweep", false);
        return;
    };
    let meme = pool.meme_mint();
    let meme_tp = pool.token_program_for(&meme).unwrap_or(TOKEN_PROGRAM);
    for amount in [50_000u64, 200_000, 1_000_000, 5_000_000] {
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
        .expect("sweep leg");
        println!("[cpmm_amount_sweep] wallet={user} amount={amount}");
        assert_funded_ok(
            &format!("cpmm_amount_sweep_{amount}"),
            simulate_legs_funded(&client, &wallet, setup, &[leg]),
        );
    }
}

#[test]
fn mainnet_sim_pumpfun_cashback_buy_soft() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let filter = EventTypeFilter::include_only(vec![EventType::PumpFunTrade]);
    let found = try_event_window(&client, &[PUMPFUN_PROGRAM], filter, 150, |sig, ev| {
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
        let mut pool = pumpfun_from_trade(e);
        // Prefer live cashback flags; also accept fee-bps signal from the event.
        if !pool.is_cashback_coin && e.cashback_fee_basis_points == 0 {
            return false;
        }
        pool.is_cashback_coin = true;
        let wallet = create_wallet();
        let user = wallet.pubkey();
        let lamports = e.sol_amount.max(1_000_000).min(10_000_000);
        let setup = setup_meme_only(&user, pool.mint, pool.mint_token_program);
        let leg = if pool.uses_v2() {
            let mut s = setup;
            s.push(create_wsol_ata(&user));
            let leg = pumpfun_buy_v2_leg(&user, &pool, lamports, 0);
            println!("[pumpfun_cashback] sig={sig} wallet={user} v2=true");
            assert_funded_ok(
                "pumpfun_cashback",
                simulate_legs_funded(&client, &wallet, s, &[leg]),
            );
            return true;
        } else {
            let user_ata = ata(&user, &pool.mint, &pool.mint_token_program);
            pumpfun_buy_leg(&user, &pool, lamports, 0, user_ata)
        };
        println!("[pumpfun_cashback] sig={sig} wallet={user} v2=false");
        assert_funded_ok(
            "pumpfun_cashback",
            simulate_legs_funded(&client, &wallet, setup, &[leg]),
        );
        true
    });
    soft_coverage("pumpfun_cashback", found);
}

#[test]
fn mainnet_sim_clmm_token2022_prefer() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let filter = EventTypeFilter::include_only(vec![EventType::RaydiumClmmSwap]);
    let found = try_event_window(&client, &[RAYDIUM_CLMM_PROGRAM], filter, 150, |sig, ev| {
        let DexEvent::RaydiumClmmSwap(e) = ev else {
            return false;
        };
        let Some(mut pool) = clmm_from_swap(e) else {
            return false;
        };
        if e.input_mint != WSOL_MINT {
            return false;
        }
        if !fill_clmm_token_programs(&client, &mut pool) {
            return false;
        }
        let out_tp = if e.output_mint == pool.token_0_mint {
            pool.token_0_program
        } else {
            pool.token_1_program
        };
        // Prefer Token-2022 meme legs — classic SPL already covered elsewhere.
        if out_tp != TOKEN_2022_PROGRAM {
            return false;
        }
        let wallet = create_wallet();
        let user = wallet.pubkey();
        let amount = e.amount_0.max(e.amount_1).max(100_000).min(2_000_000);
        let setup = setup_wsol_and_meme(&user, e.output_mint, out_tp, amount);
        let Ok(leg) = raydium_clmm_swap_leg(&user, &pool, amount, 0, WSOL_MINT) else {
            return false;
        };
        println!("[clmm_token2022] sig={sig} wallet={user} out_tp={out_tp}");
        assert_funded_ok(
            "clmm_token2022",
            simulate_legs_funded(&client, &wallet, setup, &[leg]),
        );
        true
    });
    soft_coverage("clmm_token2022", found);
}

#[test]
fn mainnet_sim_launchlab_graduated_pool_events() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let filter = EventTypeFilter::include_only(vec![EventType::RaydiumLaunchlabTrade]);
    let found = try_event_window(
        &client,
        &[fixtures::GRAD_POOL, LAUNCHLAB_PROGRAM],
        filter,
        80,
        |sig, ev| {
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
            let amount = e.amount_in.max(50_000).min(1_000_000);
            let base_ata = ata(&user, &pool.base_mint, &pool.base_token_program);
            let quote_ata = ata(&user, &pool.quote_mint, &pool.quote_token_program);
            let setup = if pool.is_sol_quote() {
                setup_wsol_and_meme(&user, pool.base_mint, pool.base_token_program, amount)
            } else {
                vec![
                    create_ata(&user, &user, &pool.base_mint, &pool.base_token_program),
                    create_ata(&user, &user, &pool.quote_mint, &pool.quote_token_program),
                ]
            };
            let leg = launchlab_buy_leg(&user, &pool, amount, 0, base_ata, quote_ata);
            println!(
                "[launchlab_grad] sig={sig} wallet={user} pool={}",
                pool.pool_state
            );
            assert_funded_ok(
                "launchlab_grad",
                simulate_legs_funded(&client, &wallet, setup, &[leg]),
            );
            true
        },
    );
    soft_coverage("launchlab_grad", found);
}

#[test]
fn mainnet_fault_whirlpool_empty_ticks() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let filter = EventTypeFilter::include_only(vec![EventType::OrcaWhirlpoolSwap]);
    let found = try_event_window(&client, &[ORCA_WHIRLPOOL_PROGRAM], filter, 100, |sig, ev| {
        let DexEvent::OrcaWhirlpoolSwap(e) = ev else {
            return false;
        };
        let Some(mut pool) = whirlpool_from_swap(e) else {
            return false;
        };
        if pool.mint_a != WSOL_MINT && pool.mint_b != WSOL_MINT {
            return false;
        }
        if !fill_whirlpool_token_programs(&client, &mut pool) {
            return false;
        }
        pool.tick_arrays = vec![Pubkey::new_unique(); pool.tick_arrays.len().max(1)];
        let input = WSOL_MINT;
        let output = if pool.mint_a == WSOL_MINT {
            pool.mint_b
        } else {
            pool.mint_a
        };
        let out_tp = if output == pool.mint_a {
            pool.token_program_a
        } else {
            pool.token_program_b
        };
        let wallet = create_wallet();
        let user = wallet.pubkey();
        let setup = setup_wsol_and_meme(&user, output, out_tp, 100_000);
        let Ok(leg) = whirlpool_swap_leg(&user, &pool, 100_000, 0, input) else {
            println!("[fault_whirlpool_ticks] sig={sig} builder rejected");
            return true;
        };
        println!("[fault_whirlpool_ticks] sig={sig} wallet={user}");
        assert_funded_fault(
            "fault_whirlpool_ticks",
            simulate_legs_funded(&client, &wallet, setup, &[leg]),
        );
        true
    });
    soft_coverage("fault_whirlpool_ticks", found);
}

#[test]
fn mainnet_fault_dlmm_empty_bins() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let filter = EventTypeFilter::include_only(vec![EventType::MeteoraDlmmSwap]);
    let found = try_event_window(&client, &[METEORA_DLMM_PROGRAM], filter, 100, |sig, ev| {
        let DexEvent::MeteoraDlmmSwap(e) = ev else {
            return false;
        };
        let Some(mut pool) = dlmm_from_swap(e) else {
            return false;
        };
        if pool.token_x_mint != WSOL_MINT && pool.token_y_mint != WSOL_MINT {
            return false;
        }
        pool.bin_arrays = vec![Pubkey::new_unique(); pool.bin_arrays.len().max(1)];
        let input = WSOL_MINT;
        let output = if pool.token_x_mint == WSOL_MINT {
            pool.token_y_mint
        } else {
            pool.token_x_mint
        };
        let out_tp = if output == pool.token_x_mint {
            pool.token_x_program
        } else {
            pool.token_y_program
        };
        let wallet = create_wallet();
        let user = wallet.pubkey();
        let setup = setup_wsol_and_meme(&user, output, out_tp, 100_000);
        let Ok(leg) = meteora_dlmm_swap_leg(&user, &pool, 100_000, 0, input) else {
            println!("[fault_dlmm_bins] sig={sig} builder rejected");
            return true;
        };
        println!("[fault_dlmm_bins] sig={sig} wallet={user}");
        assert_funded_fault(
            "fault_dlmm_bins",
            simulate_legs_funded(&client, &wallet, setup, &[leg]),
        );
        true
    });
    soft_coverage("fault_dlmm_bins", found);
}

#[test]
fn mainnet_fault_cpmm_wrong_observation_is_fault() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let Some(mut pool) = load_cpmm_pool(&client, &fixtures::WSOL_STONK_CPMM) else {
        soft_coverage("fault_cpmm_obs", false);
        return;
    };
    pool.observation_state = Pubkey::new_unique();
    let meme = pool.meme_mint();
    let meme_tp = pool.token_program_for(&meme).unwrap_or(TOKEN_PROGRAM);
    let wallet = create_wallet();
    let user = wallet.pubkey();
    let amount = 100_000u64;
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
    .expect("leg");
    println!("[fault_cpmm_obs] wallet={user}");
    assert_funded_fault(
        "fault_cpmm_obs",
        simulate_legs_funded(&client, &wallet, setup, &[leg]),
    );
}

#[test]
fn mainnet_parser_checked_vs_unchecked() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let policy = PoolGuardPolicy::disabled();
    let mut unchecked = 0usize;
    let mut checked = 0usize;
    let programs = [
        PUMPFUN_PROGRAM,
        PUMPSWAP_PROGRAM,
        RAYDIUM_CPMM_PROGRAM,
        RAYDIUM_AMM_V4_PROGRAM,
        RAYDIUM_CLMM_PROGRAM,
        ORCA_WHIRLPOOL_PROGRAM,
        METEORA_DLMM_PROGRAM,
        METEORA_DAMM_V2_PROGRAM,
        LAUNCHLAB_PROGRAM,
    ];
    let _ = scan_events(&client, &programs, 30, |_sig, ev| {
        if market_from_dex_event(ev).is_some() {
            unchecked += 1;
        }
        if market_from_dex_event_checked(ev, &policy).is_some() {
            checked += 1;
        }
        unchecked >= 8 // stop early once we have diversity
    });
    println!("[parser_checked] unchecked={unchecked} checked={checked}");
    soft_coverage("parser_checked", unchecked > 0);
}

#[test]
fn mainnet_sim_multi_dex_fixture_battery() {
    if !enabled() {
        return;
    }
    let client = rpc();
    if !require_rpc(&client) {
        return;
    }
    let mut hits = 0usize;
    // CPMM stonk
    if let Some(pool) = load_cpmm_pool(&client, &fixtures::WSOL_STONK_CPMM) {
        let wallet = create_wallet();
        let user = wallet.pubkey();
        let meme = pool.meme_mint();
        let meme_tp = pool.token_program_for(&meme).unwrap_or(TOKEN_PROGRAM);
        let amount = 80_000u64;
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
        .expect("cpmm");
        assert_funded_ok(
            "battery_cpmm",
            simulate_legs_funded(&client, &wallet, setup, &[leg]),
        );
        hits += 1;
    }
    // PumpSwap seed
    if let Some(pool) = load_pumpswap_pool(&client, &fixtures::PUMPSWAP_SEED_POOL) {
        if pool.quote_mint == WSOL_MINT {
            let wallet = create_wallet();
            let user = wallet.pubkey();
            let amount = 50_000_000u64;
            let setup = setup_wsol_and_meme(&user, pool.base_mint, pool.base_token_program, amount);
            if let Ok(leg) = pumpswap_buy_leg(&user, &pool, amount, 0) {
                assert_funded_ok(
                    "battery_pumpswap",
                    simulate_legs_funded(&client, &wallet, setup, &[leg]),
                );
                hits += 1;
            }
        }
    }
    // AMM V4
    if let Some(pool) = load_amm_v4_pool(&client, &fixtures::AMM_V4_WSOL_USDC) {
        let wallet = create_wallet();
        let user = wallet.pubkey();
        let amount = 120_000u64;
        let out = if pool.coin_mint == WSOL_MINT {
            pool.pc_mint
        } else {
            pool.coin_mint
        };
        let mut setup = setup_wsol(&user, amount);
        setup.push(create_ata(&user, &user, &out, &pool.token_program));
        if let Ok(leg) = raydium_amm_v4_swap_leg(&user, &pool, amount, 0, WSOL_MINT) {
            assert_funded_ok(
                "battery_amm_v4",
                simulate_legs_funded(&client, &wallet, setup, &[leg]),
            );
            hits += 1;
        }
    }
    println!("[multi_dex_battery] hits={hits}");
    soft_coverage("multi_dex_battery", hits > 0);
}
