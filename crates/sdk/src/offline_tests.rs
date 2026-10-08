//! Offline unit tests (no RPC) — wallets, quotes, legs, trade build, classify.

#![cfg(test)]

use solana_sdk::instruction::AccountMeta;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Keypair;
use solana_sdk::signer::Signer;

use crate::ata::AtaPolicy;
use crate::constants::{
    PUMPFUN_PROGRAM, PUMPSWAP_PROGRAM, RAYDIUM_AMM_V4_PROGRAM, RAYDIUM_CPMM_PROGRAM, TOKEN_PROGRAM,
    WSOL_MINT,
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
use crate::pool_guard::PoolGuardPolicy;
use crate::quote::{
    apply_slippage_min_out, clamp_slippage_bps, cpmm_out, fee_amount, meteora_damm_v2_out,
    pumpfun_buy_token_out, pumpfun_sell_sol_out, pumpfun_total_fee_bps, pumpswap_buy_base_out,
    pumpswap_sell_quote_out, raydium_amm_v4_out,
};
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
        protocol_fee_bps: 95,
        creator_fee_bps: 30,
        fee_rates_known: true,
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
        fee_rates_known: true,
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
        fee_rates_known: true,
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
        swap_fee_denominator: 10_000,
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
        quoted_input_mint: None,
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
        quoted_input_mint: None,
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
        quoted_input_mint: None,
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
        quoted_input_mint: None,
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
    let out = pumpfun_buy_token_out(&pool, 1_000_000).unwrap();
    assert!(out > 0);
    let sol = pumpfun_sell_sol_out(&pool, out / 2).unwrap();
    assert!(sol > 0);
}

#[test]
fn offline_cpmm_quote_positive() {
    let pool = dummy_cpmm();
    let out = cpmm_out(&pool, 1_000_000, true).expect("cpmm out");
    assert!(out > 0);
    assert!(out < pool.base_reserve);
    // The quote API may return zero; automatic trade construction must reject it.
    let mut tiny = dummy_cpmm();
    tiny.base_reserve = 1_000;
    tiny.quote_reserve = 1_000;
    assert_eq!(cpmm_out(&tiny, 1, true).unwrap(), 0);
    assert_eq!(cpmm_out(&tiny, 1, false).unwrap(), 0);
    let market = RoutedMarket {
        market: Market::CpmmOuter(tiny),
        bridge: None,
    };
    let client = RouterClient::new(Pubkey::new_unique(), Pubkey::new_unique(), 0)
        .with_pool_guard(PoolGuardPolicy::disabled());
    for result in [client.buy_with_sol(1, &market), client.sell_to_sol(1, &market)] {
        assert!(result.unwrap_err().to_string().contains("zero output"));
    }
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
    assert!(meteora_damm_v2_out(&damm, 1_000_000, true).is_err());
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
    let wallet = Keypair::new();
    let user = wallet.pubkey();
    let pf = dummy_pumpfun();
    let meme_ata = crate::ata::ata(&user, &pf.mint, &TOKEN_PROGRAM);
    let pf2 = dummy_pumpfun_v2();
    let ps = dummy_pumpswap();
    let ll = dummy_launchlab();
    let base_ata = crate::ata::ata(&user, &ll.base_mint, &TOKEN_PROGRAM);
    let quote_ata = crate::ata::ata(&user, &ll.quote_mint, &TOKEN_PROGRAM);
    let amm = dummy_amm_v4();
    let damm = dummy_damm_v2();
    let clmm = dummy_clmm();
    let wp = dummy_whirlpool();
    let dlmm = dummy_dlmm();
    let cpmm = dummy_cpmm();
    let input = crate::ata::ata(&user, &cpmm.base_mint, &cpmm.base_token_program);
    let output = crate::ata::ata(&user, &cpmm.quote_mint, &cpmm.quote_token_program);
    let mut curve = pf.clone();
    curve.bonding_curve =
        Pubkey::find_program_address(&[b"bonding-curve", curve.mint.as_ref()], &PUMPFUN_PROGRAM).0;
    curve.associated_bonding_curve =
        crate::ata::ata(&curve.bonding_curve, &curve.mint, &TOKEN_PROGRAM);
    let pf3 = crate::pumpfun_v3::PumpFunV3Pool {
        curve,
        complete: false,
        supports_graduation: true,
        real_quote_reserves: 1_000_000,
        curve_base_token_balance: 1_000_000_000,
        pool_migration_fee: 1_000,
        mayhem_mode: false,
        depth: 0,
        needs_curve_extension: false,
        needs_volume_initialization: false,
    };
    let cases = [
        ("pumpfun_v3_buy", pf3.buy_leg(&user, 1000, 1).unwrap()),
        ("pumpfun_v3_sell", pf3.sell_leg(&user, 1000, 1).unwrap()),
        (
            "pumpfun_v3_exact_out",
            pf3.buy_exact_out_leg(&user, 1, 1000).unwrap(),
        ),
        (
            "pumpswap_exact_out",
            pumpswap_buy_exact_out_leg(&user, &ps, 1, 1000).unwrap(),
        ),
        (
            "pumpfun_v1_buy",
            pumpfun_buy_leg(&user, &pf, 1000, 1, meme_ata),
        ),
        (
            "pumpfun_v1_sell",
            pumpfun_sell_leg(&user, &pf, 1000, 1, meme_ata),
        ),
        ("pumpfun_v2_buy", pumpfun_buy_v2_leg(&user, &pf2, 1000, 1)),
        ("pumpfun_v2_sell", pumpfun_sell_v2_leg(&user, &pf2, 1000, 1)),
        (
            "pumpswap_buy",
            pumpswap_buy_leg(&user, &ps, 1000, 1).unwrap(),
        ),
        (
            "pumpswap_sell",
            pumpswap_sell_leg(&user, &ps, 1000, 1).unwrap(),
        ),
        (
            "launchlab_buy",
            launchlab_buy_leg(&user, &ll, 1000, 1, base_ata, quote_ata),
        ),
        (
            "launchlab_sell",
            launchlab_sell_leg(&user, &ll, 1000, 1, base_ata, quote_ata),
        ),
        (
            "cpmm",
            cpmm_swap_leg(
                &user,
                &cpmm,
                1000,
                1,
                cpmm.base_mint,
                cpmm.quote_mint,
                input,
                output,
            )
            .unwrap(),
        ),
        (
            "amm_v4",
            raydium_amm_v4_swap_leg(&user, &amm, 1000, 1, WSOL_MINT).unwrap(),
        ),
        (
            "damm_v2",
            meteora_damm_v2_swap_leg(&user, &damm, 1000, 1, WSOL_MINT).unwrap(),
        ),
        (
            "clmm",
            raydium_clmm_swap_leg(&user, &clmm, 1000, 1, WSOL_MINT).unwrap(),
        ),
        (
            "whirlpool",
            whirlpool_swap_leg(&user, &wp, 1000, 1, WSOL_MINT).unwrap(),
        ),
        (
            "dlmm",
            meteora_dlmm_swap_leg(&user, &dlmm, 1000, 1, WSOL_MINT).unwrap(),
        ),
    ];
    for (label, leg) in cases {
        let expected_program = match label {
            name if name.starts_with("pumpfun_") => PUMPFUN_PROGRAM,
            name if name.starts_with("pumpswap_") => PUMPSWAP_PROGRAM,
            name if name.starts_with("launchlab_") => crate::constants::LAUNCHLAB_PROGRAM,
            "cpmm" => RAYDIUM_CPMM_PROGRAM,
            "amm_v4" => RAYDIUM_AMM_V4_PROGRAM,
            "damm_v2" => crate::constants::METEORA_DAMM_V2_PROGRAM,
            "clmm" => crate::constants::RAYDIUM_CLMM_PROGRAM,
            "whirlpool" => crate::constants::ORCA_WHIRLPOOL_PROGRAM,
            "dlmm" => crate::constants::METEORA_DLMM_PROGRAM,
            _ => panic!("unregistered leg: {label}"),
        };
        assert_eq!(leg.program_id, expected_program, "{label} program");
        if label.starts_with("launchlab_") {
            assert!(leg.accounts.len() >= 14);
        }
        use solana_message::{v0, VersionedMessage};
        use solana_sdk::{
            hash::Hash,
            instruction::Instruction,
            transaction::{Transaction, VersionedTransaction},
        };
        let ix = Instruction {
            program_id: leg.program_id,
            accounts: leg.accounts,
            data: leg.data,
        };
        let hash = Hash::new_unique();
        let legacy =
            Transaction::new_signed_with_payer(&[ix.clone()], Some(&user), &[&wallet], hash);
        legacy.verify().expect(label);
        let wire = bincode::serialize(&legacy).expect(label);
        let mut restored: Transaction = bincode::deserialize(&wire).expect(label);
        assert_eq!(restored, legacy, "{label} legacy wire roundtrip");
        restored.verify().expect(label);
        restored.message.recent_blockhash = Hash::new_unique();
        assert!(
            restored.verify().is_err(),
            "{label} tampered legacy signature"
        );
        let v0 = v0::Message::try_compile(&user, &[ix.clone()], &[], hash).expect(label);
        let tx = VersionedTransaction::try_new(VersionedMessage::V0(v0), &[&wallet]).expect(label);
        tx.verify_and_hash_message().expect(label);
        let wire = bincode::serialize(&tx).expect(label);
        let mut decoded: VersionedTransaction = bincode::deserialize(&wire).expect(label);
        decoded.verify_and_hash_message().expect(label);
        assert_eq!(decoded, tx, "{label} signed wire roundtrip");
        decoded.message.set_recent_blockhash(Hash::new_unique());
        assert!(
            decoded.verify_and_hash_message().is_err(),
            "{label} modified message signature must fail"
        );
        let message = solana_message::v1::Message::try_compile_with_config(
            &user,
            &[ix],
            hash,
            solana_message::v1::TransactionConfig::empty().with_compute_unit_limit(1_400_000),
        )
        .expect(label);
        let v1 =
            VersionedTransaction::try_new(VersionedMessage::V1(message), &[&wallet]).expect(label);
        v1.verify_and_hash_message().expect(label);
        let wire = wincode::serialize(&v1).expect(label);
        let mut restored: VersionedTransaction = wincode::deserialize(&wire).expect(label);
        assert_eq!(restored, v1, "{label} signed V1 wire roundtrip");
        restored.verify_and_hash_message().expect(label);
        restored.message.set_recent_blockhash(Hash::new_unique());
        assert!(
            restored.verify_and_hash_message().is_err(),
            "{label} tampered V1 signature"
        );
        crate::mainnet_sim::record_evidence(
            "offline-signed-trades",
            &serde_json::json!({
                "case": label, "legacy_signatures_verified": true, "v0_signatures_verified": true, "v1_signatures_verified": true, "v1_transaction": v1,
                "wire_roundtrip_verified": true, "tampered_message_rejected": true, "transaction": tx,
            }),
        );
    }
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

    let wallet = Keypair::new();
    let client = RouterClient::new(wallet.pubkey(), Pubkey::new_unique(), 100)
        .with_pool_guard(PoolGuardPolicy::disabled());
    let stock = Pubkey::new_unique();
    let meme = Pubkey::new_unique();
    for stock_on_base in [false, true] {
        for stock_program in [TOKEN_PROGRAM, crate::constants::TOKEN_2022_PROGRAM] {
            let meme_program = if stock_program == TOKEN_PROGRAM {
                crate::constants::TOKEN_2022_PROGRAM
            } else { TOKEN_PROGRAM };
            let mut target = dummy_cpmm();
            target.base_mint = if stock_on_base { stock } else { meme };
            target.quote_mint = if stock_on_base { meme } else { stock };
            target.base_token_program = if stock_on_base { stock_program } else { meme_program };
            target.quote_token_program = if stock_on_base { meme_program } else { stock_program };
            let mut bridge = dummy_cpmm();
            bridge.base_mint = WSOL_MINT;
            bridge.quote_mint = stock;
            bridge.quote_token_program = stock_program;
            let market = RoutedMarket::with_bridge(Market::CpmmOuter(target), bridge);
            assert_eq!(market.meme_mint(), meme);
            assert_eq!(market.meme_token_program(), meme_program);
            assert_eq!(market.quote_mint(), stock);
            assert_eq!(market.quote_token_program(), stock_program);
            let stock_ata = crate::ata::ata(&client.payer, &stock, &stock_program);
            let meme_ata = crate::ata::ata(&client.payer, &meme, &meme_program);
            let fee_stock = crate::ata::ata(&client.fee_recipient, &stock, &stock_program);
            let created = client.create_quote_ata(&market);
            assert_eq!(created.accounts[1].pubkey, stock_ata);
            assert_eq!(created.accounts[3].pubkey, stock);
            assert_eq!(created.accounts[5].pubkey, stock_program);
            let closed = client.close_quote_ata(&market);
            assert_eq!(closed.accounts[0].pubkey, stock_ata);
            assert_eq!(closed.program_id, stock_program);
            for prepared in [
                client.prepare_buy_atas(&market, crate::BuyWith::Token(stock)),
                client.prepare_sell_atas(&market, crate::SellTo::Token(stock)),
                client.prepare_buy_atas(&market, crate::BuyWith::Sol),
                client.prepare_sell_atas(&market, crate::SellTo::Sol),
            ] {
                assert!(prepared.iter().any(|ix| ix.accounts[1].pubkey == stock_ata
                    && ix.accounts[3].pubkey == stock && ix.accounts[5].pubkey == stock_program));
            }
            let buy = client.buy_with_token(100_000, &market, stock).unwrap();
            let buy_route = buy.route.as_ref().unwrap();
            assert_eq!(buy_route.accounts[2].pubkey, fee_stock);
            assert_eq!(buy_route.accounts[3].pubkey, stock_ata);
            assert_eq!(buy_route.accounts[4].pubkey, meme_ata);
            assert_eq!(buy_route.accounts[5].pubkey, stock_program);
            assert_eq!(&buy_route.data[19..51], meme.as_ref());
            let sell = client.sell_to_token(100_000, &market, stock).unwrap();
            let sell_route = sell.route.as_ref().unwrap();
            assert_eq!(sell_route.accounts[3].pubkey, meme_ata);
            assert_eq!(sell_route.accounts[4].pubkey, stock_ata);
            assert_eq!(sell_route.accounts[5].pubkey, meme_program);
            assert_eq!(&sell_route.data[19..51], stock.as_ref());
            assert!(client.buy_with_token(100_000, &market, meme).is_err());
            assert!(client.sell_to_token(100_000, &market, meme).is_err());
            for built in [buy, sell] {
                let tx = solana_sdk::transaction::Transaction::new_signed_with_payer(
                    &built.into_instructions(), Some(&wallet.pubkey()), &[&wallet],
                    solana_sdk::hash::Hash::new_unique(),
                );
                tx.verify().unwrap();
            }
        }
    }

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
    let mut pf = dummy_pumpfun();
    pf.virtual_token_reserves = 1_000;
    pf.virtual_sol_reserves = 1_000;
    pf.real_token_reserves = 800;
    pf.protocol_fee_bps = 0;
    pf.creator_fee_bps = 0;
    let tiny = RoutedMarket {
        market: Market::PumpFunInner(pf),
        bridge: None,
    };
    // One raw output unit must survive automatic slippage rounding.
    let buy = client.buy_with_sol(3, &tiny).unwrap().route.unwrap();
    assert_eq!(u64::from_le_bytes(buy.data[9..17].try_into().unwrap()), 1);
    let sell = client.sell_to_sol(2, &tiny).unwrap().route.unwrap();
    assert_eq!(u64::from_le_bytes(sell.data[9..17].try_into().unwrap()), 1);
    assert!(client.buy_with_sol(2, &tiny).is_err());
    assert!(client.sell_to_sol(1, &tiny)
        .unwrap_err().to_string().contains("zero output"));
    // Explicit thresholds remain authoritative, including a caller-selected zero.
    let explicit = client
        .buy_with_opts(2, &tiny, TradeOpts::default().buy_with_sol().with_min_out(0))
        .unwrap()
        .route.unwrap();
    assert_eq!(u64::from_le_bytes(explicit.data[9..17].try_into().unwrap()), 0);
}

#[test]
fn offline_router_client_builds_pumpswap_buy() {
    let payer = Pubkey::new_unique();
    let client = RouterClient::new(payer, Pubkey::new_unique(), 50)
        .with_pool_guard(PoolGuardPolicy::disabled());
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
        assert_ne!(market.base_mint(), market.quote_mint());
        assert_eq!(market.quote_mint(), WSOL_MINT);
        assert_eq!(market.base_token_program(), TOKEN_PROGRAM);
        assert_eq!(market.quote_token_program(), TOKEN_PROGRAM);
        assert!(!market.needs_sol_bridge());
    }

    // WSOL takes precedence over USDC regardless of stored pool order.
    // Otherwise a USDC/WSOL pool may select WSOL as both trade sides.
    let wallet = Keypair::new();
    let client = RouterClient::new(wallet.pubkey(), Pubkey::new_unique(), 100)
        .with_pool_guard(PoolGuardPolicy::disabled());
    for wsol_first in [false, true] {
        let (a, b) = if wsol_first {
            (WSOL_MINT, crate::constants::USDC_MINT)
        } else {
            (crate::constants::USDC_MINT, WSOL_MINT)
        };
        let mut amm = dummy_amm_v4();
        amm.coin_mint = a;
        amm.pc_mint = b;
        let mut damm = dummy_damm_v2();
        damm.token_a_mint = a;
        damm.token_b_mint = b;
        for raw in [Market::RaydiumAmmV4(amm), Market::MeteoraDammV2(damm)] {
            let (input_index, output_index) = match &raw {
                Market::RaydiumAmmV4(_) => (5, 6),
                Market::MeteoraDammV2(_) => (2, 3),
                _ => unreachable!(),
            };
            let market = RoutedMarket::new(raw);
            assert_eq!(market.meme_mint(), crate::constants::USDC_MINT);
            assert_eq!(market.quote_mint(), WSOL_MINT);
            assert_eq!(market.meme_token_program(), TOKEN_PROGRAM);
            assert_eq!(market.quote_token_program(), TOKEN_PROGRAM);
            assert!(!market.market.needs_sol_bridge());
            assert_eq!(
                client.create_meme_ata(&market).accounts[3].pubkey,
                crate::constants::USDC_MINT,
            );
            assert_eq!(client.create_quote_ata(&market).accounts[3].pubkey, WSOL_MINT);
            for exact_out in [false, true] {
                let opts = if exact_out {
                    TradeOpts::default().with_fixed_output(7)
                } else {
                    TradeOpts::default().with_min_out(1)
                };
                for sell in [false, true] {
                    let built = if sell {
                        client.sell_with_opts(1_000, &market, opts.clone().sell_to_wsol())
                    } else {
                        client.buy_with_opts(1_000, &market, opts.clone().buy_with_wsol())
                    }
                    .unwrap();
                    let (input, output) = if sell {
                        (crate::constants::USDC_MINT, WSOL_MINT)
                    } else {
                        (WSOL_MINT, crate::constants::USDC_MINT)
                    };
                    let route = built.route.as_ref().unwrap();
                    let input_ata = crate::ata::ata(&wallet.pubkey(), &input, &TOKEN_PROGRAM);
                    let output_ata = crate::ata::ata(&wallet.pubkey(), &output, &TOKEN_PROGRAM);
                    assert_eq!(route.accounts[3].pubkey, input_ata);
                    assert_eq!(route.accounts[4].pubkey, output_ata);
                    assert_eq!(&route.data[19..51], output.as_ref());
                    let leg = crate::mainnet_sim::single_route_leg_for_direct_simulation(&built);
                    assert_eq!(leg.accounts[input_index].pubkey, input_ata);
                    assert_eq!(leg.accounts[output_index].pubkey, output_ata);
                    let tx = solana_sdk::transaction::Transaction::new_signed_with_payer(
                        &built.into_instructions(),
                        Some(&wallet.pubkey()),
                        &[&wallet],
                        solana_sdk::hash::Hash::new_unique(),
                    );
                    tx.verify().unwrap();
                }
            }
        }
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
    let leg = raydium_amm_v4_swap_exact_out_leg(&user, &amm, 100, 1_000_000, WSOL_MINT).unwrap();
    assert_eq!(
        leg.data[0],
        crate::constants::RAYDIUM_AMM_V4_SWAP_BASE_OUT_V2
    );
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
            crate::TradeOpts::default()
                .buy_with_wsol()
                .create_wsol(true),
        )
        .unwrap();
    let ixs = built.into_instructions();
    let route = ixs
        .iter()
        .find(|ix| ix.program_id == crate::PROGRAM_ID)
        .expect("Route ix must target router PROGRAM_ID");
    assert_eq!(route.data[0], crate::route_ix::TAG_ROUTE);
    assert_eq!(&route.data[19..51], meme.as_ref());
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
        &crate::constants::SYSTEM_PROGRAM,
        &[Leg {
            program_id: leg.program_id,
            accounts: vec![solana_sdk::instruction::AccountMeta::new_readonly(
                payer, true,
            )],
            data: leg.data,
        }],
    );
    // Legacy tag-2 header includes the expected output mint at bytes 19..51.
    assert_eq!(ix.data[0], crate::route_ix::TAG_ROUTE);
    assert_eq!(
        ix.data[17],
        crate::route_ix::FEE_ASSET_SOL | crate::route_ix::FEE_ASSET_EXACT_OUT
    );
    assert_eq!(&ix.data[19..51], crate::constants::SYSTEM_PROGRAM.as_ref());

    let wallet = Keypair::new();
    let client = RouterClient::new(wallet.pubkey(), fee_recipient, 100)
        .with_pool_guard(PoolGuardPolicy::disabled());
    let markets = [
        Market::CpmmOuter(dummy_cpmm()),
        Market::RaydiumAmmV4(dummy_amm_v4()),
        Market::MeteoraDammV2(dummy_damm_v2()),
        Market::PumpSwapOuter(dummy_pumpswap()),
        Market::PumpFunInner(dummy_pumpfun()),
        Market::LaunchLabInner(dummy_launchlab()),
        Market::RaydiumClmm(dummy_clmm()),
        Market::Whirlpool(dummy_whirlpool()),
        Market::MeteoraDlmm(dummy_dlmm()),
    ];
    for (index, market) in markets.into_iter().enumerate() {
        let market = RoutedMarket::new(market);
        for sell in [false, true] {
            let supported = index < 3 || (index == 3 && !sell);
            for direct_token in [false, true] {
                let mut opts = TradeOpts::default().with_min_out(0).with_fixed_output(77);
                if direct_token {
                    opts = opts.buy_with_token(market.market.quote_mint())
                        .sell_to_token(market.market.quote_mint());
                }
                let result = if sell {
                    client.sell_with_opts(1_000, &market, opts)
                } else {
                    client.buy_with_opts(1_000, &market, opts)
                };
                if !supported {
                    assert!(result.unwrap_err().to_string().contains("fixed_output is unsupported"));
                    continue;
                }
                let built = result.unwrap();
                let route = built.route.as_ref().unwrap();
                assert_eq!(u64::from_le_bytes(route.data[1..9].try_into().unwrap()), 1_000);
                assert_eq!(u64::from_le_bytes(route.data[9..17].try_into().unwrap()), 77);
                assert_ne!(route.data[17] & crate::route_ix::FEE_ASSET_EXACT_OUT, 0);
                assert_eq!(route.data[18], 1);
                let output = if sell { market.market.quote_mint() } else { market.meme_mint() };
                assert_eq!(&route.data[19..51], output.as_ref());
                // Official ABI amount ordering differs between these venues.
                let mut expected = match index {
                    0 => crate::constants::CPMM_SWAP_BASE_OUT.to_vec(),
                    1 => vec![17],
                    2 => crate::constants::METEORA_DAMM_V2_SWAP2.to_vec(),
                    3 => crate::constants::PUMPSWAP_BUY.to_vec(),
                    _ => unreachable!(),
                };
                let (first, second) = if index <= 1 { (990u64, 77u64) } else { (77u64, 990u64) };
                expected.extend_from_slice(&first.to_le_bytes());
                expected.extend_from_slice(&second.to_le_bytes());
                if index == 2 { expected.push(crate::constants::METEORA_DAMM_V2_EXACT_OUT); }
                assert!(route.data.windows(expected.len()).any(|data| data == expected),
                    "venue={index} sell={sell} token={direct_token} payload={:?}", &route.data[86..]);
                // Sign the actual high-level route, including fee/ATA setup.
                let tx = solana_sdk::transaction::Transaction::new_signed_with_payer(
                    &built.into_instructions(), Some(&wallet.pubkey()), &[&wallet],
                    solana_sdk::hash::Hash::new_unique(),
                );
                tx.verify().unwrap();
                let restored: solana_sdk::transaction::Transaction =
                    bincode::deserialize(&bincode::serialize(&tx).unwrap()).unwrap();
                restored.verify().unwrap();
                assert_eq!(restored, tx);
            }
        }
    }
    let market = RoutedMarket::new(Market::CpmmOuter(dummy_cpmm()));
    for opts in [
        TradeOpts::default().with_fixed_output(0),
        TradeOpts::default().with_fixed_output(77).with_min_out(78),
    ] {
        assert!(client.buy_with_opts(1_000, &market, opts.clone()).is_err());
        assert!(client.sell_with_opts(1_000, &market, opts).is_err());
    }
    let stock = Pubkey::new_unique();
    let mut target = dummy_cpmm();
    target.quote_mint = stock;
    let mut bridge = dummy_amm_v4();
    bridge.pc_mint = stock;
    let bridged = RoutedMarket::with_bridge(Market::CpmmOuter(target), bridge);
    let opts = TradeOpts::default().with_min_out(0).with_fixed_output(77);
    assert!(client.buy_with_opts(1_000, &bridged, opts.clone()).unwrap_err()
        .to_string().contains("fixed_output is unsupported"));
    assert!(client.sell_with_opts(1_000, &bridged, opts.clone()).unwrap_err()
        .to_string().contains("fixed_output is unsupported"));
    // An attached bridge does not prevent supported direct quote-token trades.
    assert!(client.buy_with_opts(1_000, &bridged, opts.clone().buy_with_token(stock)).is_ok());
    assert!(client.sell_with_opts(1_000, &bridged, opts.sell_to_token(stock)).is_ok());
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
    let mut extreme = amm.clone();
    extreme.coin_reserve = u64::MAX / 2;
    extreme.pc_reserve = 1_000_000;
    extreme.swap_fee_numerator = 0;
    assert!(crate::quote::raydium_amm_v4_in_for_out(&extreme, 999_999, true).is_err());
    let valid = crate::quote::raydium_amm_v4_in_for_out(&extreme, 400_000, true).unwrap();
    assert!(raydium_amm_v4_out(&extreme, valid, true).unwrap() >= 400_000);
    assert!(raydium_amm_v4_out(&extreme, valid - 1, true).unwrap() < 400_000);
    // The gross input can fit u64 while the input vault's resulting balance cannot.
    assert!(crate::quote::raydium_amm_v4_in_for_out(&extreme, 600_000, true).is_err());
    assert!(raydium_amm_v4_out(&extreme, u64::MAX, true).is_err());
    extreme.swap_fee_numerator = u64::MAX - 1;
    extreme.swap_fee_denominator = u64::MAX;
    assert!(crate::quote::raydium_amm_v4_in_for_out(&extreme, 1, true).is_err());

    let mut small = amm.clone();
    small.coin_reserve = 100;
    small.pc_reserve = 300;
    for numerator in [0, 1, 25, 5_000, 9_999] {
        small.swap_fee_numerator = numerator;
        for direction in [true, false] {
            for wanted in 1..=60 {
                let input = crate::quote::raydium_amm_v4_in_for_out(&small, wanted, direction).unwrap();
                assert!(raydium_amm_v4_out(&small, input, direction).unwrap() >= wanted);
                if input > 1 {
                    assert!(raydium_amm_v4_out(&small, input - 1, direction).unwrap() < wanted);
                }
            }
        }
    }
}

#[test]
fn offline_via_sol_amm_v4_bridge_builds_route() {
    let payer = Pubkey::new_unique();
    let client = RouterClient::new(payer, Pubkey::new_unique(), 0)
        .with_pool_guard(PoolGuardPolicy::disabled());
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
        SimVerdict::Hard(_)                           // unknown custom → HARD (name not present)
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
    assert_eq!(
        leg.data[0],
        crate::constants::RAYDIUM_AMM_V4_SWAP_BASE_OUT_V2
    );

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
    let client = RouterClient::new(payer, Pubkey::new_unique(), 0)
        .with_pool_guard(PoolGuardPolicy::disabled());

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
                p.quoted_input_mint = Some(WSOL_MINT);
                p.expected_out = Some(900_000);
            }
            Market::Whirlpool(p) => {
                p.quoted_amount_in = Some(1_000_000);
                p.quoted_input_mint = Some(WSOL_MINT);
                p.expected_out = Some(900_000);
            }
            Market::MeteoraDlmm(p) => {
                p.quoted_amount_in = Some(1_000_000);
                p.quoted_input_mint = Some(WSOL_MINT);
                p.expected_out = Some(900_000);
            }
            Market::MeteoraDammV2(p) => {
                p.quoted_amount_in = Some(1_000_000);
                p.quoted_input_mint = Some(WSOL_MINT);
                p.expected_out = Some(900_000);
                p.token_a_reserve = 10_000_000;
                p.token_b_reserve = 10_000_000;
            }
            _ => {}
        }
        let buy_with = match &market.market {
            Market::PumpFunInner(_) | Market::LaunchLabInner(_) => {
                TradeOpts::default().buy_with_sol()
            }
            _ => TradeOpts::default().buy_with_wsol(),
        };
        // LaunchLab stock-quoted needs bridge — skip if needs_sol_bridge.
        if market.market.needs_sol_bridge() && market.bridge.is_none() {
            println!("[offline_all_markets] skip {i}: needs bridge");
            continue;
        }
        match client.buy_with_opts(1_000_000, &market, buy_with) {
            Ok(built) => {
                assert!(built.route.is_some(), "market {i} must emit Route ix");
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
    let strict = RouterClient::new(payer, Pubkey::new_unique(), 0)
        .with_pool_guard(PoolGuardPolicy::stonk_strict());
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
        route
            .accounts
            .iter()
            .any(|a| a.pubkey == fee_ata || a.pubkey == fee_recv),
        "fee destination must be in Route accounts"
    );
    assert!(crate::quote::fee_amount(amount, 500) > 0);

    let client = RouterClient::new(payer, fee_recv, 100)
        .with_pool_guard(PoolGuardPolicy::disabled());
    let mut launch = dummy_launchlab();
    launch.virtual_base = 1_100;
    launch.virtual_quote = 1_000;
    launch.real_base = 0;
    launch.real_quote = 0;
    launch.total_base_sell = 100;
    launch.trade_fee_rate = 0;
    let quote = crate::quote::launchlab_buy_quote(&launch, 990, 0).unwrap();
    assert_eq!(quote.amount_in, 100);
    let clamped = client.buy_with_opts(
        1_000, &RoutedMarket::new(Market::LaunchLabInner(launch)),
        TradeOpts::default().buy_with_wsol(),
    ).unwrap().route.unwrap();
    let budget = u64::from_le_bytes(clamped.data[1..9].try_into().unwrap());
    let charged = crate::quote::fee_amount(budget, 100);
    assert_eq!(budget, 101);
    assert_eq!(u64::from_le_bytes(clamped.data[94..102].try_into().unwrap()), quote.amount_in);
    assert_eq!(budget, quote.amount_in + charged, "exact-in route must spend its declared budget");
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
        swap_fee_numerator: 3,
        swap_fee_denominator: 1_000,
    };
    let back = raydium_amm_v4_from_params(&p);
    assert_eq!(back.amm, amm.amm);
    assert_eq!(back.coin_reserve, amm.coin_reserve);
    assert_eq!(back.swap_fee_numerator, 3);
    assert_eq!(back.swap_fee_denominator, 1_000);

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
    clmm.quoted_input_mint = Some(clmm.token_1_mint);
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
    assert!(crate::quote::meteora_damm_v2_out(&damm, 1_000, true).is_err());
    damm.quoted_amount_in = Some(1_000);
    damm.expected_out = Some(1_234);
    damm.quoted_input_mint = Some(damm.token_a_mint);
    assert_eq!(
        crate::quote::meteora_damm_v2_out(&damm, 1_000, true).unwrap(),
        1_234
    );
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
    let client = RouterClient::new(payer, Pubkey::new_unique(), 0)
        .with_pool_guard(PoolGuardPolicy::disabled());

    let pf = RoutedMarket::new(Market::PumpFunInner(dummy_pumpfun()));
    assert!(client.sell_to_sol(1_000, &pf).is_ok());

    let ps = RoutedMarket::pumpswap(dummy_pumpswap());
    assert!(client.sell_to_wsol(1_000, &ps).is_ok());

    let cpmm = RoutedMarket::new(Market::CpmmOuter(dummy_cpmm()));
    assert!(client.sell_to_wsol(1_000, &cpmm).is_ok());

    let mut clmm = dummy_clmm();
    clmm.quoted_amount_in = Some(1_000);
    clmm.quoted_input_mint = Some(clmm.token_1_mint);
    clmm.expected_out = Some(900);
    let clmm_m = RoutedMarket::new(Market::RaydiumClmm(clmm));
    assert!(client.sell_to_wsol(1_000, &clmm_m).is_ok());
}

#[test]
fn offline_dynamic_quote_buy_builds_every_first_hop_for_both_targets() {
    let payer = Pubkey::new_unique();
    let fee_recipient = Pubkey::new_unique();
    let quote = Pubkey::new_unique();
    let target = Pubkey::new_unique();

    let mut cpmm_bridge = dummy_cpmm();
    cpmm_bridge.base_mint = quote;
    let mut amm_bridge = dummy_amm_v4();
    amm_bridge.pc_mint = quote;
    let mut clmm_bridge = dummy_clmm();
    clmm_bridge.token_1_mint = quote;
    let mut orca_bridge = dummy_whirlpool();
    orca_bridge.mint_b = quote;
    let mut dlmm_bridge = dummy_dlmm();
    dlmm_bridge.token_y_mint = quote;
    let mut damm_bridge = dummy_damm_v2();
    damm_bridge.token_b_mint = quote;
    let mut pump_bridge = dummy_pumpswap();
    pump_bridge.base_mint = quote;

    let mut launch = dummy_launchlab();
    launch.quote_mint = quote;
    launch.base_mint = target;
    let mut cpmm_target = dummy_cpmm();
    cpmm_target.quote_mint = quote;
    cpmm_target.base_mint = target;
    let targets = [
        Market::LaunchLabInner(launch),
        Market::CpmmOuter(cpmm_target),
    ];
    let bridges = [
        Market::CpmmOuter(cpmm_bridge),
        Market::RaydiumAmmV4(amm_bridge),
        Market::RaydiumClmm(clmm_bridge),
        Market::Whirlpool(orca_bridge),
        Market::MeteoraDlmm(dlmm_bridge),
        Market::MeteoraDammV2(damm_bridge),
        Market::PumpSwapOuter(pump_bridge),
    ];
    for bridge in &bridges {
        for target_pool in &targets {
            let built = crate::build_dynamic_quote_buy(
                &crate::PROGRAM_ID,
                &payer,
                &fee_recipient,
                500_000_000,
                500_000_000,
                1_000,
                10,
                bridge,
                target_pool,
            )
            .unwrap();
            assert_eq!(built.instruction.data[0], crate::TAG_ROUTE_DYNAMIC);
            assert_eq!(built.instruction.data[18], 2);
            assert_eq!(&built.instruction.data[19..51], target.as_ref());
            assert_eq!(built.instruction.accounts[6].pubkey, built.quote_ata);
            assert_eq!(built.instruction.accounts[4].pubkey, built.output_ata);
            assert_eq!(built.quote_mint, quote);
            assert_eq!(built.output_mint, target);
            assert_eq!(
                built.instruction.accounts.last().unwrap().pubkey,
                match target_pool {
                    Market::LaunchLabInner(_) => crate::LAUNCHLAB_PROGRAM,
                    Market::CpmmOuter(_) => crate::RAYDIUM_CPMM_PROGRAM,
                    _ => unreachable!(),
                }
            );
        }
    }
}

#[test]
fn offline_dynamic_quote_buy_rejects_disconnected_pools() {
    let payer = Pubkey::new_unique();
    let fee_recipient = Pubkey::new_unique();
    let bridge = Market::MeteoraDlmm(dummy_dlmm());
    let target = Market::LaunchLabInner(dummy_launchlab()); // quote is WSOL, not DLMM output
    assert!(crate::build_dynamic_quote_buy(
        &crate::PROGRAM_ID,
        &payer,
        &fee_recipient,
        1_000_000,
        1_000_000,
        100,
        10,
        &bridge,
        &target,
    )
    .is_err());

    let mut invalid_bridge = dummy_whirlpool();
    invalid_bridge.mint_a = Pubkey::new_unique();
    assert!(crate::build_dynamic_quote_buy(
        &crate::PROGRAM_ID,
        &payer,
        &fee_recipient,
        1_000_000,
        1_000_000,
        100,
        10,
        &Market::Whirlpool(invalid_bridge),
        &target,
    )
    .is_err());
}

#[test]
fn offline_native_sol_sell_binds_payer_and_sol_mint_sentinel() {
    let payer = Pubkey::new_unique();
    let client = RouterClient::new(payer, Pubkey::new_unique(), 0)
        .with_pool_guard(crate::pool_guard::PoolGuardPolicy::disabled());
    let market = RoutedMarket::new(Market::PumpFunInner(dummy_pumpfun()));
    let trade = client.sell_to_sol(1_000, &market).unwrap();
    let route = trade.route.unwrap();
    assert_eq!(route.accounts[4].pubkey, payer);
    assert_eq!(
        &route.data[19..51],
        crate::constants::SYSTEM_PROGRAM.as_ref()
    );
    assert_eq!(&route.data[51..83], PUMPFUN_PROGRAM.as_ref());
}

#[test]
fn offline_amm_v4_v2_quotes_use_actual_swap_fee_fraction() {
    let mut pool = dummy_amm_v4();
    pool.coin_reserve = 1_000_000;
    pool.pc_reserve = 2_000_000;
    pool.trade_fee_numerator = 999; // Historical orderbook fee must not price V2 swaps.
    pool.swap_fee_numerator = 3;
    pool.swap_fee_denominator = 1_000;
    let amount_in = 10_001;
    let net = amount_in - 31; // ceil(10001 * 3 / 1000)
    let expected = (2_000_000u128 * net as u128 / (1_000_000 + net) as u128) as u64;
    assert_eq!(
        raydium_amm_v4_out(&pool, amount_in, true).unwrap(),
        expected
    );
    let required = crate::quote::raydium_amm_v4_in_for_out(&pool, expected, true).unwrap();
    assert!(raydium_amm_v4_out(&pool, required, true).unwrap() >= expected);
    assert!(raydium_amm_v4_out(&pool, required - 1, true).unwrap() < expected);
    pool.swap_fee_denominator = 0;
    assert!(raydium_amm_v4_out(&pool, amount_in, true).is_err());
}

#[test]
fn offline_pumpswap_signed_reserves_match_current_quote_calculator() {
    use sol_trade_sdk::instruction::utils::pumpswap::PumpSwapFeeBasisPoints;
    use sol_trade_sdk::utils::calc::pumpswap::{
        buy_quote_input_internal_with_fees, sell_base_input_internal_with_fees,
    };
    let mut pool = dummy_pumpswap();
    pool.base_reserve = 1_000_000;
    pool.quote_reserve = 2_000_000;
    pool.lp_fee_bps = 20;
    pool.protocol_fee_bps = 5;
    pool.creator_fee_bps = 30;
    let fees = PumpSwapFeeBasisPoints::new(20, 5, 30);
    for offset in [-500_000, 0, 500_000] {
        pool.virtual_quote_reserves = offset;
        let buy =
            buy_quote_input_internal_with_fees(10_000, 0, 1_000_000, 2_000_000, offset, &fees)
                .unwrap();
        let sell =
            sell_base_input_internal_with_fees(10_000, 0, 1_000_000, 2_000_000, offset, &fees)
                .unwrap();
        assert_eq!(pumpswap_buy_base_out(&pool, 10_000).unwrap(), buy.base);
        assert_eq!(
            pumpswap_sell_quote_out(&pool, 10_000).unwrap(),
            sell.ui_quote
        );
    }
    pool.virtual_quote_reserves = -2_000_000;
    assert!(pumpswap_buy_base_out(&pool, 10_000).is_err());
    pool.virtual_quote_reserves = 0;
    pool.lp_fee_bps = 20_000;
    assert!(pumpswap_sell_quote_out(&pool, 10_000).is_err());
}

#[test]
fn offline_pumpfun_exact_in_matches_official_idl_split_fee_correction() {
    let mut pool = dummy_pumpfun();
    pool.virtual_token_reserves = 1_000_000;
    pool.virtual_sol_reserves = 1_000;
    pool.real_token_reserves = 800_000;
    pool.protocol_fee_bps = 95;
    pool.creator_fee_bps = 30;
    pool.fee_rates_known = true;
    // pump 3.2.0 buy_exact_sol_in docs: budget=101 -> net=99, split
    // fees=1+1, curve input=98; floor(98*1_000_000/(1000+98)).
    assert_eq!(pumpfun_buy_token_out(&pool, 101).unwrap(), 89_253);
    // budget=100 -> net=98, fees=1+1, curve input=97.
    assert_eq!(pumpfun_buy_token_out(&pool, 100).unwrap(), 88_422);
    // Tiny budget exposes the split-ceil correction skipped by the old code.
    assert!(pumpfun_buy_token_out(&pool, 2).is_err());
    // Sell gross=101; independently rounded fees are 1+1, not ceil(1.2625)=2
    // in general. gross=100 uses 1+1 while combined ceil also 2; choose gross=1.
    pool.virtual_sol_reserves = 1_001;
    pool.virtual_token_reserves = 1_000;
    assert!(pumpfun_sell_sol_out(&pool, 1).is_err()); // gross=1, two fees exceed gross
    pool.fee_rates_known = false;
    assert!(pumpfun_buy_token_out(&pool, 1_000).is_err());
    assert!(pumpfun_sell_sol_out(&pool, 100).is_err());
    let market = RoutedMarket::new(Market::PumpFunInner(pool));
    let client = RouterClient::new(Pubkey::new_unique(), Pubkey::new_unique(), 0)
        .with_pool_guard(PoolGuardPolicy::disabled());
    assert!(client
        .buy_with_opts(
            1_000,
            &market,
            TradeOpts::default().buy_with_sol().with_min_out(1)
        )
        .is_ok());
}

#[test]
fn offline_damm_exact_out_uses_official_amount_order_and_quotes_bind_direction() {
    let mut pool = dummy_damm_v2();
    let client = RouterClient::new(Pubkey::new_unique(), Pubkey::new_unique(), 0)
        .with_pool_guard(PoolGuardPolicy::disabled());
    let market = RoutedMarket::new(Market::MeteoraDammV2(pool.clone()));
    let built = client
        .buy_with_opts(
            1_000,
            &market,
            TradeOpts::default().buy_with_wsol().with_fixed_output(99),
        )
        .unwrap();
    let route = built.route.unwrap();
    let offset = route
        .data
        .windows(8)
        .position(|w| w == crate::constants::METEORA_DAMM_V2_SWAP2)
        .unwrap();
    assert_eq!(
        u64::from_le_bytes(route.data[offset + 8..offset + 16].try_into().unwrap()),
        99
    );
    assert_eq!(
        u64::from_le_bytes(route.data[offset + 16..offset + 24].try_into().unwrap()),
        1_000
    );
    assert_eq!(route.data[offset + 24], 2);
    pool.quoted_amount_in = Some(1_000);
    pool.quoted_input_mint = Some(pool.token_a_mint);
    pool.expected_out = Some(100);
    assert_eq!(meteora_damm_v2_out(&pool, 1_000, true).unwrap(), 100);
    assert!(meteora_damm_v2_out(&pool, 1_000, false).is_err());
    pool.swap_mode = 1;
    assert!(meteora_damm_v2_swap_leg(&client.payer, &pool, 1_000, 1, pool.token_a_mint).is_err());
}

#[test]
fn offline_unknown_fee_state_is_rejected_and_known_zero_is_valid() {
    let mut cpmm = dummy_cpmm();
    cpmm.fee_rates_known = false;
    assert!(cpmm_out(&cpmm, 10_000, true).is_err());
    cpmm.fee_rates_known = true;
    cpmm.trade_fee_rate = 0;
    assert!(cpmm_out(&cpmm, 10_000, true).unwrap() > 0);
    cpmm.trade_fee_rate = 1_000_000;
    assert!(cpmm_out(&cpmm, 10_000, false).is_err());
    let mut launch = dummy_launchlab();
    launch.fee_rates_known = false;
    assert!(crate::quote::launchlab_buy_base_out(&launch, 10_000).is_err());
    let mut pump = dummy_pumpfun();
    pump.protocol_fee_bps = 0;
    pump.creator_fee_bps = 0;
    assert!(pumpfun_buy_token_out(&pump, 10_000).unwrap() > 0);
}

#[test]
fn offline_cpmm_exact_input_flag_cannot_determine_canonical_direction() {
    let mut event = sol_parser_sdk::core::events::RaydiumCpmmSwapEvent::default();
    event.pool_id = Pubkey::new_unique();
    for exact_in in [true, false] {
        event.base_input = exact_in;
        assert!(crate::parser::cpmm_from_swap(&event).is_none());
    }
}

#[test]
fn offline_pumpswap_adapter_preserves_current_recipients_and_cashback_bucket() {
    let k = Pubkey::new_unique;
    let protocol = k();
    let buyback = k();
    let mut params = sol_trade_sdk::trading::core::params::PumpSwapParams::new(
        k(),
        k(),
        WSOL_MINT,
        k(),
        k(),
        1_000_000,
        2_000_000,
        0,
        k(),
        k(),
        TOKEN_PROGRAM,
        TOKEN_PROGRAM,
        protocol,
        k(),
        true,
        7,
    );
    params.protocol_fee_recipient_override = Some(protocol);
    params.protocol_extra_fee_recipient_override = Some(buyback);
    let pool = crate::adapter::pumpswap_from_params(&params);
    assert_eq!(
        pool.creator_fee_bps,
        params.fee_basis_points.coin_creator_fee_basis_points
    );
    for leg in [
        pumpswap_buy_leg(&k(), &pool, 10_000, 1).unwrap(),
        pumpswap_sell_leg(&k(), &pool, 10_000, 1).unwrap(),
    ] {
        assert_eq!(leg.accounts[9].pubkey, protocol);
        assert_eq!(leg.accounts[leg.accounts.len() - 2].pubkey, buyback);
    }
    let mut event = sol_parser_sdk::core::events::PumpSwapBuyEvent::default();
    event.coin_creator_fee_basis_points = 30;
    event.cashback_fee_basis_points = 7;
    assert_eq!(crate::parser::pumpswap_from_buy(&event).creator_fee_bps, 37);
}

#[test]
fn offline_whirlpool_supplemental_ticks_match_official_remaining_accounts_idl() {
    let mut pool = dummy_whirlpool();
    let user = Pubkey::new_unique();
    let ordinary = whirlpool_swap_leg(&user, &pool, 100, 1, pool.mint_a).unwrap();
    assert_eq!(ordinary.data.len(), 43);
    assert_eq!(ordinary.data[42], 0);
    for count in 1..=3 {
        let supplemental: Vec<_> = (0..count).map(|_| Pubkey::new_unique()).collect();
        pool.tick_arrays.truncate(3);
        pool.tick_arrays.extend_from_slice(&supplemental);
        let leg = whirlpool_swap_leg(&user, &pool, 100, 1, pool.mint_a).unwrap();
        // Official IDL variant order: SupplementalTickArrays=6; one u32-length
        // vector inside a Borsh Option, followed by enum ordinal and u8 count.
        assert_eq!(&leg.data[..42], &ordinary.data[..42]);
        assert_eq!(&leg.data[42..], &[1, 1, 0, 0, 0, 6, count as u8]);
        assert_eq!(leg.accounts.len(), 15 + count);
        for (account, key) in leg.accounts[15..].iter().zip(&supplemental) {
            assert_eq!(account.pubkey, *key);
            assert!(account.is_writable && !account.is_signer);
        }
    }
    pool.tick_arrays.push(Pubkey::new_unique());
    assert!(whirlpool_swap_leg(&user, &pool, 100, 1, pool.mint_a).is_err());
}

#[test]
fn offline_pumpswap_mayhem_fallback_does_not_use_regular_recipient() {
    use sol_trade_sdk::instruction::utils::pumpswap::accounts::MAYHEM_FEE_RECIPIENT;
    let k = Pubkey::new_unique;
    let params = sol_trade_sdk::trading::core::params::PumpSwapParams::new(
        k(),
        k(),
        WSOL_MINT,
        k(),
        k(),
        1_000_000,
        2_000_000,
        0,
        k(),
        k(),
        TOKEN_PROGRAM,
        TOKEN_PROGRAM,
        MAYHEM_FEE_RECIPIENT,
        k(),
        false,
        0,
    );
    assert!(params.is_mayhem_mode && params.protocol_fee_recipient_override.is_none());
    let pool = crate::adapter::pumpswap_from_params(&params);
    assert_eq!(pool.protocol_fee_recipient, MAYHEM_FEE_RECIPIENT);
    assert_eq!(
        pumpswap_buy_leg(&k(), &pool, 100, 1).unwrap().accounts[9].pubkey,
        MAYHEM_FEE_RECIPIENT
    );
}

#[test]
fn offline_cpmm_quotes_differential_current_rust_sdk_creator_modes_and_transfer_fees() {
    use sol_trade_sdk::trading::core::params::{
        RaydiumCpmmParams, TokenTransferFee as UpstreamFee,
    };
    use sol_trade_sdk::utils::calc::raydium_cpmm::compute_swap_amount_for_pool;
    let pool = dummy_cpmm();
    let mut params = RaydiumCpmmParams {
        pool_state: pool.pool_state,
        amm_config: pool.amm_config,
        observation_state: pool.observation_state,
        base_mint: pool.base_mint,
        quote_mint: pool.quote_mint,
        base_vault: pool.base_vault,
        quote_vault: pool.quote_vault,
        base_token_program: TOKEN_PROGRAM,
        quote_token_program: TOKEN_PROGRAM,
        base_reserve: 1_000_000_000,
        quote_reserve: 2_000_000_000,
        trade_fee_rate: 2_500,
        protocol_fee_rate: 120_000,
        fund_fee_rate: 80_000,
        creator_fee_rate: 10_000,
        creator_fee_on: 0,
        enable_creator_fee: true,
        base_transfer_fee: UpstreamFee::default(),
        quote_transfer_fee: UpstreamFee::default(),
    };
    for mode in 0..=2 {
        params.creator_fee_on = mode;
        for enabled in [false, true] {
            params.enable_creator_fee = enabled;
            for transfer_bps in [0, 25, 500] {
                params.base_transfer_fee = UpstreamFee {
                    basis_points: transfer_bps,
                    maximum_fee: 50,
                };
                params.quote_transfer_fee = UpstreamFee {
                    basis_points: transfer_bps,
                    maximum_fee: 123,
                };
                let router = crate::adapter::cpmm_from_params(&params);
                for base_in in [false, true] {
                    for amount in [101, 999, 10_000, 10_000_000] {
                        let official =
                            compute_swap_amount_for_pool(&params, base_in, amount, 0).unwrap();
                        let actual = cpmm_out(&router, amount, base_in).unwrap();
                        assert_eq!(actual, official.amount_out,
                            "mode={mode}, enabled={enabled}, transfer={transfer_bps}, base_in={base_in}, amount={amount}");
                        let minimal_in =
                            crate::quote::cpmm_in_for_out(&router, actual, base_in).unwrap();
                        assert!(minimal_in <= amount);
                        assert!(cpmm_out(&router, minimal_in, base_in).unwrap() >= actual);
                        if minimal_in > 1 {
                            assert!(cpmm_out(&router, minimal_in - 1, base_in).unwrap() < actual);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn offline_clmm_unknown_mint_owner_needs_overlay_even_without_transfer_fees() {
    let mut event = sol_parser_sdk::core::events::RaydiumClmmSwapEvent::default();
    event.pool_state = Pubkey::new_unique();
    event.input_mint = Pubkey::new_unique();
    event.output_mint = WSOL_MINT;
    event.input_vault = Pubkey::new_unique();
    event.output_vault = Pubkey::new_unique();
    event.tick_arrays = vec![Pubkey::new_unique(); 3];
    event.zero_for_one = true;
    let mut pool = crate::parser::clmm_from_swap(&event).unwrap();
    assert_eq!(pool.token_0_program, Pubkey::default());
    assert_eq!(pool.token_1_program, TOKEN_PROGRAM);
    let user = Pubkey::new_unique();
    assert!(raydium_clmm_swap_leg(&user, &pool, 100, 1, event.input_mint).is_err());
    crate::parser::clmm_apply_token_programs(
        &mut pool,
        crate::constants::TOKEN_2022_PROGRAM,
        TOKEN_PROGRAM,
    );
    let leg = raydium_clmm_swap_leg(&user, &pool, 100, 1, event.input_mint).unwrap();
    assert_eq!(
        leg.accounts[3].pubkey,
        crate::ata::ata(
            &user,
            &event.input_mint,
            &crate::constants::TOKEN_2022_PROGRAM
        )
    );
    crate::parser::clmm_apply_token_programs(&mut pool, Pubkey::default(), TOKEN_PROGRAM);
    assert!(raydium_clmm_swap_leg(&user, &pool, 100, 1, event.input_mint).is_err());
}

#[test]
fn offline_whirlpool_owner_overlay_preserves_independently_known_side() {
    let mut pool = dummy_whirlpool();
    pool.token_program_a = Pubkey::default();
    pool.token_program_b = crate::constants::TOKEN_2022_PROGRAM;
    let user = Pubkey::new_unique();
    assert!(whirlpool_swap_leg(&user, &pool, 100, 1, pool.mint_a).is_err());
    let mut event = sol_parser_sdk::core::events::OrcaWhirlpoolSwapEvent::default();
    event.token_program_a = TOKEN_PROGRAM;
    event.token_program_b = Pubkey::default();
    crate::parser::merge_whirlpool_swap(&mut pool, &event);
    assert_eq!(pool.token_program_a, TOKEN_PROGRAM);
    assert_eq!(pool.token_program_b, crate::constants::TOKEN_2022_PROGRAM);
    assert!(whirlpool_swap_leg(&user, &pool, 100, 1, pool.mint_a).is_ok());
}

#[test]
fn offline_amm_v4_v2_rejects_token_2022_in_both_amount_modes() {
    let mut pool = dummy_amm_v4();
    let user = Pubkey::new_unique();
    pool.token_program = crate::constants::TOKEN_2022_PROGRAM;
    assert!(raydium_amm_v4_swap_leg(&user, &pool, 100, 1, pool.coin_mint).is_err());
    assert!(raydium_amm_v4_swap_exact_out_leg(&user, &pool, 1, 100, pool.coin_mint).is_err());
    pool.token_program = TOKEN_PROGRAM;
    assert!(raydium_amm_v4_swap_leg(&user, &pool, 100, 1, pool.coin_mint).is_ok());
    assert!(raydium_amm_v4_swap_exact_out_leg(&user, &pool, 1, 100, pool.coin_mint).is_ok());
}

#[test]
fn offline_dynamic_route_accepts_whirlpool_supplemental_ticks_as_following_hop() {
    let user = Pubkey::new_unique();
    let mut pool = dummy_whirlpool();
    pool.tick_arrays
        .extend([Pubkey::new_unique(), Pubkey::new_unique()]);
    let input = crate::ata::ata(&user, &pool.mint_a, &pool.token_program_a);
    let output = crate::ata::ata(&user, &pool.mint_b, &pool.token_program_b);
    let first = Leg {
        program_id: TOKEN_PROGRAM,
        accounts: vec![AccountMeta::new(input, false)],
        data: vec![3],
    };
    let second = whirlpool_swap_leg(&user, &pool, 0, 1, pool.mint_a).unwrap();
    let accounts = || crate::route_ix::RouteAccounts {
        payer: user,
        fee_destination: Pubkey::new_unique(),
        fee_source: Pubkey::new_unique(),
        output_token_account: output,
        fee_program: TOKEN_PROGRAM,
        fee_mint: WSOL_MINT,
    };
    assert!(crate::route_ix::build_dynamic_route_instruction(
        &crate::PROGRAM_ID,
        accounts(),
        input,
        100,
        1,
        1,
        &pool.mint_b,
        &first,
        &second
    )
    .is_ok());
    let mut missing = second.clone();
    missing.accounts.pop();
    assert!(crate::route_ix::build_dynamic_route_instruction(
        &crate::PROGRAM_ID,
        accounts(),
        input,
        100,
        1,
        1,
        &pool.mint_b,
        &first,
        &missing
    )
    .is_err());
}
