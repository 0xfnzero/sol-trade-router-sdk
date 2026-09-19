//! Offline unit tests (no RPC) — wallets, quotes, legs, trade build, classify.

#![cfg(test)]

use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Keypair;
use solana_sdk::signer::Signer;

use crate::ata::AtaPolicy;
use crate::constants::{
    METEORA_DAMM_V2_PROGRAM, METEORA_DLMM_PROGRAM, ORCA_WHIRLPOOL_PROGRAM, PUMPFUN_PROGRAM,
    PUMPSWAP_PROGRAM, RAYDIUM_AMM_V4_PROGRAM, RAYDIUM_CLMM_PROGRAM, RAYDIUM_CPMM_PROGRAM,
    TOKEN_PROGRAM, WSOL_MINT,
};
use crate::legs::{
    cpmm_swap_exact_out_leg, cpmm_swap_leg, launchlab_buy_leg, launchlab_sell_leg,
    meteora_damm_v2_swap_leg, meteora_dlmm_swap_leg, pumpfun_buy_leg, pumpfun_buy_v2_leg,
    pumpfun_sell_leg, pumpfun_sell_v2_leg, pumpswap_buy_exact_out_leg, pumpswap_buy_leg,
    pumpswap_sell_leg, raydium_amm_v4_swap_exact_out_leg, raydium_amm_v4_swap_leg,
    raydium_clmm_swap_leg, whirlpool_swap_leg, Leg,
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
        real_base: 100_000_000_000_000,
        real_quote: 5_000_000_000,
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
        token_program: TOKEN_PROGRAM,
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
    // Layout: InvalidSplTokenProgram must not be soft-hidden as generic custom.
    assert!(matches!(
        classify_err("custom program error: 0x26"),
        SimVerdict::Hard(_)
    ));
    assert!(matches!(
        classify_err("Error: InvalidSplTokenProgram"),
        SimVerdict::Hard(_)
    ));
    assert!(matches!(
        classify_err("ZeroBaseAmount"),
        SimVerdict::Soft(_)
    ));
    assert!(matches!(
        classify_err("Error Code: UnsupportedQuoteMint. Error Number: 6063 0x17af"),
        SimVerdict::Soft(_)
    ));
    assert!(matches!(
        classify_err("custom program error: 0x17af"),
        SimVerdict::Soft(_)
    ));
    assert!(matches!(
        classify_err(
            "RPC response error -32602: VersionedTransaction too large: 1756 bytes (max: encoded/raw 1644/1232)"
        ),
        SimVerdict::Soft(_)
    ));
    assert!(matches!(
        classify_err("UiTransactionError(ProgramAccountNotFound)"),
        SimVerdict::Soft(_)
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

#[test]
fn offline_exact_out_legs_use_expected_discriminators() {
    let user = Pubkey::new_unique();
    let cpmm = dummy_cpmm();
    let meme = cpmm.meme_mint();
    let leg = cpmm_swap_exact_out_leg(
        &user,
        &cpmm,
        1_000_000,
        500,
        WSOL_MINT,
        meme,
        crate::ata::ata(&user, &WSOL_MINT, &TOKEN_PROGRAM),
        crate::ata::ata(&user, &meme, &TOKEN_PROGRAM),
    )
    .unwrap();
    assert_eq!(&leg.data[..8], &crate::constants::CPMM_SWAP_BASE_OUT);

    let ps = dummy_pumpswap();
    let leg = pumpswap_buy_exact_out_leg(&user, &ps, 1_000, 2_000_000).unwrap();
    assert_eq!(&leg.data[..8], &crate::constants::PUMPSWAP_BUY);

    let amm = dummy_amm_v4();
    let leg =
        raydium_amm_v4_swap_exact_out_leg(&user, &amm, 100, 1_000_000, WSOL_MINT).unwrap();
    assert_eq!(leg.data[0], crate::constants::RAYDIUM_AMM_V4_SWAP_BASE_OUT_V2);
}

#[test]
fn offline_adapter_cpmm_and_route_ix_targets_router_program() {
    use sol_trade_sdk::trading::core::params::{DexParamEnum, RaydiumCpmmParams};

    let meme = Pubkey::new_unique();
    let params = RaydiumCpmmParams {
        pool_state: Pubkey::new_unique(),
        amm_config: Pubkey::new_unique(),
        base_mint: WSOL_MINT,
        quote_mint: meme,
        base_reserve: 10_000_000_000,
        quote_reserve: 1_000_000_000,
        base_vault: Pubkey::new_unique(),
        quote_vault: Pubkey::new_unique(),
        base_token_program: TOKEN_PROGRAM,
        quote_token_program: TOKEN_PROGRAM,
        observation_state: Pubkey::new_unique(),
        trade_fee_rate: 2_500,
        protocol_fee_rate: 0,
        fund_fee_rate: 0,
        creator_fee_rate: 0,
        creator_fee_on: 0,
        enable_creator_fee: false,
        base_transfer_fee: Default::default(),
        quote_transfer_fee: Default::default(),
    };
    let routed = crate::to_routed_market(&DexParamEnum::RaydiumCpmm(params), meme).unwrap();
    assert!(matches!(routed.market, Market::CpmmOuter(_)));

    let payer = Keypair::new();
    let fee_recipient = Pubkey::new_unique();
    let client = crate::RouterClient::new(payer.pubkey(), fee_recipient, 0)
        .with_pool_guard(PoolGuardPolicy::disabled());
    let built = client
        .buy_with_opts(
            1_000_000,
            &routed,
            crate::TradeOpts::default().buy_with_wsol().create_wsol(true),
        )
        .unwrap();
    let ixs = built.into_instructions();
    let route = ixs
        .iter()
        .find(|ix| ix.program_id == crate::PROGRAM_ID)
        .expect("Route ix must target router PROGRAM_ID");
    assert_eq!(route.data[0], crate::route_ix::TAG_ROUTE);
}

#[test]
fn offline_route_exact_out_flag_sets_fee_asset_bit() {
    let payer = Pubkey::new_unique();
    let fee_recipient = Pubkey::new_unique();
    let accounts = crate::route_ix::RouteAccounts {
        payer,
        fee_destination: fee_recipient,
        fee_source: payer,
        output_token_account: Pubkey::new_unique(),
        fee_program: crate::constants::SYSTEM_PROGRAM,
        fee_mint: crate::constants::SYSTEM_PROGRAM,
    };
    let leg = Leg {
        program_id: RAYDIUM_CPMM_PROGRAM,
        accounts: vec![],
        data: vec![1, 2, 3],
    };
    // Empty accounts would fail on-chain; encoding-only check here.
    let ix = crate::route_ix::build_route_instruction_ex(
        &crate::PROGRAM_ID,
        accounts,
        1_000,
        100,
        crate::route_ix::FEE_ASSET_SOL,
        true,
        &[Leg {
            program_id: leg.program_id,
            accounts: vec![solana_sdk::instruction::AccountMeta::new_readonly(
                payer, true,
            )],
            data: leg.data,
        }],
    );
    // data[0]=TAG, then amount_in(8), min_out(8), fee_asset at offset 17
    assert_eq!(ix.data[0], crate::route_ix::TAG_ROUTE);
    assert_eq!(
        ix.data[17],
        crate::route_ix::FEE_ASSET_SOL | crate::route_ix::FEE_ASSET_EXACT_OUT
    );
}

#[test]
fn offline_pumpswap_cashback_inserts_volume_ata() {
    let user = Pubkey::new_unique();
    let mut pool = dummy_pumpswap();
    pool.is_cashback_coin = true;
    pool.coin_creator = Pubkey::new_unique();
    let leg = pumpswap_buy_leg(&user, &pool, 1_000_000, 0).unwrap();
    let non_cashback = {
        let mut p = pool.clone();
        p.is_cashback_coin = false;
        pumpswap_buy_leg(&user, &p, 1_000_000, 0).unwrap()
    };
    assert!(
        leg.accounts.len() > non_cashback.accounts.len(),
        "cashback buy must insert volume quote ATA"
    );
    assert_eq!(leg.data[24], 1);
}

#[test]
fn offline_amm_v4_rejects_unset_mints() {
    let user = Pubkey::new_unique();
    let mut pool = dummy_amm_v4();
    pool.coin_mint = Pubkey::default();
    assert!(raydium_amm_v4_swap_leg(&user, &pool, 1000, 0, WSOL_MINT).is_err());
}

#[test]
fn offline_amm_v4_in_for_out_roundtrip() {
    let amm = dummy_amm_v4();
    let out = raydium_amm_v4_out(&amm, 1_000_000, true).unwrap();
    let back_in = crate::quote::raydium_amm_v4_in_for_out(&amm, out, true).unwrap();
    assert!(back_in >= 1_000_000);
    let out2 = raydium_amm_v4_out(&amm, back_in, true).unwrap();
    assert!(out2 >= out);
}

#[test]
fn offline_via_sol_amm_v4_bridge_builds_route() {
    let payer = Pubkey::new_unique();
    let client =
        RouterClient::new(payer, Pubkey::new_unique(), 0).with_pool_guard(PoolGuardPolicy::disabled());
    let stock = Pubkey::new_unique();
    let mut ll = dummy_launchlab();
    ll.quote_mint = stock;
    ll.quote_token_program = TOKEN_PROGRAM;
    let mut hop = dummy_amm_v4();
    hop.coin_mint = WSOL_MINT;
    hop.pc_mint = stock;
    hop.coin_reserve = 10_000_000_000;
    hop.pc_reserve = 50_000_000_000;
    let market = RoutedMarket::with_bridge(
        Market::LaunchLabInner(ll),
        crate::market::BridgePool::AmmV4(hop),
    );
    let built = client
        .buy_with_opts(100_000, &market, TradeOpts::default().buy_with_sol())
        .expect("via sol amm_v4 bridge buy");
    assert!(built.route.is_some());
    assert!(built.route.as_ref().unwrap().accounts.len() > 10);
}

#[test]
fn offline_classify_zerobase_and_layout_codes() {
    assert!(matches!(
        classify_err("custom program error: 0x1771"), // ZeroBaseAmount = 6001
        SimVerdict::Hard(_) // unknown custom → HARD (name not present)
    ));
    assert!(matches!(
        classify_err("Error Code: ZeroBaseAmount. Error Number: 6001"),
        SimVerdict::Soft(_)
    ));
    assert!(matches!(
        classify_err("Buy zero amount. Error Number: 6020"),
        SimVerdict::Soft(_)
    ));
    assert!(matches!(
        classify_err("IncorrectTokenProgramId"),
        SimVerdict::Hard(_)
    ));
}

#[test]
fn offline_all_exact_out_and_reverse_legs() {
    let user = Pubkey::new_unique();
    let cpmm = dummy_cpmm();
    let in_ata = crate::ata::ata(&user, &WSOL_MINT, &TOKEN_PROGRAM);
    let out_ata = crate::ata::ata(&user, &cpmm.base_mint, &TOKEN_PROGRAM);
    let leg = cpmm_swap_exact_out_leg(
        &user,
        &cpmm,
        1_000_000,
        100,
        WSOL_MINT,
        cpmm.base_mint,
        in_ata,
        out_ata,
    )
    .unwrap();
    assert_eq!(leg.program_id, RAYDIUM_CPMM_PROGRAM);

    let amm = dummy_amm_v4();
    let leg = raydium_amm_v4_swap_exact_out_leg(&user, &amm, 100, 1_000_000, WSOL_MINT).unwrap();
    assert_eq!(leg.program_id, RAYDIUM_AMM_V4_PROGRAM);
    assert_eq!(leg.data[0], crate::constants::RAYDIUM_AMM_V4_SWAP_BASE_OUT_V2);

    let ps = dummy_pumpswap();
    let leg = pumpswap_buy_exact_out_leg(&user, &ps, 1_000, 2_000_000).unwrap();
    assert_eq!(leg.program_id, PUMPSWAP_PROGRAM);

    // Reverse directions
    let clmm = dummy_clmm();
    assert!(raydium_clmm_swap_leg(&user, &clmm, 1000, 0, clmm.token_1_mint).is_ok());
    let wp = dummy_whirlpool();
    assert!(whirlpool_swap_leg(&user, &wp, 1000, 0, wp.mint_b).is_ok());
    let dlmm = dummy_dlmm();
    assert!(meteora_dlmm_swap_leg(&user, &dlmm, 1000, 0, dlmm.token_y_mint).is_ok());
}

#[test]
fn offline_router_builds_all_market_kinds() {
    let payer = Pubkey::new_unique();
    let client =
        RouterClient::new(payer, Pubkey::new_unique(), 0).with_pool_guard(PoolGuardPolicy::disabled());

    let markets = [
        RoutedMarket::new(Market::PumpFunInner(dummy_pumpfun())),
        // LaunchLab curve clamp is covered in dedicated offline/mainnet tests.
        RoutedMarket::new(Market::CpmmOuter(dummy_cpmm())),
        RoutedMarket::pumpswap(dummy_pumpswap()),
        RoutedMarket::raydium_amm_v4(dummy_amm_v4()),
        RoutedMarket::new(Market::MeteoraDammV2(dummy_damm_v2())),
        RoutedMarket::new(Market::RaydiumClmm(dummy_clmm())),
        RoutedMarket::new(Market::Whirlpool(dummy_whirlpool())),
        RoutedMarket::new(Market::MeteoraDlmm(dummy_dlmm())),
    ];
    for (i, market) in markets.iter().enumerate() {
        // Concentrated venues need matching quoted_amount_in; patch when missing.
        let mut market = market.clone();
        match &mut market.market {
            Market::RaydiumClmm(p) => {
                p.quoted_amount_in = Some(1_000_000);
                p.expected_out = Some(900_000);
            }
            Market::Whirlpool(p) => {
                p.quoted_amount_in = Some(1_000_000);
                p.expected_out = Some(900_000);
            }
            Market::MeteoraDlmm(p) => {
                p.quoted_amount_in = Some(1_000_000);
                p.expected_out = Some(900_000);
            }
            Market::MeteoraDammV2(p) => {
                p.quoted_amount_in = Some(1_000_000);
                p.expected_out = Some(900_000);
                p.token_a_reserve = 10_000_000;
                p.token_b_reserve = 10_000_000;
            }
            _ => {}
        }
        let buy_with = match &market.market {
            Market::PumpFunInner(_) | Market::LaunchLabInner(_) => TradeOpts::default().buy_with_sol(),
            _ => TradeOpts::default().buy_with_wsol(),
        };
        // LaunchLab stock-quoted needs bridge — skip if needs_sol_bridge.
        if market.market.needs_sol_bridge() && market.bridge.is_none() {
            println!("[offline_all_markets] skip {i}: needs bridge");
            continue;
        }
        match client.buy_with_opts(1_000_000, &market, buy_with) {
            Ok(built) => {
                assert!(
                    built.route.is_some(),
                    "market {i} must emit Route ix"
                );
                assert_eq!(
                    built.route.as_ref().unwrap().program_id,
                    crate::constants::PROGRAM_ID
                );
            }
            Err(err) => panic!("market {i} buy build failed: {err}"),
        }
    }
}

#[test]
fn offline_pool_guard_stonk_strict_blocks_untrusted_cpmm() {
    let payer = Pubkey::new_unique();
    let cpmm = dummy_cpmm();
    let market = RoutedMarket::new(Market::CpmmOuter(cpmm.clone()));
    let strict =
        RouterClient::new(payer, Pubkey::new_unique(), 0).with_pool_guard(PoolGuardPolicy::stonk_strict());
    assert!(strict
        .buy_with_opts(1_000, &market, TradeOpts::default().buy_with_wsol())
        .is_err());
    let ok = RouterClient::new(payer, Pubkey::new_unique(), 0)
        .with_pool_guard(PoolGuardPolicy::stonk_strict().trust(cpmm.pool_state));
    assert!(ok
        .buy_with_opts(1_000, &market, TradeOpts::default().buy_with_wsol())
        .is_ok());
}

#[test]
fn offline_fee_reduces_route_amount_in() {
    let payer = Pubkey::new_unique();
    let fee_recv = Pubkey::new_unique();
    let client =
        RouterClient::new(payer, fee_recv, 500).with_pool_guard(PoolGuardPolicy::disabled());
    let market = RoutedMarket::new(Market::CpmmOuter(dummy_cpmm()));
    let amount = 1_000_000u64;
    let built = client
        .buy_with_opts(amount, &market, TradeOpts::default().buy_with_wsol())
        .unwrap();
    let route = built.route.unwrap();
    // On-chain fee: Route amount_in is the full user budget; program skims fee_bps.
    let encoded = u64::from_le_bytes(route.data[1..9].try_into().unwrap());
    assert_eq!(encoded, amount);
    let fee_ata = crate::ata::ata(&fee_recv, &WSOL_MINT, &TOKEN_PROGRAM);
    assert!(
        route.accounts.iter().any(|a| a.pubkey == fee_ata || a.pubkey == fee_recv),
        "fee destination must be in Route accounts"
    );
    assert!(crate::quote::fee_amount(amount, 500) > 0);
}

#[test]
fn offline_adapter_amm_v4_and_cpmm_from_params() {
    use crate::adapter::{cpmm_from_params, raydium_amm_v4_from_params};

    let amm = dummy_amm_v4();
    let p = sol_trade_sdk::trading::core::params::RaydiumAmmV4Params {
        amm: amm.amm,
        coin_mint: amm.coin_mint,
        pc_mint: amm.pc_mint,
        token_coin: amm.token_coin,
        token_pc: amm.token_pc,
        amm_open_orders: amm.amm_open_orders,
        amm_target_orders: amm.amm_target_orders,
        serum_program: amm.serum_program,
        serum_market: amm.serum_market,
        serum_bids: amm.serum_bids,
        serum_asks: amm.serum_asks,
        serum_event_queue: amm.serum_event_queue,
        serum_coin_vault_account: amm.serum_coin_vault_account,
        serum_pc_vault_account: amm.serum_pc_vault_account,
        serum_vault_signer: amm.serum_vault_signer,
        coin_reserve: amm.coin_reserve,
        pc_reserve: amm.pc_reserve,
    };
    let back = raydium_amm_v4_from_params(&p);
    assert_eq!(back.amm, amm.amm);
    assert_eq!(back.coin_reserve, amm.coin_reserve);

    let cpmm = dummy_cpmm();
    let cp = sol_trade_sdk::trading::core::params::RaydiumCpmmParams {
        pool_state: cpmm.pool_state,
        amm_config: cpmm.amm_config,
        base_mint: cpmm.base_mint,
        quote_mint: cpmm.quote_mint,
        base_reserve: cpmm.base_reserve,
        quote_reserve: cpmm.quote_reserve,
        base_vault: cpmm.base_vault,
        quote_vault: cpmm.quote_vault,
        base_token_program: cpmm.base_token_program,
        quote_token_program: cpmm.quote_token_program,
        observation_state: cpmm.observation_state,
        trade_fee_rate: cpmm.trade_fee_rate,
        protocol_fee_rate: 0,
        fund_fee_rate: 0,
        creator_fee_rate: cpmm.creator_fee_rate,
        creator_fee_on: cpmm.creator_fee_on,
        enable_creator_fee: cpmm.enable_creator_fee,
        base_transfer_fee: sol_trade_sdk::trading::core::params::TokenTransferFee {
            basis_points: cpmm.base_transfer_fee.basis_points,
            maximum_fee: cpmm.base_transfer_fee.maximum_fee,
        },
        quote_transfer_fee: sol_trade_sdk::trading::core::params::TokenTransferFee {
            basis_points: cpmm.quote_transfer_fee.basis_points,
            maximum_fee: cpmm.quote_transfer_fee.maximum_fee,
        },
    };
    assert_eq!(cpmm_from_params(&cp).pool_state, cpmm.pool_state);
}

#[test]
fn offline_pumpfun_uses_v2_and_sell_v2_leg() {
    let mut pool = dummy_pumpfun();
    assert!(!pool.uses_v2());
    assert!(pool.is_native_sol_quote());

    pool.use_v2 = true;
    assert!(pool.uses_v2());
    assert!(pool.uses_wsol_ata_settlement());

    pool.use_v2 = false;
    pool.quote_mint = Pubkey::new_unique();
    assert!(pool.uses_v2());
    assert!(!pool.is_native_sol_quote());

    let user = Pubkey::new_unique();
    let leg = pumpfun_sell_v2_leg(&user, &pool, 1_000, 0);
    assert_eq!(leg.program_id, PUMPFUN_PROGRAM);
    assert!(!leg.accounts.is_empty());
    assert_eq!(leg.data.len(), 24);

    let v1 = dummy_pumpfun();
    let ata = crate::ata::ata(&user, &v1.mint, &TOKEN_PROGRAM);
    let sell = pumpfun_sell_leg(&user, &v1, 1_000, 0, ata);
    assert_eq!(sell.program_id, PUMPFUN_PROGRAM);
}

#[test]
fn offline_launchlab_sell_leg_builds() {
    let user = Pubkey::new_unique();
    let pool = dummy_launchlab();
    let base = crate::ata::ata(&user, &pool.base_mint, &TOKEN_PROGRAM);
    let quote = crate::ata::ata(&user, &pool.quote_mint, &TOKEN_PROGRAM);
    let leg = launchlab_sell_leg(&user, &pool, 1_000, 0, base, quote);
    assert_eq!(leg.program_id, crate::constants::LAUNCHLAB_PROGRAM);
}

#[test]
fn offline_quote_helpers_cover_concentrated_venues() {
    let clmm = dummy_clmm();
    assert!(crate::quote::raydium_clmm_out(&clmm, 1_000).is_err()); // needs quoted match
    let mut clmm = clmm;
    clmm.quoted_amount_in = Some(1_000);
    clmm.expected_out = Some(900);
    assert_eq!(crate::quote::raydium_clmm_out(&clmm, 1_000).unwrap(), 900);

    let mut wp = dummy_whirlpool();
    wp.quoted_amount_in = Some(2_000);
    wp.expected_out = Some(1_800);
    assert_eq!(crate::quote::whirlpool_out(&wp, 2_000).unwrap(), 1_800);

    let mut dlmm = dummy_dlmm();
    dlmm.quoted_amount_in = Some(3_000);
    dlmm.expected_out = Some(2_700);
    assert_eq!(crate::quote::meteora_dlmm_out(&dlmm, 3_000).unwrap(), 2_700);

    let mut damm = dummy_damm_v2();
    damm.token_a_reserve = 1_000_000;
    damm.token_b_reserve = 2_000_000;
    assert!(crate::quote::meteora_damm_v2_out(&damm, 1_000, true).unwrap() > 0);
}

#[test]
fn offline_transfer_fee_and_ata_policy() {
    let fee = TokenTransferFee {
        basis_points: 100,
        maximum_fee: 50,
    };
    assert_eq!(fee.calculate(10_000), 50); // capped
    assert_eq!(TokenTransferFee::none().calculate(10_000), 0);

    let buy = AtaPolicy::for_buy();
    assert!(buy.create_meme);
    let sell = AtaPolicy::for_sell();
    assert!(!sell.create_meme);
}

#[test]
fn offline_pool_guard_disabled_allows_all_dummy_markets() {
    let policy = PoolGuardPolicy::disabled();
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
        crate::pool_guard::assert_market_ok(&market, &policy).expect("disabled allows");
    }
}

#[test]
fn offline_router_sell_builds_for_major_dexes() {
    let payer = Pubkey::new_unique();
    let client =
        RouterClient::new(payer, Pubkey::new_unique(), 0).with_pool_guard(PoolGuardPolicy::disabled());

    let pf = RoutedMarket::new(Market::PumpFunInner(dummy_pumpfun()));
    assert!(client.sell_to_sol(1_000, &pf).is_ok());

    let ps = RoutedMarket::pumpswap(dummy_pumpswap());
    assert!(client.sell_to_wsol(1_000, &ps).is_ok());

    let cpmm = RoutedMarket::new(Market::CpmmOuter(dummy_cpmm()));
    assert!(client.sell_to_wsol(1_000, &cpmm).is_ok());

    let mut clmm = dummy_clmm();
    clmm.quoted_amount_in = Some(1_000);
    clmm.expected_out = Some(900);
    let clmm_m = RoutedMarket::new(Market::RaydiumClmm(clmm));
    assert!(client.sell_to_wsol(1_000, &clmm_m).is_ok());
}
