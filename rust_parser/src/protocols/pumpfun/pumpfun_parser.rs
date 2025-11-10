use crate::core::instruction_classifier::InstructionClassifier;
use crate::core::transaction_adapter::TransactionAdapter;
use crate::protocols::simple::{MemeEventParser, TradeParser};
use crate::types::{ClassifiedInstruction, DexInfo, MemeEvent, TradeInfo, TradeType, TransferMap};

use super::constants::PUMP_FUN_PROGRAM_ID;
use super::error::PumpfunError;
use super::pumpfun_event_parser::PumpfunEventParser;
use super::util::{attach_token_transfers, get_pumpfun_trade_info};

pub struct PumpfunParser {
    adapter: TransactionAdapter,
    dex_info: DexInfo,
    transfer_actions: TransferMap,
    classified_instructions: Vec<ClassifiedInstruction>,
    event_parser: PumpfunEventParser,
}

impl PumpfunParser {
    pub fn new(
        adapter: TransactionAdapter,
        dex_info: DexInfo,
        transfer_actions: TransferMap,
        classified_instructions: Vec<ClassifiedInstruction>,
    ) -> Self {
        let event_parser = PumpfunEventParser::new(adapter.clone());
        Self {
            adapter,
            dex_info,
            transfer_actions,
            classified_instructions,
            event_parser,
        }
    }

    fn parse_events(&self) -> Result<Vec<MemeEvent>, PumpfunError> {
        let start = std::time::Instant::now();
        let result = self.event_parser
            .parse_instructions(&self.classified_instructions);
        let duration = start.elapsed();
        
        match &result {
            Ok(events) => {
                tracing::debug!(
                    "⏱️  PumpfunParser::parse_events: total={:.3}ms, events_count={}, instructions_count={}",
                    duration.as_secs_f64() * 1000.0,
                    events.len(),
                    self.classified_instructions.len()
                );
            },
            Err(err) => {
                tracing::debug!(
                    "⏱️  PumpfunParser::parse_events: ERROR={:.3}ms, error={}, instructions_count={}",
                    duration.as_secs_f64() * 1000.0,
                    err,
                    self.classified_instructions.len()
                );
            }
        }
        
        result
    }
}

impl TradeParser for PumpfunParser {
    fn process_trades(&mut self) -> Vec<TradeInfo> {
        let method_start = std::time::Instant::now();
        tracing::info!("🔧 PumpfunParser::process_trades START");
        
        let t0 = std::time::Instant::now();
        let parse_result = self.parse_events();
        let t1 = std::time::Instant::now();
        tracing::debug!(
            "⏱️  PumpfunParser::process_trades: parse_events={:.3}ms",
            (t1 - t0).as_secs_f64() * 1000.0
        );
        
        match parse_result {
            Ok(events) => {
                let events_count = events.len();
                tracing::debug!(
                    "⏱️  PumpfunParser::process_trades: parsed {} events",
                    events_count
                );
                
                let t2 = std::time::Instant::now();
                let filtered_events: Vec<_> = events
                    .into_iter()
                    .filter(|event| matches!(event.event_type, TradeType::Buy | TradeType::Sell))
                    .collect();
                let t3 = std::time::Instant::now();
                let filtered_count = filtered_events.len();
                tracing::debug!(
                    "⏱️  PumpfunParser::process_trades: filter_trade_events={:.3}μs, input={}, output={}",
                    (t3 - t2).as_secs_f64() * 1_000_000.0,
                    events_count,
                    filtered_count
                );
                
                let t4 = std::time::Instant::now();
                let trades: Vec<TradeInfo> = filtered_events
                    .into_iter()
                    .enumerate()
                    .map(|(idx, event)| {
                        let event_start = std::time::Instant::now();
                        
                        let t0 = std::time::Instant::now();
                        let trade = get_pumpfun_trade_info(&event, &self.adapter, &self.dex_info);
                        let t1 = std::time::Instant::now();
                        tracing::debug!(
                            "⏱️  PumpfunParser::process_trades: [{}/{}] get_pumpfun_trade_info={:.3}μs, event_type={:?}",
                            idx + 1,
                            filtered_count,
                            (t1 - t0).as_secs_f64() * 1_000_000.0,
                            event.event_type
                        );
                        
                        let t2 = std::time::Instant::now();
                        let result = attach_token_transfers(&self.adapter, trade, &self.transfer_actions);
                        let t3 = std::time::Instant::now();
                        tracing::debug!(
                            "⏱️  PumpfunParser::process_trades: [{}/{}] attach_token_transfers={:.3}μs",
                            idx + 1,
                            filtered_count,
                            (t3 - t2).as_secs_f64() * 1_000_000.0
                        );
                        
                        let event_duration = event_start.elapsed();
                        tracing::debug!(
                            "⏱️  PumpfunParser::process_trades: [{}/{}] process_event_total={:.3}μs",
                            idx + 1,
                            filtered_count,
                            event_duration.as_secs_f64() * 1_000_000.0
                        );
                        
                        result
                    })
                    .collect();
                let t5 = std::time::Instant::now();
                tracing::debug!(
                    "⏱️  PumpfunParser::process_trades: map_to_trades={:.3}ms, trades_count={}",
                    (t5 - t4).as_secs_f64() * 1000.0,
                    trades.len()
                );
                
                let method_duration = method_start.elapsed();
                tracing::info!(
                    "✅ PumpfunParser::process_trades END: total={:.3}ms, events_parsed={}, trades_found={}",
                    method_duration.as_secs_f64() * 1000.0,
                    events_count,
                    trades.len()
                );
                
                trades
            },
            Err(err) => {
                let method_duration = method_start.elapsed();
                tracing::error!(
                    "❌ PumpfunParser::process_trades ERROR: total={:.3}ms, error={}",
                    method_duration.as_secs_f64() * 1000.0,
                    err
                );
                Vec::new()
            }
        }
    }
}

pub struct PumpfunMemeParser {
    adapter: TransactionAdapter,
    _transfer_actions: TransferMap,
}

impl PumpfunMemeParser {
    pub fn new(adapter: TransactionAdapter, transfer_actions: TransferMap) -> Self {
        Self {
            adapter,
            _transfer_actions: transfer_actions,
        }
    }
}

impl MemeEventParser for PumpfunMemeParser {
    fn process_events(&mut self) -> Vec<MemeEvent> {
        let classifier = InstructionClassifier::new(&self.adapter);
        let instructions = classifier.get_instructions(PUMP_FUN_PROGRAM_ID);
        let parser = PumpfunEventParser::new(self.adapter.clone());
        match parser.parse_instructions(&instructions) {
            Ok(events) => events,
            Err(err) => {
                tracing::error!("failed to parse pumpfun meme events: {err}");
                Vec::new()
            }
        }
    }
}
