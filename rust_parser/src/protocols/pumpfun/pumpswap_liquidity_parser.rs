use crate::core::transaction_adapter::TransactionAdapter;
use crate::protocols::simple::LiquidityParser;
use crate::types::{ClassifiedInstruction, PoolEvent, TradeType, TransferMap};

use super::constants::{PUMP_SWAP_PROGRAM_ID, PUMP_SWAP_PROGRAM_NAME};
use super::pumpswap_event_parser::{
    PumpswapCreatePoolEvent, PumpswapDepositEvent, PumpswapEvent, PumpswapEventData,
    PumpswapEventParser, PumpswapWithdrawEvent,
};
use super::util::convert_to_ui_amount;

pub struct PumpswapLiquidityParser {
    adapter: TransactionAdapter,
    _transfer_actions: TransferMap,
    classified_instructions: Vec<ClassifiedInstruction>,
    event_parser: PumpswapEventParser,
}

impl PumpswapLiquidityParser {
    pub fn new(
        adapter: TransactionAdapter,
        transfer_actions: TransferMap,
        classified_instructions: Vec<ClassifiedInstruction>,
    ) -> Self {
        let event_parser = PumpswapEventParser::new(adapter.clone());
        Self {
            adapter,
            _transfer_actions: transfer_actions,
            classified_instructions,
            event_parser,
        }
    }

    fn parse_events(&self) -> Vec<PumpswapEvent> {
        let start = std::time::Instant::now();
        let result = match self
            .event_parser
            .parse_instructions(&self.classified_instructions)
        {
            Ok(events) => {
                let duration = start.elapsed();
                tracing::debug!(
                    "⏱️  PumpswapLiquidityParser::parse_events: total={:.3}ms, events_count={}, instructions_count={}",
                    duration.as_secs_f64() * 1000.0,
                    events.len(),
                    self.classified_instructions.len()
                );
                events
            },
            Err(err) => {
                let duration = start.elapsed();
                tracing::error!(
                    "⏱️  PumpswapLiquidityParser::parse_events: ERROR={:.3}ms, error={}, instructions_count={}",
                    duration.as_secs_f64() * 1000.0,
                    err,
                    self.classified_instructions.len()
                );
                Vec::new()
            }
        };
        result
    }

    fn parse_create_event(
        &self,
        event: &PumpswapEvent,
        data: &PumpswapCreatePoolEvent,
    ) -> PoolEvent {
        PoolEvent {
            user: self.adapter.signer(),
            event_type: TradeType::Create,
            program_id: Some(PUMP_SWAP_PROGRAM_ID.to_string()),
            amm: Some(PUMP_SWAP_PROGRAM_NAME.to_string()),
            slot: event.slot,
            timestamp: event.timestamp,
            signature: event.signature.clone(),
            idx: event.idx.clone(),
            signer: event.signer.clone(),
            pool_id: data.pool.clone(),
            config: None,
            pool_lp_mint: Some(data.lp_mint.clone()),
            token0_mint: Some(data.base_mint.clone()),
            token0_amount: Some(convert_to_ui_amount(
                data.base_amount_in as u128,
                data.base_mint_decimals,
            )),
            token0_amount_raw: Some(data.base_amount_in.to_string()),
            token0_balance_change: None,
            token0_decimals: Some(data.base_mint_decimals),
            token1_mint: Some(data.quote_mint.clone()),
            token1_amount: Some(convert_to_ui_amount(
                data.quote_amount_in as u128,
                data.quote_mint_decimals,
            )),
            token1_amount_raw: Some(data.quote_amount_in.to_string()),
            token1_balance_change: None,
            token1_decimals: Some(data.quote_mint_decimals),
            lp_amount: Some(convert_to_ui_amount(
                data.lp_token_amount_out as u128,
                data.base_mint_decimals,
            )),
            lp_amount_raw: Some(data.lp_token_amount_out.to_string()),
        }
    }

    fn parse_deposit_event(
        &self,
        event: &PumpswapEvent,
        data: &PumpswapDepositEvent,
    ) -> Option<PoolEvent> {
        let token0_info = self
            .adapter
            .token_account_info(&data.user_base_token_account)?;
        let token1_info = self
            .adapter
            .token_account_info(&data.user_quote_token_account)?;
        let lp_info = self
            .adapter
            .token_account_info(&data.user_pool_token_account)?;

        let token0_decimals = self
            .adapter
            .token_decimals(&token0_info.mint)
            .unwrap_or(token0_info.decimals);
        let token1_decimals = self
            .adapter
            .token_decimals(&token1_info.mint)
            .unwrap_or(token1_info.decimals);
        let lp_decimals = self
            .adapter
            .token_decimals(&lp_info.mint)
            .unwrap_or(lp_info.decimals);

        Some(PoolEvent {
            user: self.adapter.signer(),
            event_type: TradeType::Add,
            program_id: Some(PUMP_SWAP_PROGRAM_ID.to_string()),
            amm: Some(PUMP_SWAP_PROGRAM_NAME.to_string()),
            slot: event.slot,
            timestamp: event.timestamp,
            signature: event.signature.clone(),
            idx: event.idx.clone(),
            signer: event.signer.clone(),
            pool_id: data.pool.clone(),
            config: None,
            pool_lp_mint: Some(lp_info.mint.clone()),
            token0_mint: Some(token0_info.mint.clone()),
            token0_amount: Some(convert_to_ui_amount(
                data.base_amount_in as u128,
                token0_decimals,
            )),
            token0_amount_raw: Some(data.base_amount_in.to_string()),
            token0_balance_change: None,
            token0_decimals: Some(token0_decimals),
            token1_mint: Some(token1_info.mint.clone()),
            token1_amount: Some(convert_to_ui_amount(
                data.quote_amount_in as u128,
                token1_decimals,
            )),
            token1_amount_raw: Some(data.quote_amount_in.to_string()),
            token1_balance_change: None,
            token1_decimals: Some(token1_decimals),
            lp_amount: Some(convert_to_ui_amount(
                data.lp_token_amount_out as u128,
                lp_decimals,
            )),
            lp_amount_raw: Some(data.lp_token_amount_out.to_string()),
        })
    }

    fn parse_withdraw_event(
        &self,
        event: &PumpswapEvent,
        data: &PumpswapWithdrawEvent,
    ) -> Option<PoolEvent> {
        let token0_info = self
            .adapter
            .token_account_info(&data.user_base_token_account)?;
        let token1_info = self
            .adapter
            .token_account_info(&data.user_quote_token_account)?;
        let lp_info = self
            .adapter
            .token_account_info(&data.user_pool_token_account)?;

        let token0_decimals = self
            .adapter
            .token_decimals(&token0_info.mint)
            .unwrap_or(token0_info.decimals);
        let token1_decimals = self
            .adapter
            .token_decimals(&token1_info.mint)
            .unwrap_or(token1_info.decimals);
        let lp_decimals = self
            .adapter
            .token_decimals(&lp_info.mint)
            .unwrap_or(lp_info.decimals);

        Some(PoolEvent {
            user: self.adapter.signer(),
            event_type: TradeType::Remove,
            program_id: Some(PUMP_SWAP_PROGRAM_ID.to_string()),
            amm: Some(PUMP_SWAP_PROGRAM_NAME.to_string()),
            slot: event.slot,
            timestamp: event.timestamp,
            signature: event.signature.clone(),
            idx: event.idx.clone(),
            signer: event.signer.clone(),
            pool_id: data.pool.clone(),
            config: None,
            pool_lp_mint: Some(lp_info.mint.clone()),
            token0_mint: Some(token0_info.mint.clone()),
            token0_amount: Some(convert_to_ui_amount(
                data.base_amount_out as u128,
                token0_decimals,
            )),
            token0_amount_raw: Some(data.base_amount_out.to_string()),
            token0_balance_change: None,
            token0_decimals: Some(token0_decimals),
            token1_mint: Some(token1_info.mint.clone()),
            token1_amount: Some(convert_to_ui_amount(
                data.quote_amount_out as u128,
                token1_decimals,
            )),
            token1_amount_raw: Some(data.quote_amount_out.to_string()),
            token1_balance_change: None,
            token1_decimals: Some(token1_decimals),
            lp_amount: Some(convert_to_ui_amount(
                data.lp_token_amount_in as u128,
                lp_decimals,
            )),
            lp_amount_raw: Some(data.lp_token_amount_in.to_string()),
        })
    }
}

impl LiquidityParser for PumpswapLiquidityParser {
    fn process_liquidity(&mut self) -> Vec<PoolEvent> {
        let method_start = std::time::Instant::now();
        tracing::info!("💧 PumpswapLiquidityParser::process_liquidity START");
        
        let t0 = std::time::Instant::now();
        let parsed_events = self.parse_events();
        let t1 = std::time::Instant::now();
        let events_count = parsed_events.len();
        let parse_events_time = (t1 - t0).as_secs_f64() * 1000.0;
        tracing::info!("⏱️  [1/2] parse_events={:.3}ms, events_count={}", parse_events_time, events_count);
        
        let t2 = std::time::Instant::now();
        let mut events = Vec::with_capacity(events_count);
        let mut create_count = 0;
        let mut deposit_count = 0;
        let mut withdraw_count = 0;
        let mut skipped_count = 0;
        let mut create_time = 0.0;
        let mut deposit_time = 0.0;
        let mut withdraw_time = 0.0;
        
        for (idx, event) in parsed_events.into_iter().enumerate() {
            let event_start = std::time::Instant::now();
            
            match &event.data {
                PumpswapEventData::Create(data) => {
                    let t_create = std::time::Instant::now();
                    let pool_event = self.parse_create_event(&event, data);
                    let create_duration = (std::time::Instant::now() - t_create).as_secs_f64() * 1000.0;
                    create_time += create_duration;
                    events.push(pool_event);
                    create_count += 1;
                    tracing::info!("⏱️  [{}/{}] parse_create_event={:.3}ms", idx + 1, events_count, create_duration);
                }
                PumpswapEventData::Deposit(data) => {
                    let t_deposit = std::time::Instant::now();
                    if let Some(pool) = self.parse_deposit_event(&event, data) {
                        let deposit_duration = (std::time::Instant::now() - t_deposit).as_secs_f64() * 1000.0;
                        deposit_time += deposit_duration;
                        events.push(pool);
                        deposit_count += 1;
                        tracing::info!("⏱️  [{}/{}] parse_deposit_event={:.3}ms", idx + 1, events_count, deposit_duration);
                    } else {
                        skipped_count += 1;
                        tracing::debug!("⏱️  [{}/{}] parse_deposit_event returned None", idx + 1, events_count);
                    }
                }
                PumpswapEventData::Withdraw(data) => {
                    let t_withdraw = std::time::Instant::now();
                    if let Some(pool) = self.parse_withdraw_event(&event, data) {
                        let withdraw_duration = (std::time::Instant::now() - t_withdraw).as_secs_f64() * 1000.0;
                        withdraw_time += withdraw_duration;
                        events.push(pool);
                        withdraw_count += 1;
                        tracing::info!("⏱️  [{}/{}] parse_withdraw_event={:.3}ms", idx + 1, events_count, withdraw_duration);
                    } else {
                        skipped_count += 1;
                        tracing::debug!("⏱️  [{}/{}] parse_withdraw_event returned None", idx + 1, events_count);
                    }
                }
                _ => {
                    skipped_count += 1;
                    tracing::debug!("⏱️  [{}/{}] skipped unknown event type", idx + 1, events_count);
                }
            }
            
            let event_duration = event_start.elapsed().as_secs_f64() * 1000.0;
            tracing::debug!("⏱️  [{}/{}] process_event_total={:.3}ms", idx + 1, events_count, event_duration);
        }
        let t3 = std::time::Instant::now();
        let process_events_time = (t3 - t2).as_secs_f64() * 1000.0;
        tracing::info!("⏱️  [2/2] process_events={:.3}ms (create: {} processed, {:.3}ms total; deposit: {} processed, {:.3}ms total; withdraw: {} processed, {:.3}ms total; skipped: {})",
            process_events_time, create_count, create_time, deposit_count, deposit_time, withdraw_count, withdraw_time, skipped_count);
        
        let method_duration = method_start.elapsed().as_secs_f64() * 1000.0;
        tracing::info!(
            "✅ PumpswapLiquidityParser::process_liquidity END: total={:.3}ms (parse_events={:.3}ms, process_events={:.3}ms), events_parsed={}, pools_created={}, create={}, deposit={}, withdraw={}, skipped={}",
            method_duration,
            parse_events_time,
            process_events_time,
            events_count,
            events.len(),
            create_count,
            deposit_count,
            withdraw_count,
            skipped_count
        );
        
        events
    }
}

