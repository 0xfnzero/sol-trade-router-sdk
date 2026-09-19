//! Offline unit tests (no RPC) — wallets, quotes, legs, trade build, classify.

#![cfg(test)]

use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Keypair;
use solana_sdk::signer::Signer;

use crate::constants::{
    METEORA_DAMM_V2_PROGRAM, METEORA_DLMM_PROGRAM, ORCA_WHIRLPOOL_PROGRAM, PUMPFUN_PROGRAM,
    PUMPSWAP_PROGRAM, RAYDIUM_AMM_V4_PROGRAM, RAYDIUM_CLMM_PROGRAM, RAYDIUM_CPMM_PROGRAM,
    TOKEN_PROGRAM, WSOL_MINT,
};
use crate::legs::{
    cpmm_swap_leg, launchlab_buy_leg, launchlab_sell_leg, meteora_damm_v2_swap_leg,
    meteora_dlmm_swap_leg, pumpfun_buy_leg, pumpfun_buy_v2_leg, pumpfun_sell_leg,
    pumpswap_buy_leg, pumpswap_sell_leg, raydium_amm_v4_swap_leg, raydium_clmm_swap_leg,
    whirlpool_swap_leg, Leg,
};
use crate::mainnet_sim::{classify_err, create_wallet, SimVerdict};
use crate::market::{
    CpmmPool, LaunchLabPool, Market, MeteoraDammV2Pool, MeteoraDlmmPool, PumpFunPool, PumpSwapPool,
    RaydiumAmmV4Pool, RaydiumClmmPool, RoutedMarket, WhirlpoolPool,
};
use crate::quote::{
    apply_slippage_min_out, clamp_slippage_bps, cpmm_out, fee_amount, meteora_damm_v2_out,
    pumpfun_buy_token_out, pumpfun_sell_sol_out, pumpfun_total_fee_bps, pumpswap_buy_base_out,
    pumpswap_sell_quote_out, raydium_amm_v4_out,
};
use crate::pool_guard::PoolGuardPolicy;
use crate::trade::{RouterClient, TradeOpts};
use crate::transfer_fee::TokenTransferFee;

fn dummy_pumpfun() -> PumpFunPool {
    PumpFunPool {
        mint: Pubkey::new_unique(),
        mint_token_program: TOKEN_PROGRAM,
        quote_mint: WSOL_MINT,
        quote_token_program: TOKEN_PROGRAM,
        use_v2: false,
        bonding_curve: Pubkey::new_unique(),
        associated_bonding_curve: Pubkey::new_unique(),
        creator_vault: Pubkey::new_unique(),
        fee_recipient: Pubkey::new_unique(),
        buyback_fee_recipient: Pubkey::new_unique(),
        global: Pubkey::new_unique(),
        event_authority: Pubkey::new_unique(),
        global_volume_accumulator: Pubkey::new_unique(),
        user_volume_accumulator: Pubkey::new_unique(),
        fee_config: Pubkey::new_unique(),
        fee_program: Pubkey::new_unique(),
        bonding_curve_v2: Pubkey::new_unique(),
        protocol_fee_recipient: Pubkey::new_unique(),
        virtual_token_reserves: 1_000_000_000,
        virtual_sol_reserves: 30_000_000_000,
        real_token_reserves: 800_000_000,
        protocol_fee_bps: 0,
        has_creator: true,
        is_cashback_coin: false,
    }
}

fn dummy_pumpfun_v2() -> PumpFunPool {
    let mut p = dummy_pumpfun();
    p.use_v2 = true;
    p
}

fn dummy_cpmm() -> CpmmPool {
    CpmmPool {
        pool_state: Pubkey::new_unique(),
        amm_config: Pubkey::new_unique(),
        observation_state: Pubkey::new_unique(),
        base_mint: Pubkey::new_unique(),
        quote_mint: WSOL_MINT,
        base_vault: Pubkey::new_unique(),
        quote_vault: Pubkey::new_unique(),
        base_token_program: TOKEN_PROGRAM,
        quote_token_program: TOKEN_PROGRAM,
        base_reserve: 1_000_000_000,
        quote_reserve: 50_000_000_000,
        trade_fee_rate: 2500,
        creator_fee_rate: 0,
        creator_fee_on: 0,
        enable_creator_fee: false,
        base_transfer_fee: TokenTransferFee::default(),
        quote_transfer_fee: TokenTransferFee::default(),
    }
}

fn dummy_pumpswap() -> PumpSwapPool {
    PumpSwapPool {
        pool: Pubkey::new_unique(),
        base_mint: Pubkey::new_unique(),
        quote_mint: WSOL_MINT,
        pool_base_token_account: Pubkey::new_unique(),
        pool_quote_token_account: Pubkey::new_unique(),
        base_token_program: TOKEN_PROGRAM,
        quote_token_program: TOKEN_PROGRAM,
        coin_creator_vault_ata: Pubkey::new_unique(),
        coin_creator_vault_authority: Pubkey::new_unique(),
        coin_creator: Pubkey::new_unique(),
        base_reserve: 1_000_000_000,
        quote_reserve: 30_000_000_000,
        virtual_quote_reserves: 0,
        lp_fee_bps: 20,
        protocol_fee_bps: 5,
        creator_fee_bps: 5,
        is_cashback_coin: false,
        protocol_fee_recipient: Pubkey::new_unique(),
        buyback_fee_recipient: Pubkey::new_unique(),
    }
}

fn dummy_launchlab() -> LaunchLabPool {
    LaunchLabPool {
        base_mint: Pubkey::new_unique(),
        quote_mint: WSOL_MINT,
        base_token_program: TOKEN_PROGRAM,
        quote_token_program: TOKEN_PROGRAM,
        pool_state: Pubkey::new_unique(),
        base_vault: Pubkey::new_unique(),
        quote_vault: Pubkey::new_unique(),
        platform_config: Pubkey::new_unique(),
        platform_associated_account: Pubkey::new_unique(),
        creator_associated_account: Pubkey::new_unique(),
        global_config: Pubkey::new_unique(),
        virtual_base: 1_073_791_680_000_000,
        virtual_quote: 30_000_000_000,
        real_base: 800_000_000_000_000,
        real_quote: 0,
        total_base_sell: 793_100_000_000_000,
        curve_type: 0,
        trade_fee_rate: 2500,
        platform_fee_rate: 0,
        creator_fee_rate: 0,
        base_transfer_fee: TokenTransferFee::default(),
        quote_transfer_fee: TokenTransferFee::default(),
    }
}

fn dummy_amm_v4() -> RaydiumAmmV4Pool {
    RaydiumAmmV4Pool {
        amm: Pubkey::new_unique(),
        coin_mint: WSOL_MINT,
        pc_mint: Pubkey::new_unique(),
        token_coin: Pubkey::new_unique(),
        token_pc: Pubkey::new_unique(),
        amm_open_orders: Pubkey::default(),
        amm_target_orders: Pubkey::new_unique(),
        serum_program: Pubkey::default(),
        serum_market: Pubkey::default(),
        serum_bids: Pubkey::default(),
        serum_asks: Pubkey::default(),
        serum_event_queue: Pubkey::default(),
        serum_coin_vault_account: Pubkey::default(),
        serum_pc_vault_account: Pubkey::default(),
        serum_vault_signer: Pubkey::default(),
        coin_reserve: 100_000_000_000,
        pc_reserve: 10_000_000_000,
        trade_fee_numerator: 25,
        swap_fee_numerator: 25,
    }
}

fn dummy_damm_v2() -> MeteoraDammV2Pool {
    MeteoraDammV2Pool {
        pool: Pubkey::new_unique(),
        token_a_vault: Pubkey::new_unique(),
        token_b_vault: Pubkey::new_unique(),
        token_a_mint: WSOL_MINT,
        token_b_mint: Pubkey::new_unique(),
        token_a_program: TOKEN_PROGRAM,
        token_b_program: TOKEN_PROGRAM,
        token_a_reserve: 50_000_000_000,
        token_b_reserve: 1_000_000_000,
        fee_bps: 25,
        quoted_amount_in: None,
        expected_out: None,
        swap_mode: 0,
        referral_token_account: None,
        include_rate_limiter_sysvar: false,
    }
}

fn dummy_clmm() -> RaydiumClmmPool {
    RaydiumClmmPool {
        amm_config: Pubkey::new_unique(),
        pool_state: Pubkey::new_unique(),
        observation_state: Pubkey::new_unique(),
        token_0_mint: WSOL_MINT,
        token_1_mint: Pubkey::new_unique(),
        token_0_vault: Pubkey::new_unique(),
        token_1_vault: Pubkey::new_unique(),
        token_0_program: TOKEN_PROGRAM,
        token_1_program: TOKEN_PROGRAM,
        tick_arrays: vec![Pubkey::new_unique(); 3],
        tick_array_bitmap_extension: Some(Pubkey::new_unique()),
        quoted_amount_in: Some(1_000_000),
        expected_out: Some(500_000),
        fee_bps: 25,
    }
}

fn dummy_whirlpool() -> WhirlpoolPool {
    WhirlpoolPool {
        whirlpool: Pubkey::new_unique(),
        mint_a: WSOL_MINT,
        mint_b: Pubkey::new_unique(),
        vault_a: Pubkey::new_unique(),
        vault_b: Pubkey::new_unique(),
        token_program_a: TOKEN_PROGRAM,
        token_program_b: TOKEN_PROGRAM,
        tick_arrays: vec![Pubkey::new_unique(); 3],
        quoted_amount_in: Some(1_000_000),
        expected_out: Some(400_000),
        fee_bps: 30,
    }
}

fn dummy_dlmm() -> MeteoraDlmmPool {
    MeteoraDlmmPool {
        lb_pair: Pubkey::new_unique(),
        bitmap_extension: Some(Pubkey::new_unique()),
        reserve_x: Pubkey::new_unique(),
        reserve_y: Pubkey::new_unique(),
        token_x_mint: WSOL_MINT,
        token_y_mint: Pubkey::new_unique(),
        token_x_program: TOKEN_PROGRAM,
        token_y_program: TOKEN_PROGRAM,
        oracle: Pubkey::new_unique(),
        bin_arrays: vec![Pubkey::new_unique(); 3],
        quoted_amount_in: Some(1_000_000),
        expected_out: Some(350_000),
        fee_bps: 20,
    }
}

#[test]
fn offline_create_unique_wallets() {
    let a = Keypair::new();
    let b = Keypair::new();
    assert_ne!(a.pubkey(), b.pubkey());
}

#[test]
fn offline_mainnet_sim_create_wallet_helper() {
    let a = create_wallet();
    let b = create_wallet();
    assert_ne!(a.pubkey(), b.pubkey());
}

#[test]
fn offline_fee_and_slippage_helpers() {
    assert_eq!(fee_amount(1_000_000, 100), 10_000); // 1%
    assert_eq!(clamp_slippage_bps(50), 50);
    assert!(clamp_slippage_bps(10_000) <= 10_000);
    assert_eq!(apply_slippage_min_out(1_000_000, 100), 990_000);
}

#[test]
fn offline_pumpfun_quote_positive() {
    let pool = dummy_pumpfun();
    let fee_bps = pumpfun_total_fee_bps(pool.has_creator);
    assert!(fee_bps >= 95);
    let out = pumpfun_buy_token_out(&pool, 1_000_000);
    assert!(out > 0);
    let sol = pumpfun_sell_sol_out(&pool, out / 2);
    assert!(sol > 0);
}

#[test]
fn offline_cpmm_quote_positive() {
    let pool = dummy_cpmm();
    let out = cpmm_out(&pool, 1_000_000, true).expect("cpmm out");
    assert!(out > 0);
    assert!(out < pool.base_reserve);
}

#[test]
fn offline_pumpswap_and_amm_quotes() {
    let ps = dummy_pumpswap();
    let base = pumpswap_buy_base_out(&ps, 1_000_000).unwrap();
    assert!(base > 0);
    let quote = pumpswap_sell_quote_out(&ps, base / 2).unwrap();
    assert!(quote > 0);

    let amm = dummy_amm_v4();
    let out = raydium_amm_v4_out(&amm, 1_000_000, true).unwrap();
    assert!(out > 0);

    let damm = dummy_damm_v2();
    let out = meteora_damm_v2_out(&damm, 1_000_000, true).unwrap();
    assert!(out > 0);
}

#[test]
fn offline_leg_builders_emit_accounts() {
    let user = Pubkey::new_unique();
    let pf = dummy_pumpfun();
    let ata = crate::ata::ata(&user, &pf.mint, &TOKEN_PROGRAM);
    let leg = pumpfun_buy_leg(&user, &pf, 1_000_000, 0, ata);
    assert_eq!(leg.program_id, PUMPFUN_PROGRAM);
    assert!(leg.accounts.len() >= 16);
    assert!(leg.accounts.iter().any(|a| a.is_signer));

    let cpmm = dummy_cpmm();
    let in_ata = crate::ata::ata(&user, &WSOL_MINT, &TOKEN_PROGRAM);
    let out_ata = crate::ata::ata(&user, &cpmm.base_mint, &TOKEN_PROGRAM);
    let leg = cpmm_swap_leg(
        &user,
        &cpmm,
        1_000_000,
        0,
        WSOL_MINT,
        cpmm.base_mint,
        in_ata,
        out_ata,
    )
    .unwrap();
    assert_eq!(leg.program_id, RAYDIUM_CPMM_PROGRAM);
    assert!(leg.accounts.len() >= 13);
}

#[test]
fn offline_all_dex_leg_builders() {
    let user = Pubkey::new_unique();

    let pf = dummy_pumpfun();
    let ata = crate::ata::ata(&user, &pf.mint, &TOKEN_PROGRAM);
    assert_eq!(
        pumpfun_buy_leg(&user, &pf, 1000, 0, ata).program_id,
        PUMPFUN_PROGRAM
    );
    assert_eq!(
        pumpfun_sell_leg(&user, &pf, 1000, 0, ata).program_id,
        PUMPFUN_PROGRAM
    );
    let pf2 = dummy_pumpfun_v2();
    assert_eq!(
        pumpfun_buy_v2_leg(&user, &pf2, 1000, 0).program_id,
        PUMPFUN_PROGRAM
    );

    let ps = dummy_pumpswap();
    assert_eq!(
        pumpswap_buy_leg(&user, &ps, 1000, 0).unwrap().program_id,
        PUMPSWAP_PROGRAM
    );
    assert_eq!(
        pumpswap_sell_leg(&user, &ps, 1000, 0).unwrap().program_id,
        PUMPSWAP_PROGRAM
    );

    let ll = dummy_launchlab();
    let base_ata = crate::ata::ata(&user, &ll.base_mint, &TOKEN_PROGRAM);
    let quote_ata = crate::ata::ata(&user, &ll.quote_mint, &TOKEN_PROGRAM);
    assert!(
        launchlab_buy_leg(&user, &ll, 1000, 0, base_ata, quote_ata)
            .accounts
            .len()
            >= 14
    );
    assert!(
        launchlab_sell_leg(&user, &ll, 1000, 0, base_ata, quote_ata)
            .accounts
            .len()
            >= 14
    );

    let amm = dummy_amm_v4();
    assert_eq!(
        raydium_amm_v4_swap_leg(&user, &amm, 1000, 0, WSOL_MINT)
            .unwrap()
            .program_id,
        RAYDIUM_AMM_V4_PROGRAM
    );

    let damm = dummy_damm_v2();
    assert_eq!(
        meteora_damm_v2_swap_leg(&user, &damm, 1000, 0, WSOL_MINT)
            .unwrap()
            .program_id,
        METEORA_DAMM_V2_PROGRAM
    );

    let clmm = dummy_clmm();
    assert_eq!(
        raydium_clmm_swap_leg(&user, &clmm, 1000, 0, WSOL_MINT)
            .unwrap()
            .program_id,
        RAYDIUM_CLMM_PROGRAM
    );

    let wp = dummy_whirlpool();
    assert_eq!(
        whirlpool_swap_leg(&user, &wp, 1000, 0, WSOL_MINT)
            .unwrap()
            .program_id,
        ORCA_WHIRLPOOL_PROGRAM
    );

    let dlmm = dummy_dlmm();
    assert_eq!(
        meteora_dlmm_swap_leg(&user, &dlmm, 1000, 0, WSOL_MINT)
            .unwrap()
            .program_id,
        METEORA_DLMM_PROGRAM
    );
}

#[test]
fn offline_pumpswap_leg_builds() {
    let user = Pubkey::new_unique();
    let pool = dummy_pumpswap();
    let leg = pumpswap_buy_leg(&user, &pool, 1_000_000, 0).unwrap();
    assert_eq!(leg.program_id, PUMPSWAP_PROGRAM);
    assert!(!leg.accounts.is_empty());
}

#[test]
fn offline_routed_market_helpers() {
    let pool = dummy_cpmm();
    let routed = RoutedMarket::stonk_outer(pool.clone(), None);
    assert_eq!(routed.meme_mint(), pool.meme_mint());
    match &routed.market {
        Market::CpmmOuter(_) => {}
        _ => panic!("expected CpmmOuter"),
    }

    let pf = dummy_pumpfun();
    let routed = RoutedMarket {
        market: Market::PumpFunInner(pf.clone()),
        bridge: None,
    };
    assert_eq!(routed.meme_mint(), pf.mint);
    assert!(!routed.market.needs_sol_bridge());
}

#[test]
fn offline_router_client_builds_pumpfun_buy() {
    let payer = Pubkey::new_unique();
    let fee_recipient = Pubkey::new_unique();
    let client =
        RouterClient::new(payer, fee_recipient, 50).with_pool_guard(PoolGuardPolicy::disabled());
    let pf = dummy_pumpfun();
    let market = RoutedMarket {
        market: Market::PumpFunInner(pf),
        bridge: None,
    };
    let built = client
        .buy_with_opts(1_000_000, &market, TradeOpts::default().buy_with_sol())
        .expect("buy build");
    let ixs = built.into_instructions();
    assert!(!ixs.is_empty());
}

#[test]
fn offline_router_client_builds_pumpswap_buy() {
    let payer = Pubkey::new_unique();
    let client =
        RouterClient::new(payer, Pubkey::new_unique(), 50).with_pool_guard(PoolGuardPolicy::disabled());
    let ps = dummy_pumpswap();
    let market = RoutedMarket {
        market: Market::PumpSwapOuter(ps),
        bridge: None,
    };
    let built = client
        .buy_with_opts(1_000_000, &market, TradeOpts::default().buy_with_sol())
        .expect("pumpswap buy");
    assert!(!built.into_instructions().is_empty());
}

#[test]
fn offline_classify_soft_vs_hard() {
    assert!(matches!(
        classify_err("Error: insufficient funds"),
        SimVerdict::Soft(_)
    ));
    assert!(matches!(
        classify_err("custom program error: 0x1"),
        SimVerdict::Soft(_)
    ));
    assert!(matches!(
        classify_err("AccountOwnedByWrongProgram caused by account: amm_config"),
        SimVerdict::Hard(_)
    ));
    assert!(matches!(
        classify_err("InstructionFallbackNotFound Error Number: 101 0x65"),
        SimVerdict::Hard(_)
    ));
    assert!(matches!(
        classify_err("SqrtPriceOutOfBounds 0x177b"),
        SimVerdict::Hard(_)
    ));
    assert!(matches!(
        classify_err("error sending request for url"),
        SimVerdict::Hard(_) // classify_err is for sim errors; transport handled elsewhere
    ));
    assert!(matches!(
        classify_err("ExceededSlippage"),
        SimVerdict::Soft(_)
    ));
    assert!(matches!(
        classify_err("AccountDiscriminatorMismatch 0xbbf"),
        SimVerdict::Hard(_)
    ));
}

#[test]
fn offline_fault_mutate_discriminator() {
    let user = Pubkey::new_unique();
    let pool = dummy_cpmm();
    let mut leg = cpmm_swap_leg(
        &user,
        &pool,
        100,
        0,
        WSOL_MINT,
        pool.base_mint,
        Pubkey::new_unique(),
        Pubkey::new_unique(),
    )
    .unwrap();
    let original = leg.data[..8].to_vec();
    leg.data[..8].fill(0xff);
    assert_ne!(leg.data[..8], original);
}

#[test]
fn offline_fault_mutate_pool_slot() {
    let user = Pubkey::new_unique();
    let pf = dummy_pumpfun();
    let ata = crate::ata::ata(&user, &pf.mint, &TOKEN_PROGRAM);
    let mut leg = pumpfun_buy_leg(&user, &pf, 1000, 0, ata);
    let before = leg.accounts[3].pubkey;
    leg.accounts[3].pubkey = Pubkey::new_unique();
    assert_ne!(leg.accounts[3].pubkey, before);
    let _ = Leg {
        program_id: leg.program_id,
        accounts: leg.accounts,
        data: leg.data,
    };
}

#[test]
fn offline_market_mint_helpers_cover_dexes() {
    for market in [
        Market::PumpFunInner(dummy_pumpfun()),
        Market::PumpSwapOuter(dummy_pumpswap()),
        Market::CpmmOuter(dummy_cpmm()),
        Market::LaunchLabInner(dummy_launchlab()),
        Market::RaydiumAmmV4(dummy_amm_v4()),
        Market::MeteoraDammV2(dummy_damm_v2()),
        Market::RaydiumClmm(dummy_clmm()),
        Market::Whirlpool(dummy_whirlpool()),
        Market::MeteoraDlmm(dummy_dlmm()),
    ] {
        let _ = market.base_mint();
        let _ = market.quote_mint();
        let _ = market.base_token_program();
        let _ = market.needs_sol_bridge();
    }
}
