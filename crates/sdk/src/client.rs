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

    pub async fn new(payer: Arc<Keypair>, config: RouterTradeConfig) -> Self {
        let infra_config = InfrastructureConfig::from_trade_config(&config.trade);
        let infra = TradingInfrastructure::new(infra_config).await;
        let mut client = Self::from_infrastructure(
            payer,
            Arc::new(infra),
            config.fee_recipient,
            config.fee_bps,
            config.trade.use_seed_optimize,
        );
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

    pub fn with_dedicated_sender_threads(mut self, cores: Option<Vec<usize>>) -> Self {
        self.use_dedicated_sender_threads = true;
        self.sender_thread_cores = cores.map(Arc::new);
        self
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
        if let Some(gate) = self.risk_gate.as_deref() {
            gate.check_buy(&params)?;
        }
        let instructions = self.build_buy_instructions(&params)?;
        self.submit(
            instructions,
            params.address_lookup_table_accounts,
            params.recent_blockhash,
            params.durable_nonce,
            true,
            params.wait_tx_confirmed,
            params.wait_for_all_submits,
            params.gas_fee_strategy,
            params.simulate,
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
        let instructions = self.build_sell_instructions(&params)?;
        self.submit(
            instructions,
            params.address_lookup_table_accounts,
            params.recent_blockhash,
            params.durable_nonce,
            false,
            params.wait_tx_confirmed,
            params.wait_for_all_submits,
            params.gas_fee_strategy,
            params.simulate,
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

    #[allow(clippy::too_many_arguments)]
    async fn submit(
        &self,
        instructions: Vec<Instruction>,
        address_lookup_table_accounts: Vec<AddressLookupTableAccount>,
        recent_blockhash: Option<Hash>,
        durable_nonce: Option<DurableNonceInfo>,
        is_buy: bool,
        wait_tx_confirmed: bool,
        wait_for_all_submits: bool,
        gas_fee_strategy: GasFeeStrategy,
        simulate: bool,
    ) -> Result<(bool, Vec<Signature>, Option<TradeError>, Vec<(sol_trade_sdk::swqos::SwqosType, i64)>)>
    {
        if simulate {
            return Ok((true, Vec::new(), None, Vec::new()));
        }
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
            true,
            gas_fee_strategy,
            self.use_dedicated_sender_threads,
            sender_config,
            self.check_min_tip,
            self.transaction_version,
        )
        .await?;
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

pub use sol_trade_sdk::{
    BuyAmount, SellAmount, StonkFunMemeLeg, StonkFunParams, StonkFunSolHop, StonkFunSwapParams,
    StonkFunViaSolParams, AccountPolicy,
};

pub use crate::adapter::{load_routed_market_by_rpc, to_routed_market, LoadMarketRequest};

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
