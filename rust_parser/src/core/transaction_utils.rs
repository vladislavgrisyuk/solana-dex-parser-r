use crate::core::constants::dex_program_names;
use crate::core::instruction_classifier::InstructionClassifier;
use crate::core::transaction_adapter::TransactionAdapter;
use crate::types::{DexInfo, FeeInfo, PoolEvent, TradeInfo, TradeType, TransferData, TransferMap};

pub struct TransactionUtils {
    pub(crate) adapter: TransactionAdapter,
}

impl TransactionUtils {
    pub fn new(adapter: TransactionAdapter) -> Self {
        let start = std::time::Instant::now();
        let result = Self { adapter };
        let duration = start.elapsed();
        tracing::debug!(
            "⏱️  TransactionUtils::new={:.3}μs",
            duration.as_secs_f64() * 1_000_000.0
        );
        result
    }

    pub fn get_dex_info(&self, classifier: &InstructionClassifier) -> DexInfo {
        let start = std::time::Instant::now();
        
        let t0 = std::time::Instant::now();
        let all_program_ids = classifier.get_all_program_ids();
        let t1 = std::time::Instant::now();
        tracing::debug!(
            "⏱️  get_dex_info: classifier.get_all_program_ids={:.3}μs, count={}",
            (t1 - t0).as_secs_f64() * 1_000_000.0,
            all_program_ids.len()
        );
        
        let t2 = std::time::Instant::now();
        let program_id = all_program_ids.into_iter().next();
        let amm = program_id
            .as_ref()
            .map(|id| dex_program_names::name(id).to_string());
        let t3 = std::time::Instant::now();
        tracing::debug!(
            "⏱️  get_dex_info: prepare_info={:.3}μs",
            (t3 - t2).as_secs_f64() * 1_000_000.0
        );
        
        let result = DexInfo {
            program_id: program_id.clone(),
            amm: amm.clone(),
            route: None,
        };
        
        let duration = start.elapsed();
        tracing::debug!(
            "⏱️  get_dex_info: total={:.3}μs, program_id={:?}, amm={:?}",
            duration.as_secs_f64() * 1_000_000.0,
            program_id,
            amm
        );
        
        result
    }

    pub fn get_transfer_actions(&self) -> TransferMap {
        let start = std::time::Instant::now();
        let result = self.adapter.get_transfer_actions();
        let duration = start.elapsed();
        let transfer_count: usize = result.values().map(|v| v.len()).sum();
        tracing::debug!(
            "⏱️  get_transfer_actions: total={:.3}μs, programs={}, total_transfers={}",
            duration.as_secs_f64() * 1_000_000.0,
            result.len(),
            transfer_count
        );
        result
    }

    pub fn process_swap_data(
        &self,
        transfers: &[TransferData],
        dex_info: &DexInfo,
    ) -> Option<TradeInfo> {
        let start = std::time::Instant::now();
        tracing::debug!(
            "⏱️  process_swap_data START: transfers={}",
            transfers.len()
        );
        
        if transfers.len() < 2 {
            tracing::debug!("⏱️  process_swap_data: insufficient transfers, returning None");
            return None;
        }

        let t0 = std::time::Instant::now();
        let input = transfers.first()?;
        let output = transfers.get(1)?;
        let t1 = std::time::Instant::now();
        tracing::debug!(
            "⏱️  process_swap_data: get_input_output={:.3}μs",
            (t1 - t0).as_secs_f64() * 1_000_000.0
        );
        
        let t2 = std::time::Instant::now();
        let program_id = dex_info
            .program_id
            .clone()
            .unwrap_or_else(|| input.program_id.clone());
        let amm = dex_info
            .amm
            .clone()
            .unwrap_or_else(|| dex_program_names::name(&program_id).to_string());
        let t3 = std::time::Instant::now();
        tracing::debug!(
            "⏱️  process_swap_data: prepare_program_info={:.3}μs",
            (t3 - t2).as_secs_f64() * 1_000_000.0
        );

        let t4 = std::time::Instant::now();
        let input_token = Self::transfer_to_token_info(input);
        let t5 = std::time::Instant::now();
        let output_token = Self::transfer_to_token_info(output);
        let t6 = std::time::Instant::now();
        tracing::debug!(
            "⏱️  process_swap_data: transfer_to_token_info input={:.3}μs, output={:.3}μs",
            (t5 - t4).as_secs_f64() * 1_000_000.0,
            (t6 - t5).as_secs_f64() * 1_000_000.0
        );

        let t7 = std::time::Instant::now();
        let result = Some(TradeInfo {
            trade_type: TradeType::Swap,
            pool: Vec::new(),
            input_token,
            output_token,
            slippage_bps: None,
            fee: None,
            fees: Vec::new(),
            user: Some(input.info.source.clone()),
            program_id: Some(program_id.clone()),
            amm: Some(amm.clone()),
            amms: None,
            route: dex_info.route.clone(),
            slot: self.adapter.slot(),
            timestamp: self.adapter.block_time(),
            signature: self.adapter.signature().to_string(),
            idx: input.idx.clone(),
            signer: Some(self.adapter.signers().to_vec()),
        });
        let t8 = std::time::Instant::now();
        tracing::debug!(
            "⏱️  process_swap_data: build_trade_info={:.3}μs",
            (t8 - t7).as_secs_f64() * 1_000_000.0
        );
        
        let duration = start.elapsed();
        tracing::debug!(
            "⏱️  process_swap_data: total={:.3}μs, program_id={:?}, amm={:?}",
            duration.as_secs_f64() * 1_000_000.0,
            program_id,
            amm
        );
        
        result
    }

    pub fn attach_trade_fee(&self, mut trade: TradeInfo) -> TradeInfo {
        let start = std::time::Instant::now();
        
        let t0 = std::time::Instant::now();
        let fee_amount = self.adapter.fee();
        let t1 = std::time::Instant::now();
        tracing::debug!(
            "⏱️  attach_trade_fee: adapter.fee()={:.3}μs, amount={}",
            (t1 - t0).as_secs_f64() * 1_000_000.0,
            fee_amount.amount
        );
        
        if fee_amount.amount != "0" {
            let t2 = std::time::Instant::now();
            let fee = FeeInfo {
                mint: "SOL".to_string(),
                amount: fee_amount.ui_amount.unwrap_or(0.0),
                amount_raw: fee_amount.amount.clone(),
                decimals: fee_amount.decimals,
                dex: None,
                fee_type: None,
                recipient: None,
            };
            trade.fee = Some(fee);
            let t3 = std::time::Instant::now();
            tracing::debug!(
                "⏱️  attach_trade_fee: create_fee_info={:.3}μs",
                (t3 - t2).as_secs_f64() * 1_000_000.0
            );
        }
        
        let duration = start.elapsed();
        tracing::debug!(
            "⏱️  attach_trade_fee: total={:.3}μs, has_fee={}",
            duration.as_secs_f64() * 1_000_000.0,
            trade.fee.is_some()
        );
        
        trade
    }

    pub fn attach_token_transfer_info(
        &self,
        trade: TradeInfo,
        _transfer_actions: &TransferMap,
    ) -> TradeInfo {
        let start = std::time::Instant::now();
        // Currently a no-op, but we log it anyway
        let duration = start.elapsed();
        tracing::debug!(
            "⏱️  attach_token_transfer_info: total={:.3}μs",
            duration.as_secs_f64() * 1_000_000.0
        );
        trade
    }

    pub fn attach_user_balance_to_lps(&self, pools: Vec<PoolEvent>) -> Vec<PoolEvent> {
        let signer = self.adapter.signer();
        if !signer.is_empty() {
            pools
                .into_iter()
                .map(|mut pool| {
                    pool.idx = format!("{}-{}", signer, pool.idx);
                    pool
                })
                .collect()
        } else {
            pools
        }
    }
}

impl TransactionUtils {
    fn transfer_to_token_info(transfer: &TransferData) -> crate::types::TokenInfo {
        let amount = transfer.info.token_amount.ui_amount.unwrap_or_else(|| {
            transfer
                .info
                .token_amount
                .amount
                .parse::<f64>()
                .unwrap_or(0.0)
        });

        crate::types::TokenInfo {
            mint: transfer.info.mint.clone(),
            amount,
            amount_raw: transfer.info.token_amount.amount.clone(),
            decimals: transfer.info.token_amount.decimals,
            authority: transfer.info.authority.clone(),
            destination: Some(transfer.info.destination.clone()),
            destination_owner: transfer.info.destination_owner.clone(),
            destination_balance: transfer.info.destination_balance.clone(),
            destination_pre_balance: transfer.info.destination_pre_balance.clone(),
            source: Some(transfer.info.source.clone()),
            source_balance: transfer.info.source_balance.clone(),
            source_pre_balance: transfer.info.source_pre_balance.clone(),
            destination_balance_change: None,
            source_balance_change: None,
            balance_change: transfer.info.sol_balance_change.clone(),
        }
    }
}
