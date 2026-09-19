//! Product-facing [`TradingClient`]: Route CPI build + trade-sdk SWQoS submit.

use anyhow::{anyhow, Result};
use solana_hash::Hash;
use solana_message::AddressLookupTableAccount;
use solana_sdk::{
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{Keypair, Signature},
    signer::Signer,
};
use sol_trade_sdk::common::{
    GasFeeStrategy, InfrastructureConfig, TradeConfig, TradeTransactionVersion,
};
use sol_trade_sdk::constants::USD1_TOKEN_ACCOUNT;
use sol_trade_sdk::swqos::common::TradeError;
use sol_trade_sdk::trading::core::{
    async_executor::execute_parallel_with_version,
    params::{DexParamEnum, SenderConcurrencyConfig},
};
use sol_trade_sdk::trading::factory::DexType;
use sol_trade_sdk::trading::MiddlewareManager;
use sol_trade_sdk::{
    DurableNonceInfo, SimpleBuyParams, SimpleSellParams, TradeBuyParams, TradeRiskGate,
    TradeSellParams, TradeTokenType, TradingInfrastructure,
};
use std::sync::Arc;

use crate::{
    adapter::to_routed_market_for_user,
    asset::{BuyWith, SellTo},
    ata::AtaPolicy,
    constants::{PROGRAM_ID, USDC_MINT, WSOL_MINT},
    market::RoutedMarket,
    pool_guard::PoolGuardPolicy,
    trade::{BuiltTrade, RouterClient, TradeOpts},
};

/// Router fee + wild-pool settings layered on trade-sdk [`TradeConfig`].
#[derive(Clone, Debug)]
pub struct RouterTradeConfig {
    pub trade: TradeConfig,
    pub fee_recipient: Pubkey,
    pub fee_bps: u16,
    pub program_id: Pubkey,
    pub pool_guard: PoolGuardPolicy,
}

impl RouterTradeConfig {
    pub fn new(trade: TradeConfig, fee_recipient: Pubkey, fee_bps: u16) -> Self {
        Self {
            trade,
            fee_recipient,
            fee_bps,
            program_id: PROGRAM_ID,
            pool_guard: PoolGuardPolicy::default(),
        }
    }

    pub fn with_program_id(mut self, program_id: Pubkey) -> Self {
        self.program_id = program_id;
        self
    }

    pub fn with_pool_guard(mut self, pool_guard: PoolGuardPolicy) -> Self {
        self.pool_guard = pool_guard;
        self
    }

    pub fn stonk_strict(mut self) -> Self {
        self.pool_guard = PoolGuardPolicy::stonk_strict();
        self
    }
}

/// High-level client: same API surface as `sol-trade-sdk::TradingClient`, but
/// swap legs are packed through the Pinocchio Route program.
pub struct TradingClient {
    pub payer: Arc<Keypair>,
    pub infrastructure: Arc<TradingInfrastructure>,
    pub middleware_manager: Option<Arc<MiddlewareManager>>,
    pub risk_gate: Option<Arc<dyn TradeRiskGate>>,
    pub router: RouterClient,
    pub use_seed_optimize: bool,
    pub use_dedicated_sender_threads: bool,
    pub sender_thread_cores: Option<Arc<Vec<usize>>>,
    pub max_sender_concurrency: usize,
    pub effective_core_ids: Arc<Vec<core_affinity::CoreId>>,
    pub log_enabled: bool,
    pub check_min_tip: bool,
    pub transaction_version: TradeTransactionVersion,
}

pub type SolanaTrade = TradingClient;

impl Clone for TradingClient {
    fn clone(&self) -> Self {
        Self {
            payer: self.payer.clone(),
            infrastructure: self.infrastructure.clone(),
            middleware_manager: self.middleware_manager.clone(),
            risk_gate: self.risk_gate.clone(),
            router: self.router.clone(),
            use_seed_optimize: self.use_seed_optimize,
            use_dedicated_sender_threads: self.use_dedicated_sender_threads,
            sender_thread_cores: self.sender_thread_cores.clone(),
            max_sender_concurrency: self.max_sender_concurrency,
            effective_core_ids: self.effective_core_ids.clone(),
            log_enabled: self.log_enabled,
            check_min_tip: self.check_min_tip,
            transaction_version: self.transaction_version,
        }
    }
}

impl TradingClient {
    pub fn from_infrastructure(
        payer: Arc<Keypair>,
        infrastructure: Arc<TradingInfrastructure>,
        fee_recipient: Pubkey,
        fee_bps: u16,
        use_seed_optimize: bool,
    ) -> Self {
        sol_trade_sdk::common::fast_fn::fast_init(&payer.pubkey());
        let router = RouterClient::new(payer.pubkey(), fee_recipient, fee_bps);
        Self {
            max_sender_concurrency: infrastructure.max_sender_concurrency,
            effective_core_ids: infrastructure.effective_core_ids.clone(),
            payer,
            infrastructure,
            middleware_manager: None,
            risk_gate: None,
            router,
            use_seed_optimize,
            use_dedicated_sender_threads: false,
            sender_thread_cores: None,
            log_enabled: true,
            check_min_tip: false,
            transaction_version: TradeTransactionVersion::V0,
        }
    }

    /// Same as [`Self::from_infrastructure`] plus optional background WSOL ATA creation
    /// (does not block bot startup — mirrors sol-trade-sdk).
    pub async fn from_infrastructure_with_wsol_setup(
        payer: Arc<Keypair>,
        infrastructure: Arc<TradingInfrastructure>,
        fee_recipient: Pubkey,
        fee_bps: u16,
        use_seed_optimize: bool,
        create_wsol_ata: bool,
    ) -> Self {
        // Delegate WSOL warm to trade-sdk (same infrastructure type).
        if create_wsol_ata {
            let _ = sol_trade_sdk::TradingClient::from_infrastructure_with_wsol_setup(
                payer.clone(),
                infrastructure.clone(),
                use_seed_optimize,
                true,
            )
            .await;
        }
        Self::from_infrastructure(payer, infrastructure, fee_recipient, fee_bps, use_seed_optimize)
    }

    pub async fn new(payer: Arc<Keypair>, config: RouterTradeConfig) -> Self {
        // Warm monotonic clock before subscribe (same as sol-trade-sdk).
        let _ = sol_trade_sdk::common::clock::now_micros();
        let infra_config = InfrastructureConfig::from_trade_config(&config.trade);
        let infra = Arc::new(TradingInfrastructure::new(infra_config).await);
        let mut client = Self::from_infrastructure_with_wsol_setup(
            payer,
            infra,
            config.fee_recipient,
            config.fee_bps,
            config.trade.use_seed_optimize,
            config.trade.create_wsol_ata_on_startup,
        )
        .await;
        client.router = client
            .router
            .with_program_id(config.program_id)
            .with_pool_guard(config.pool_guard);
        client.log_enabled = config.trade.log_enabled;
        client.check_min_tip = config.trade.check_min_tip;
        client.transaction_version = config.trade.transaction_version;
        client
    }

    pub fn with_middleware_manager(mut self, mw: Arc<MiddlewareManager>) -> Self {
        self.middleware_manager = Some(mw);
        self
    }

    pub fn with_risk_gate(mut self, gate: Arc<dyn TradeRiskGate>) -> Self {
        self.risk_gate = Some(gate);
        self
    }

    pub fn with_pool_guard(mut self, policy: PoolGuardPolicy) -> Self {
        self.router = self.router.with_pool_guard(policy);
        self
    }

    pub fn with_transaction_version(mut self, version: TradeTransactionVersion) -> Self {
        self.transaction_version = version;
        self
    }

    /// Enable dedicated SWQoS sender threads (and warm the pool immediately).
    ///
    /// - `None`: shared tokio pool
    /// - `Some(vec![])`: dedicated threads, no core pin
    /// - `Some(indices)`: pin to those cores (trimmed to `max_sender_concurrency`)
    pub fn with_dedicated_sender_threads(mut self, core_indices: Option<Vec<usize>>) -> Self {
        match core_indices {
            None => {
                self.use_dedicated_sender_threads = false;
                self.sender_thread_cores = None;
            }
            Some(v) if v.is_empty() => {
                self.use_dedicated_sender_threads = true;
                self.sender_thread_cores = None;
            }
            Some(v) => {
                self.use_dedicated_sender_threads = true;
                let cap = v.len().min(self.max_sender_concurrency);
                self.sender_thread_cores =
                    Some(Arc::new(if cap < v.len() { v[..cap].to_vec() } else { v }));
            }
        }
        if self.use_dedicated_sender_threads {
            sol_trade_sdk::trading::core::async_executor::warm_dedicated_sender_pool(
                self.sender_thread_cores.as_ref().map(|v| v.as_slice()),
                self.max_sender_concurrency,
            );
        }
        self
    }

    pub fn get_rpc(&self) -> &Arc<sol_trade_sdk::common::SolanaRpcClient> {
        &self.infrastructure.rpc
    }

    pub fn get_payer(&self) -> &Keypair {
        self.payer.as_ref()
    }

    pub fn get_payer_pubkey(&self) -> Pubkey {
        self.payer.pubkey()
    }

    /// Cold-path: reusable ATA create instructions (WSOL / stock), not meme.
    pub fn prepare_buy_atas(&self, market: &RoutedMarket, buy_with: BuyWith) -> Vec<Instruction> {
        self.router.prepare_buy_atas(market, buy_with)
    }

    pub fn create_wsol_ata(&self) -> Instruction {
        self.router.create_wsol_ata()
    }

    /// Build Route instructions without submitting (simulate / custom send).
    pub fn build_buy_instructions(&self, params: &TradeBuyParams) -> Result<Vec<Instruction>> {
        let market = to_routed_market_for_user(
            &params.extension_params,
            params.mint,
            &self.payer.pubkey(),
        )?;
        let opts = buy_opts_from_params(params)?;
        Ok(self
            .router
            .buy_with_opts(params.input_token_amount, &market, opts)?
            .into_instructions())
    }

    pub fn build_sell_instructions(&self, params: &TradeSellParams) -> Result<Vec<Instruction>> {
        let market = to_routed_market_for_user(
            &params.extension_params,
            params.mint,
            &self.payer.pubkey(),
        )?;
        let opts = sell_opts_from_params(params)?;
        Ok(self
            .router
            .sell_with_opts(params.input_token_amount, &market, opts)?
            .into_instructions())
    }

    pub async fn buy(
        &self,
        params: TradeBuyParams,
    ) -> Result<(bool, Vec<Signature>, Option<TradeError>, Vec<(sol_trade_sdk::swqos::SwqosType, i64)>)>
    {
        if params.recent_blockhash.is_none() && params.durable_nonce.is_none() {
            return Err(anyhow!(
                "Must provide either recent_blockhash or durable_nonce for buy"
            ));
        }
        if !validate_protocol_params(params.dex_type, &params.extension_params) {
            return Err(anyhow!(
                "Invalid protocol params for Trade (dex={:?})",
                params.dex_type
            ));
        }
        if let Some(gate) = self.risk_gate.as_deref() {
            gate.check_buy(&params)?;
        }
        let timing_start_us = if self.log_enabled {
            Some(
                params
                    .grpc_recv_us
                    .unwrap_or_else(sol_trade_sdk::common::clock::now_micros),
            )
        } else {
            None
        };
        let instructions = self.build_buy_instructions(&params)?;
        let build_end_us = (self.log_enabled && sol_trade_sdk::common::sdk_log::sdk_log_enabled())
            .then(sol_trade_sdk::common::clock::now_micros);
        self.submit(
            instructions,
            params.address_lookup_table_accounts,
            params.recent_blockhash,
            params.durable_nonce,
            true,
            true, // buy always tips SWQoS lanes (parity with trade-sdk)
            params.wait_tx_confirmed,
            params.wait_for_all_submits,
            params.gas_fee_strategy,
            params.simulate,
            timing_start_us,
            build_end_us,
        )
        .await
    }

    pub async fn buy_simple(
        &self,
        params: SimpleBuyParams,
    ) -> Result<(bool, Vec<Signature>, Option<TradeError>, Vec<(sol_trade_sdk::swqos::SwqosType, i64)>)>
    {
        self.buy(params.into()).await
    }

    pub async fn sell(
        &self,
        params: TradeSellParams,
    ) -> Result<(bool, Vec<Signature>, Option<TradeError>, Vec<(sol_trade_sdk::swqos::SwqosType, i64)>)>
    {
        if params.recent_blockhash.is_none() && params.durable_nonce.is_none() {
            return Err(anyhow!(
                "Must provide either recent_blockhash or durable_nonce for sell"
            ));
        }
        if !validate_protocol_params(params.dex_type, &params.extension_params) {
            return Err(anyhow!(
                "Invalid protocol params for Trade (dex={:?})",
                params.dex_type
            ));
        }
        let timing_start_us = if self.log_enabled {
            Some(
                params
                    .grpc_recv_us
                    .unwrap_or_else(sol_trade_sdk::common::clock::now_micros),
            )
        } else {
            None
        };
        let instructions = self.build_sell_instructions(&params)?;
        let build_end_us = (self.log_enabled && sol_trade_sdk::common::sdk_log::sdk_log_enabled())
            .then(sol_trade_sdk::common::clock::now_micros);
        self.submit(
            instructions,
            params.address_lookup_table_accounts,
            params.recent_blockhash,
            params.durable_nonce,
            false,
            params.with_tip,
            params.wait_tx_confirmed,
            params.wait_for_all_submits,
            params.gas_fee_strategy,
            params.simulate,
            timing_start_us,
            build_end_us,
        )
        .await
    }

    pub async fn sell_simple(
        &self,
        params: SimpleSellParams,
    ) -> Result<(bool, Vec<Signature>, Option<TradeError>, Vec<(sol_trade_sdk::swqos::SwqosType, i64)>)>
    {
        self.sell(params.into()).await
    }

    pub fn build_buy_from_market(
        &self,
        amount_in: u64,
        market: &RoutedMarket,
        opts: TradeOpts,
    ) -> Result<BuiltTrade> {
        self.router.buy_with_opts(amount_in, market, opts)
    }

    pub fn build_sell_from_market(
        &self,
        amount_in: u64,
        market: &RoutedMarket,
        opts: TradeOpts,
    ) -> Result<BuiltTrade> {
        self.router.sell_with_opts(amount_in, market, opts)
    }

    /// Query payer token balance with the same ATA derivation used on the trade path
    /// (including seed-optimized ATAs when enabled). Prefer cold-path / post-confirm use only.
    pub async fn get_payer_token_balance_with_program(
        &self,
        mint: &Pubkey,
        token_program: &Pubkey,
    ) -> Result<u64> {
        Ok(sol_trade_sdk::trading::common::utils::get_token_balance_with_options(
            &self.infrastructure.rpc,
            &self.payer.pubkey(),
            mint,
            token_program,
            self.use_seed_optimize,
        )
        .await?)
    }

    pub async fn get_payer_sol_balance(&self) -> Result<u64> {
        Ok(sol_trade_sdk::trading::common::utils::get_sol_balance(
            &self.infrastructure.rpc,
            &self.payer.pubkey(),
        )
        .await?)
    }

    pub async fn get_payer_token_balance(&self, mint: &Pubkey) -> Result<u64> {
        Ok(sol_trade_sdk::trading::common::utils::get_token_balance(
            &self.infrastructure.rpc,
            &self.payer.pubkey(),
            mint,
        )
        .await?)
    }

    #[allow(clippy::too_many_arguments)]
    async fn submit(
        &self,
        instructions: Vec<Instruction>,
        address_lookup_table_accounts: Vec<AddressLookupTableAccount>,
        recent_blockhash: Option<Hash>,
        durable_nonce: Option<DurableNonceInfo>,
        is_buy: bool,
        with_tip: bool,
        wait_tx_confirmed: bool,
        wait_for_all_submits: bool,
        gas_fee_strategy: GasFeeStrategy,
        simulate: bool,
        timing_start_us: Option<i64>,
        build_end_us: Option<i64>,
    ) -> Result<(bool, Vec<Signature>, Option<TradeError>, Vec<(sol_trade_sdk::swqos::SwqosType, i64)>)>
    {
        if simulate {
            if self.log_enabled && sol_trade_sdk::common::sdk_log::sdk_log_enabled() {
                let before_submit_us = sol_trade_sdk::common::clock::now_micros();
                sol_trade_sdk::common::sdk_log::print_sdk_timing_block(
                    if is_buy { "Buy" } else { "Sell" },
                    timing_start_us,
                    build_end_us,
                    Some(before_submit_us),
                    &[],
                    None,
                );
            }
            return Ok((true, Vec::new(), None, Vec::new()));
        }
        let before_submit_us = (self.log_enabled && sol_trade_sdk::common::sdk_log::sdk_log_enabled())
            .then(sol_trade_sdk::common::clock::now_micros);
        let sender_config = SenderConcurrencyConfig {
            sender_thread_cores: self.sender_thread_cores.clone(),
            effective_core_ids: self.effective_core_ids.clone(),
            max_sender_concurrency: self.max_sender_concurrency,
        };
        let (ok, sigs, err, timings) = execute_parallel_with_version(
            self.infrastructure.swqos_clients.as_slice(),
            self.payer.clone(),
            instructions,
            address_lookup_table_accounts,
            recent_blockhash,
            durable_nonce,
            self.middleware_manager.clone(),
            "Router",
            is_buy,
            wait_tx_confirmed,
            wait_for_all_submits,
            with_tip,
            gas_fee_strategy,
            self.use_dedicated_sender_threads,
            sender_config,
            self.check_min_tip,
            self.transaction_version,
        )
        .await?;
        if self.log_enabled && sol_trade_sdk::common::sdk_log::sdk_log_enabled() {
            let confirm_us = wait_tx_confirmed.then(sol_trade_sdk::common::clock::now_micros);
            sol_trade_sdk::common::sdk_log::print_sdk_timing_block(
                if is_buy { "Buy" } else { "Sell" },
                timing_start_us,
                build_end_us,
                before_submit_us,
                &timings,
                confirm_us,
            );
        }
        let legacy = timings
            .into_iter()
            .map(|t| (t.swqos_type, t.submit_done_us))
            .collect();
        Ok((ok, sigs, err.map(TradeError::from), legacy))
    }
}

fn trade_token_to_buy_with(t: TradeTokenType) -> BuyWith {
    match t {
        TradeTokenType::SOL => BuyWith::Sol,
        TradeTokenType::WSOL => BuyWith::Wsol,
        TradeTokenType::USDC => BuyWith::Token(USDC_MINT),
        TradeTokenType::USD1 => BuyWith::Token(USD1_TOKEN_ACCOUNT),
        TradeTokenType::Token(mint) => {
            if mint == WSOL_MINT {
                BuyWith::Wsol
            } else {
                BuyWith::Token(mint)
            }
        }
    }
}

fn trade_token_to_sell_to(t: TradeTokenType) -> SellTo {
    match t {
        TradeTokenType::SOL => SellTo::Sol,
        TradeTokenType::WSOL => SellTo::Wsol,
        TradeTokenType::USDC => SellTo::Token(USDC_MINT),
        TradeTokenType::USD1 => SellTo::Token(USD1_TOKEN_ACCOUNT),
        TradeTokenType::Token(mint) => {
            if mint == WSOL_MINT {
                SellTo::Wsol
            } else {
                SellTo::Token(mint)
            }
        }
    }
}

fn buy_ata_policy(params: &TradeBuyParams) -> AtaPolicy {
    AtaPolicy {
        create_meme: params.create_mint_ata,
        create_wsol: params.create_input_token_ata
            && matches!(
                params.input_token_type,
                TradeTokenType::SOL | TradeTokenType::WSOL
            ),
        create_quote: params.create_input_token_ata
            && !matches!(
                params.input_token_type,
                TradeTokenType::SOL | TradeTokenType::WSOL
            ),
        close_wsol: params.close_input_token_ata
            && matches!(params.input_token_type, TradeTokenType::WSOL),
        close_meme: false,
        close_quote: false,
    }
}

fn sell_ata_policy(params: &TradeSellParams) -> AtaPolicy {
    AtaPolicy {
        create_meme: false,
        create_wsol: params.create_output_token_ata
            && matches!(
                params.output_token_type,
                TradeTokenType::SOL | TradeTokenType::WSOL
            ),
        create_quote: params.create_output_token_ata
            && !matches!(
                params.output_token_type,
                TradeTokenType::SOL | TradeTokenType::WSOL
            ),
        close_wsol: params.close_output_token_ata
            && matches!(params.output_token_type, TradeTokenType::WSOL),
        close_meme: params.close_mint_token_ata,
        close_quote: false,
    }
}

fn buy_opts_from_params(params: &TradeBuyParams) -> Result<TradeOpts> {
    let mut opts = TradeOpts::default()
        .with_slippage_bps(params.slippage_basis_points.unwrap_or(100))
        .with_ata(buy_ata_policy(params));
    opts.buy_with = trade_token_to_buy_with(params.input_token_type);
    if let Some(fixed) = params.fixed_output_token_amount {
        opts = opts.with_fixed_output(fixed);
    }
    Ok(opts)
}

fn sell_opts_from_params(params: &TradeSellParams) -> Result<TradeOpts> {
    let mut opts = TradeOpts::default()
        .with_slippage_bps(params.slippage_basis_points.unwrap_or(100))
        .with_ata(sell_ata_policy(params));
    opts.sell_to = trade_token_to_sell_to(params.output_token_type);
    if let Some(fixed) = params.fixed_output_token_amount {
        opts = opts.with_fixed_output(fixed);
    }
    Ok(opts)
}

/// Validate that `dex_type` matches `extension_params` (mirrors trade-sdk).
pub fn validate_protocol_params(dex_type: DexType, params: &DexParamEnum) -> bool {
    match dex_type {
        DexType::PumpFun => matches!(params, DexParamEnum::PumpFun(_)),
        DexType::PumpSwap => matches!(params, DexParamEnum::PumpSwap(_)),
        DexType::LaunchLab => matches!(params, DexParamEnum::LaunchLab(_)),
        DexType::Bonk => matches!(params, DexParamEnum::Bonk(_)),
        DexType::StonkFun => matches!(
            params,
            DexParamEnum::StonkFun(_)
                | DexParamEnum::StonkFunSwap(_)
                | DexParamEnum::StonkFunViaSol(_)
        ),
        DexType::RaydiumCpmm => matches!(params, DexParamEnum::RaydiumCpmm(_)),
        DexType::RaydiumAmmV4 => matches!(params, DexParamEnum::RaydiumAmmV4(_)),
        DexType::MeteoraDammV2 => matches!(params, DexParamEnum::MeteoraDammV2(_)),
        DexType::RaydiumClmm => matches!(params, DexParamEnum::RaydiumClmm(_)),
        DexType::OrcaWhirlpool => matches!(params, DexParamEnum::OrcaWhirlpool(_)),
        DexType::MeteoraDlmm => matches!(params, DexParamEnum::MeteoraDlmm(_)),
    }
}
