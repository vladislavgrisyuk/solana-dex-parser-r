use std::collections::{HashMap, HashSet};

use crate::config::ParseConfig;
use crate::core::constants::{dex_program_names, dex_programs};
use crate::core::error::ParserError;
use crate::core::instruction_classifier::InstructionClassifier;
use crate::core::transaction_adapter::TransactionAdapter;
use crate::core::transaction_utils::TransactionUtils;
use crate::protocols::pumpfun::{
    build_pumpfun_meme_parser, build_pumpfun_trade_parser, build_pumpswap_liquidity_parser,
    build_pumpswap_trade_parser, build_pumpswap_transfer_parser,
};
use crate::protocols::simple::{
    LiquidityParser, MemeEventParser, SimpleLiquidityParser, SimpleMemeParser, SimpleTradeParser,
    SimpleTransferParser, TradeParser, TransferParser,
};
use crate::types::{
    BlockInput, BlockParseResult, ClassifiedInstruction, DexInfo, FromJsonValue, ParseResult,
    PoolEvent, SolanaBlock, SolanaTransaction, TradeInfo, TransferData, TransferMap,
};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ParseType {
    Trades,
    Liquidity,
    Transfer,
    All,
}

impl ParseType {
    fn includes_trades(self) -> bool {
        matches!(self, ParseType::Trades | ParseType::All)
    }

    fn includes_liquidity(self) -> bool {
        matches!(self, ParseType::Liquidity | ParseType::All)
    }

    fn includes_transfer(self) -> bool {
        matches!(self, ParseType::Transfer | ParseType::All)
    }
}

type TradeParserBuilder = fn(
    TransactionAdapter,
    DexInfo,
    TransferMap,
    Vec<ClassifiedInstruction>,
) -> Box<dyn TradeParser>;

type LiquidityParserBuilder =
    fn(TransactionAdapter, TransferMap, Vec<ClassifiedInstruction>) -> Box<dyn LiquidityParser>;

type TransferParserBuilder = fn(
    TransactionAdapter,
    DexInfo,
    TransferMap,
    Vec<ClassifiedInstruction>,
) -> Box<dyn TransferParser>;

type MemeParserBuilder = fn(TransactionAdapter, TransferMap) -> Box<dyn MemeEventParser>;

pub struct DexParser {
    trade_parsers: HashMap<String, TradeParserBuilder>,
    liquidity_parsers: HashMap<String, LiquidityParserBuilder>,
    transfer_parsers: HashMap<String, TransferParserBuilder>,
    meme_parsers: HashMap<String, MemeParserBuilder>,
}

impl Default for DexParser {
    fn default() -> Self {
        Self::new()
    }
}

impl DexParser {
    pub fn new() -> Self {
        let mut trade_parsers: HashMap<String, TradeParserBuilder> = HashMap::new();
        let mut liquidity_parsers: HashMap<String, LiquidityParserBuilder> = HashMap::new();
        let mut transfer_parsers: HashMap<String, TransferParserBuilder> = HashMap::new();
        let mut meme_parsers: HashMap<String, MemeParserBuilder> = HashMap::new();

        let default_programs = [
            dex_programs::JUPITER,
            dex_programs::RAYDIUM,
            dex_programs::ORCA,
            dex_programs::METEORA,
        ];

        for program in default_programs {
            trade_parsers.insert(program.to_string(), SimpleTradeParser::boxed);
            liquidity_parsers.insert(program.to_string(), SimpleLiquidityParser::boxed);
            transfer_parsers.insert(program.to_string(), SimpleTransferParser::boxed);
            meme_parsers.insert(program.to_string(), SimpleMemeParser::boxed);
        }

        trade_parsers.insert(
            dex_programs::PUMP_FUN.to_string(),
            build_pumpfun_trade_parser,
        );
        trade_parsers.insert(
            dex_programs::PUMP_SWAP.to_string(),
            build_pumpswap_trade_parser,
        );
        liquidity_parsers.insert(
            dex_programs::PUMP_SWAP.to_string(),
            build_pumpswap_liquidity_parser,
        );
        transfer_parsers.insert(
            dex_programs::PUMP_SWAP.to_string(),
            build_pumpswap_transfer_parser,
        );
        meme_parsers.insert(
            dex_programs::PUMP_FUN.to_string(),
            build_pumpfun_meme_parser,
        );

        Self {
            trade_parsers,
            liquidity_parsers,
            transfer_parsers,
            meme_parsers,
        }
    }

    fn try_parse(
        &self,
        tx: SolanaTransaction,
        config: ParseConfig,
        parse_type: ParseType,
    ) -> Result<ParseResult, ParserError> {
        let method_start = std::time::Instant::now();
        tracing::info!("📝 try_parse START: signature={}", tx.signature);
        
        let t0 = std::time::Instant::now();
        let adapter = TransactionAdapter::new(tx, config.clone());
        let t1 = std::time::Instant::now();
        let adapter_time = (t1 - t0).as_secs_f64() * 1000.0;
        tracing::info!("⏱️  [1/8] TransactionAdapter::new={:.3}ms", adapter_time);
        
        let t2 = std::time::Instant::now();
        let utils = TransactionUtils::new(adapter);
        let t3 = std::time::Instant::now();
        let utils_time = (t3 - t2).as_secs_f64() * 1000.0;
        tracing::info!("⏱️  [2/8] TransactionUtils::new={:.3}ms", utils_time);
        
        let t4 = std::time::Instant::now();
        let classifier = InstructionClassifier::new(&utils.adapter);
        let t5 = std::time::Instant::now();
        let classifier_time = (t5 - t4).as_secs_f64() * 1000.0;
        tracing::info!("⏱️  [3/8] InstructionClassifier::new={:.3}ms", classifier_time);
        
        let t6 = std::time::Instant::now();
        let dex_info = utils.get_dex_info(&classifier);
        let t7 = std::time::Instant::now();
        let dex_info_time = (t7 - t6).as_secs_f64() * 1000.0;
        tracing::info!("⏱️  [4/8] utils.get_dex_info={:.3}ms, program_id={:?}, amm={:?}", 
            dex_info_time, dex_info.program_id, dex_info.amm);
        
        let t8 = std::time::Instant::now();
        let transfer_actions = utils.get_transfer_actions();
        let t9 = std::time::Instant::now();
        let transfer_count: usize = transfer_actions.values().map(|v| v.len()).sum();
        let transfer_actions_time = (t9 - t8).as_secs_f64() * 1000.0;
        tracing::info!("⏱️  [5/8] utils.get_transfer_actions={:.3}ms, total_transfers={}, programs={}",
            transfer_actions_time, transfer_count, transfer_actions.len());
        
        let t10 = std::time::Instant::now();
        let all_program_ids = classifier.get_all_program_ids();
        let t11 = std::time::Instant::now();
        let get_program_ids_time = (t11 - t10).as_secs_f64() * 1000.0;
        tracing::info!("⏱️  [6/8] classifier.get_all_program_ids={:.3}ms, count={}",
            get_program_ids_time, all_program_ids.len());
        tracing::info!("DexParser: found {} program IDs to process: {:?}",
            all_program_ids.len(), all_program_ids);

        let t12 = std::time::Instant::now();
        let mut result = ParseResult::new();
        result.slot = utils.adapter.slot();
        result.timestamp = utils.adapter.block_time();
        result.signature = utils.adapter.signature().to_string();
        result.signer = utils.adapter.signers().to_vec();
        result.compute_units = utils.adapter.compute_units();
        result.tx_status = utils.adapter.tx_status();
        result.fee = utils.adapter.fee();

        if let Some(change) = utils.adapter.signer_sol_balance_change() {
            result.sol_balance_change = Some(change);
        }
        if let Some(token_change) = utils.adapter.signer_token_balance_changes() {
            result.token_balance_change = token_change.clone();
        }
        let t13 = std::time::Instant::now();
        let init_result_time = (t13 - t12).as_secs_f64() * 1000.0;
        tracing::info!("⏱️  [7/8] Initialize ParseResult={:.3}ms", init_result_time);

        if let Some(program_filter) = config.program_ids.as_ref() {
            if !program_filter.iter().any(|id| all_program_ids.contains(id)) {
                result.state = false;
                return Ok(result);
            }
        }

        if parse_type.includes_trades() {
            let trades_start = std::time::Instant::now();
            tracing::info!("🔍 Processing TRADES for {} programs", all_program_ids.len());
            
            for (idx, program_id) in all_program_ids.iter().enumerate() {
                let program_start = std::time::Instant::now();
                
                if let Some(filter) = config.program_ids.as_ref() {
                    if !filter.iter().any(|id| id == program_id) {
                        tracing::debug!("⏭️  Skipping program {} (filtered out)", program_id);
                        continue;
                    }
                }
                if let Some(ignore) = config.ignore_program_ids.as_ref() {
                    if ignore.iter().any(|id| id == program_id) {
                        tracing::debug!("⏭️  Skipping program {} (ignored)", program_id);
                        continue;
                    }
                }

                let t0 = std::time::Instant::now();
                let classified_instructions = classifier.get_instructions(program_id);
                let t1 = std::time::Instant::now();
                tracing::debug!(
                    "⏱️  [{}/{}] classifier.get_instructions({})={:.3}μs, found {} instructions",
                    idx + 1,
                    all_program_ids.len(),
                    program_id,
                    (t1 - t0).as_secs_f64() * 1_000_000.0,
                    classified_instructions.len()
                );
                
                if let Some(builder) = self.trade_parsers.get(program_id) {
                    tracing::info!("🔧 Using trade parser for program: {}", program_id);
                    let trade_start = std::time::Instant::now();
                    
                    let t2 = std::time::Instant::now();
                    let mut program_info = DexInfo {
                        program_id: Some(program_id.clone()),
                        amm: dex_info.amm.clone().or_else(|| Some(dex_program_names::name(program_id).to_string())),
                        route: None,
                    };
                    let t3 = std::time::Instant::now();
                    tracing::debug!(
                        "⏱️  [{}/{}] prepare_program_info({})={:.3}μs",
                        idx + 1,
                        all_program_ids.len(),
                        program_id,
                        (t3 - t2).as_secs_f64() * 1_000_000.0
                    );
                    
                    let t4 = std::time::Instant::now();
                    let mut parser = builder(
                        utils.adapter.clone(),
                        program_info,
                        transfer_actions.clone(),
                        classified_instructions,
                    );
                    let t5 = std::time::Instant::now();
                    tracing::debug!(
                        "⏱️  [{}/{}] builder({})={:.3}μs",
                        idx + 1,
                        all_program_ids.len(),
                        program_id,
                        (t5 - t4).as_secs_f64() * 1_000_000.0
                    );
                    
                    let t6 = std::time::Instant::now();
                    tracing::info!("🔹 [{}/{}] Calling process_trades() for program: {}", idx + 1, all_program_ids.len(), program_id);
                    let trades = parser.process_trades();
                    let t7 = std::time::Instant::now();
                    let trade_duration = trade_start.elapsed();
                    let process_trades_time = (t7 - t6).as_secs_f64() * 1000.0;
                    tracing::info!(
                        "⏱️  [{}/{}] parser.process_trades({})={:.3}ms, found {} trades",
                        idx + 1,
                        all_program_ids.len(),
                        program_id,
                        process_trades_time,
                        trades.len()
                    );
                    tracing::info!(
                        "✅ [{}/{}] Parsed {} trades for program {} (builder={:.3}ms, process_trades={:.3}ms, total={:.3}ms)",
                        idx + 1,
                        all_program_ids.len(),
                        trades.len(),
                        program_id,
                        (t5 - t4).as_secs_f64() * 1000.0,
                        process_trades_time,
                        trade_duration.as_secs_f64() * 1000.0
                    );
                    result.trades.extend(trades);
                } else if config.try_unknown_dex {
                    tracing::debug!("🔍 Trying unknown DEX parser for program: {}", program_id);
                    let unknown_start = std::time::Instant::now();
                    
                    if let Some(transfers) = transfer_actions.get(program_id) {
                        let t0 = std::time::Instant::now();
                        let has_supported = transfers
                            .iter()
                            .any(|transfer| utils.adapter.is_supported_token(&transfer.info.mint));
                        let t1 = std::time::Instant::now();
                        tracing::debug!(
                            "⏱️  [{}/{}] check_supported_token({})={:.3}μs, has_supported={}",
                            idx + 1,
                            all_program_ids.len(),
                            program_id,
                            (t1 - t0).as_secs_f64() * 1_000_000.0,
                            has_supported
                        );
                        
                        if transfers.len() >= 2 && has_supported {
                            let t2 = std::time::Instant::now();
                            let program_info = DexInfo {
                                program_id: Some(program_id.clone()),
                                amm: dex_info.amm.clone().or_else(|| Some(dex_program_names::name(program_id).to_string())),
                                route: None,
                            };
                            let t3 = std::time::Instant::now();
                            tracing::debug!(
                                "⏱️  [{}/{}] prepare_program_info_unknown({})={:.3}μs",
                                idx + 1,
                                all_program_ids.len(),
                                program_id,
                                (t3 - t2).as_secs_f64() * 1_000_000.0
                            );
                            
                            let t4 = std::time::Instant::now();
                            let trade_opt = utils.process_swap_data(transfers, &program_info);
                            let t5 = std::time::Instant::now();
                            tracing::debug!(
                                "⏱️  [{}/{}] utils.process_swap_data({})={:.3}μs",
                                idx + 1,
                                all_program_ids.len(),
                                program_id,
                                (t5 - t4).as_secs_f64() * 1_000_000.0
                            );
                            
                            if let Some(trade) = trade_opt {
                                let t6 = std::time::Instant::now();
                                let trade = utils.attach_token_transfer_info(trade, &transfer_actions);
                                let t7 = std::time::Instant::now();
                                tracing::debug!(
                                    "⏱️  [{}/{}] utils.attach_token_transfer_info({})={:.3}μs",
                                    idx + 1,
                                    all_program_ids.len(),
                                    program_id,
                                    (t7 - t6).as_secs_f64() * 1_000_000.0
                                );
                                result.trades.push(trade);
                                tracing::info!(
                                    "✅ [{}/{}] Unknown DEX trade parsed for {} (total={:.3}ms)",
                                    idx + 1,
                                    all_program_ids.len(),
                                    program_id,
                                    unknown_start.elapsed().as_secs_f64() * 1000.0
                                );
                            }
                        }
                    }
                }
                
                let program_duration = program_start.elapsed();
                tracing::debug!(
                    "⏱️  [{}/{}] Total time for program {}: {:.3}ms",
                    idx + 1,
                    all_program_ids.len(),
                    program_id,
                    program_duration.as_secs_f64() * 1000.0
                );
            }
            
            let trades_duration = trades_start.elapsed();
            tracing::info!(
                "✅ TRADES processing complete: total={:.3}ms, trades_found={}",
                trades_duration.as_secs_f64() * 1000.0,
                result.trades.len()
            );
        }

        if parse_type.includes_liquidity() {
            let liquidity_start = std::time::Instant::now();
            tracing::info!("💧 Processing LIQUIDITY for {} programs", all_program_ids.len());
            
            for (idx, program_id) in all_program_ids.iter().enumerate() {
                let program_start = std::time::Instant::now();
                
                if let Some(filter) = config.program_ids.as_ref() {
                    if !filter.iter().any(|id| id == program_id) {
                        tracing::debug!("⏭️  Skipping liquidity for {} (filtered out)", program_id);
                        continue;
                    }
                }
                if let Some(ignore) = config.ignore_program_ids.as_ref() {
                    if ignore.iter().any(|id| id == program_id) {
                        tracing::debug!("⏭️  Skipping liquidity for {} (ignored)", program_id);
                        continue;
                    }
                }
                
                if let Some(builder) = self.liquidity_parsers.get(program_id) {
                    tracing::info!("🔧 Using liquidity parser for program: {}", program_id);
                    
                    let t0 = std::time::Instant::now();
                    let classified_instructions = classifier.get_instructions(program_id);
                    let t1 = std::time::Instant::now();
                    tracing::debug!(
                        "⏱️  [{}/{}] classifier.get_instructions({})={:.3}μs, found {} instructions",
                        idx + 1,
                        all_program_ids.len(),
                        program_id,
                        (t1 - t0).as_secs_f64() * 1_000_000.0,
                        classified_instructions.len()
                    );
                    
                    let t2 = std::time::Instant::now();
                    let mut parser = builder(
                        utils.adapter.clone(),
                        transfer_actions.clone(),
                        classified_instructions,
                    );
                    let t3 = std::time::Instant::now();
                    tracing::debug!(
                        "⏱️  [{}/{}] liquidity_builder({})={:.3}μs",
                        idx + 1,
                        all_program_ids.len(),
                        program_id,
                        (t3 - t2).as_secs_f64() * 1_000_000.0
                    );
                    
                    let t4 = std::time::Instant::now();
                    tracing::info!("🔹 [{}/{}] Calling process_liquidity() for program: {}", idx + 1, all_program_ids.len(), program_id);
                    let liquidities = parser.process_liquidity();
                    let t5 = std::time::Instant::now();
                    let process_liquidity_time = (t5 - t4).as_secs_f64() * 1000.0;
                    tracing::info!(
                        "⏱️  [{}/{}] parser.process_liquidity({})={:.3}ms, found {} events",
                        idx + 1,
                        all_program_ids.len(),
                        program_id,
                        process_liquidity_time,
                        liquidities.len()
                    );
                    result.liquidities.extend(liquidities);
                    
                    let program_duration = program_start.elapsed();
                    tracing::info!(
                        "✅ [{}/{}] Parsed liquidity for {} (total={:.3}ms)",
                        idx + 1,
                        all_program_ids.len(),
                        program_id,
                        program_duration.as_secs_f64() * 1000.0
                    );
                }
            }
            
            let liquidity_duration = liquidity_start.elapsed();
            tracing::info!(
                "✅ LIQUIDITY processing complete: total={:.3}ms, events_found={}",
                liquidity_duration.as_secs_f64() * 1000.0,
                result.liquidities.len()
            );
        }

        if parse_type == ParseType::All {
            let meme_start = std::time::Instant::now();
            tracing::info!("🎭 Processing MEME EVENTS for {} programs", all_program_ids.len());
            
            for (idx, program_id) in all_program_ids.iter().enumerate() {
                let program_start = std::time::Instant::now();
                
                if let Some(filter) = config.program_ids.as_ref() {
                    if !filter.iter().any(|id| id == program_id) {
                        tracing::debug!("⏭️  Skipping meme events for {} (filtered out)", program_id);
                        continue;
                    }
                }
                if let Some(ignore) = config.ignore_program_ids.as_ref() {
                    if ignore.iter().any(|id| id == program_id) {
                        tracing::debug!("⏭️  Skipping meme events for {} (ignored)", program_id);
                        continue;
                    }
                }
                
                if let Some(builder) = self.meme_parsers.get(program_id) {
                    tracing::info!("🔧 Using meme parser for program: {}", program_id);
                    
                    let t0 = std::time::Instant::now();
                    let mut parser = builder(utils.adapter.clone(), transfer_actions.clone());
                    let t1 = std::time::Instant::now();
                    tracing::debug!(
                        "⏱️  [{}/{}] meme_builder({})={:.3}μs",
                        idx + 1,
                        all_program_ids.len(),
                        program_id,
                        (t1 - t0).as_secs_f64() * 1_000_000.0
                    );
                    
                    let t2 = std::time::Instant::now();
                    let events = parser.process_events();
                    let t3 = std::time::Instant::now();
                    tracing::debug!(
                        "⏱️  [{}/{}] parser.process_events({})={:.3}ms, found {} events",
                        idx + 1,
                        all_program_ids.len(),
                        program_id,
                        (t3 - t2).as_secs_f64() * 1000.0,
                        events.len()
                    );
                    result.meme_events.extend(events);
                    
                    let program_duration = program_start.elapsed();
                    tracing::info!(
                        "✅ [{}/{}] Parsed meme events for {} (total={:.3}ms)",
                        idx + 1,
                        all_program_ids.len(),
                        program_id,
                        program_duration.as_secs_f64() * 1000.0
                    );
                }
            }
            
            let meme_duration = meme_start.elapsed();
            tracing::info!(
                "✅ MEME EVENTS processing complete: total={:.3}ms, events_found={}",
                meme_duration.as_secs_f64() * 1000.0,
                result.meme_events.len()
            );
        }

        if result.trades.is_empty()
            && result.liquidities.is_empty()
            && parse_type.includes_transfer()
        {
            let transfer_start = std::time::Instant::now();
            tracing::info!("📤 Processing TRANSFERS");
            
            if let Some(program_id) = dex_info.program_id.clone() {
                tracing::info!("🔧 Using transfer parser for program: {}", program_id);
                
                if let Some(builder) = self.transfer_parsers.get(&program_id) {
                    let t0 = std::time::Instant::now();
                    let classified_instructions = classifier.get_instructions(&program_id);
                    let t1 = std::time::Instant::now();
                    tracing::debug!(
                        "⏱️  classifier.get_instructions({})={:.3}μs, found {} instructions",
                        program_id,
                        (t1 - t0).as_secs_f64() * 1_000_000.0,
                        classified_instructions.len()
                    );
                    
                    let t2 = std::time::Instant::now();
                    let mut program_info = DexInfo {
                        program_id: dex_info.program_id.clone(),
                        amm: dex_info.amm.clone(),
                        route: None,
                    };
                    let mut parser = builder(
                        utils.adapter.clone(),
                        program_info,
                        transfer_actions.clone(),
                        classified_instructions,
                    );
                    let t3 = std::time::Instant::now();
                    tracing::debug!(
                        "⏱️  transfer_builder({})={:.3}μs",
                        program_id,
                        (t3 - t2).as_secs_f64() * 1_000_000.0
                    );
                    
                    let t4 = std::time::Instant::now();
                    let transfers = parser.process_transfers();
                    let t5 = std::time::Instant::now();
                    tracing::debug!(
                        "⏱️  parser.process_transfers({})={:.3}ms, found {} transfers",
                        program_id,
                        (t5 - t4).as_secs_f64() * 1000.0,
                        transfers.len()
                    );
                    result.transfers.extend(transfers);
                }
            }
            
            if result.transfers.is_empty() {
                let t0 = std::time::Instant::now();
                let fallback_transfers: Vec<_> = transfer_actions.values().flatten().cloned().collect();
                let t1 = std::time::Instant::now();
                tracing::debug!(
                    "⏱️  fallback_transfers={:.3}μs, found {} transfers",
                    (t1 - t0).as_secs_f64() * 1_000_000.0,
                    fallback_transfers.len()
                );
                result.transfers.extend(fallback_transfers);
            }
            
            let transfer_duration = transfer_start.elapsed();
            tracing::info!(
                "✅ TRANSFERS processing complete: total={:.3}ms, transfers_found={}",
                transfer_duration.as_secs_f64() * 1000.0,
                result.transfers.len()
            );
        }

        let t14 = std::time::Instant::now();
        if !result.trades.is_empty() {
            let postprocess_start = std::time::Instant::now();
            tracing::info!("🔧 Post-processing {} trades", result.trades.len());
            
            let t0 = std::time::Instant::now();
            let mut seen = HashSet::with_capacity(result.trades.len());
            let before_dedup = result.trades.len();
            result
                .trades
                .retain(|trade| seen.insert((trade.signature.clone(), trade.idx.clone())));
            let after_dedup = result.trades.len();
            let t1 = std::time::Instant::now();
            let dedup_time = (t1 - t0).as_secs_f64() * 1000.0;
            tracing::info!(
                "⏱️  deduplicate_trades={:.3}ms, before={}, after={}, removed={}",
                dedup_time, before_dedup, after_dedup, before_dedup - after_dedup
            );
            
            let t2 = std::time::Instant::now();
            result.trades.sort_by(|a, b| a.idx.cmp(&b.idx));
            let t3 = std::time::Instant::now();
            let sort_time = (t3 - t2).as_secs_f64() * 1000.0;
            tracing::info!("⏱️  sort_trades={:.3}ms", sort_time);
            
            if utils.adapter.config().aggregate_trades {
                let t4 = std::time::Instant::now();
                if let Some(last_trade) = result.trades.last().cloned() {
                    let t5 = std::time::Instant::now();
                    let trade_with_fee = utils.attach_trade_fee(last_trade);
                    let t6 = std::time::Instant::now();
                    let attach_fee_time = (t6 - t5).as_secs_f64() * 1000.0;
                    tracing::info!("⏱️  attach_trade_fee={:.3}ms", attach_fee_time);
                    result.aggregate_trade = Some(trade_with_fee);
                }
                let t7 = std::time::Instant::now();
                let aggregate_time = (t7 - t4).as_secs_f64() * 1000.0;
                tracing::info!("⏱️  aggregate_trades_total={:.3}ms", aggregate_time);
            }
            
            let postprocess_duration = postprocess_start.elapsed();
            tracing::info!(
                "✅ Post-processing complete: total={:.3}ms",
                postprocess_duration.as_secs_f64() * 1000.0
            );
        }
        let t15 = std::time::Instant::now();
        let postprocess_time = (t15 - t14).as_secs_f64() * 1000.0;
        tracing::info!("⏱️  [8/8] Post-processing={:.3}ms", postprocess_time);

        let method_duration = method_start.elapsed();
        let total_time = method_duration.as_secs_f64() * 1000.0;
        tracing::info!(
            "✅ try_parse END: total={:.3}ms (adapter={:.3}ms, utils={:.3}ms, classifier={:.3}ms, dex_info={:.3}ms, transfers={:.3}ms, program_ids={:.3}ms, init={:.3}ms, postprocess={:.3}ms), trades={}, liquidities={}, transfers={}, meme_events={}, state={}",
            total_time,
            adapter_time, utils_time, classifier_time, dex_info_time, transfer_actions_time, get_program_ids_time, init_result_time, postprocess_time,
            result.trades.len(),
            result.liquidities.len(),
            result.transfers.len(),
            result.meme_events.len(),
            result.state
        );

        Ok(result)
    }

    fn parse_with_classifier(
        &self,
        tx: SolanaTransaction,
        config: Option<ParseConfig>,
        parse_type: ParseType,
    ) -> ParseResult {
        let method_start = std::time::Instant::now();
        let parse_type_str = match parse_type {
            ParseType::Trades => "Trades",
            ParseType::Liquidity => "Liquidity",
            ParseType::Transfer => "Transfer",
            ParseType::All => "All",
        };
        tracing::info!(
            "🚀 parse_with_classifier START: parse_type={}, signature={}",
            parse_type_str,
            tx.signature
        );
        
        let t0 = std::time::Instant::now();
        let config = config.unwrap_or_default();
        let t1 = std::time::Instant::now();
        tracing::debug!(
            "⏱️  parse_with_classifier: config_unwrap={:.3}μs",
            (t1 - t0).as_secs_f64() * 1_000_000.0
        );
        
        let t2 = std::time::Instant::now();
        let config_clone = config.clone();
        let result = match self.try_parse(tx, config_clone, parse_type) {
            Ok(result) => {
                let t3 = std::time::Instant::now();
                tracing::debug!(
                    "⏱️  parse_with_classifier: try_parse SUCCESS={:.3}ms",
                    (t3 - t2).as_secs_f64() * 1000.0
                );
                result
            },
            Err(err) => {
                let t3 = std::time::Instant::now();
                tracing::debug!(
                    "⏱️  parse_with_classifier: try_parse ERROR={:.3}ms, error={}",
                    (t3 - t2).as_secs_f64() * 1000.0,
                    err
                );
                if config.throw_error {
                    tracing::error!("parser error: {err}");
                }
                let mut result = ParseResult::new();
                result.state = false;
                result.msg = Some(err.to_string());
                result
            }
        };
        
        let method_duration = method_start.elapsed();
        tracing::info!(
            "✅ parse_with_classifier END: total={:.3}ms, parse_type={}, trades={}, liquidities={}, transfers={}, state={}",
            method_duration.as_secs_f64() * 1000.0,
            parse_type_str,
            result.trades.len(),
            result.liquidities.len(),
            result.transfers.len(),
            result.state
        );
        
        result
    }

    pub fn parse_trades(
        &self,
        tx: SolanaTransaction,
        config: Option<ParseConfig>,
    ) -> Vec<TradeInfo> {
        self.parse_with_classifier(tx, config, ParseType::Trades)
            .trades
    }

    pub fn parse_liquidity(
        &self,
        tx: SolanaTransaction,
        config: Option<ParseConfig>,
    ) -> Vec<PoolEvent> {
        self.parse_with_classifier(tx, config, ParseType::Liquidity)
            .liquidities
    }

    pub fn parse_transfers(
        &self,
        tx: SolanaTransaction,
        config: Option<ParseConfig>,
    ) -> Vec<TransferData> {
        self.parse_with_classifier(tx, config, ParseType::Transfer)
            .transfers
    }

    pub fn parse_all(&self, tx: SolanaTransaction, config: Option<ParseConfig>) -> ParseResult {
        self.parse_with_classifier(tx, config, ParseType::All)
    }

    pub fn parse_block_raw(
        &self,
        transactions: &[Value],
        config: Option<ParseConfig>,
    ) -> Result<BlockParseResult, ParserError> {
        let cfg = config.unwrap_or_default();
        let mut results = Vec::with_capacity(transactions.len());
        for tx_value in transactions {
            // Optimized: use from_value directly (Value is already parsed, no need to serialize/deserialize)
            let tx = SolanaTransaction::from_value(tx_value, &cfg)
                .map_err(|err| ParserError::generic(err.to_string()))?;
            results.push(self.parse_all(tx, Some(cfg.clone())));
        }
        Ok(BlockParseResult {
            slot: 0,
            timestamp: None,
            transactions: results,
        })
    }
    
    /// Fast path: parse block from JSON bytes directly
    pub fn parse_block_raw_bytes(
        &self,
        transactions_json: &[u8],
        config: Option<ParseConfig>,
    ) -> Result<BlockParseResult, ParserError> {
        let cfg = config.unwrap_or_default();
        // Parse array of transactions from bytes
        let transactions: Vec<Value> = serde_json::from_slice(transactions_json)
            .map_err(|err| ParserError::generic(format!("failed to parse transactions array: {err}")))?;
        
        let mut results = Vec::with_capacity(transactions.len());
        for tx_value in &transactions {
            // Serialize each transaction to bytes for fast parsing
            let bytes = serde_json::to_vec(tx_value)
                .map_err(|err| ParserError::generic(format!("failed to serialize transaction: {err}")))?;
            let tx = SolanaTransaction::from_slice(&bytes, &cfg)
                .map_err(|err| ParserError::generic(err.to_string()))?;
            results.push(self.parse_all(tx, Some(cfg.clone())));
        }
        Ok(BlockParseResult {
            slot: 0,
            timestamp: None,
            transactions: results,
        })
    }

    pub fn parse_block_parsed(
        &self,
        block: &SolanaBlock,
        config: Option<ParseConfig>,
    ) -> BlockParseResult {
        let cfg = config.unwrap_or_default();
        let mut results = Vec::with_capacity(block.transactions.len());
        for tx in &block.transactions {
            results.push(self.parse_all(tx.clone(), Some(cfg.clone())));
        }
        BlockParseResult {
            slot: block.slot,
            timestamp: block.block_time,
            transactions: results,
        }
    }

    pub fn parse_block(
        &self,
        input: &BlockInput,
        config: Option<ParseConfig>,
    ) -> Result<BlockParseResult, ParserError> {
        match input {
            BlockInput::Raw { transactions } => self.parse_block_raw(transactions, config),
            BlockInput::Parsed { block } => Ok(self.parse_block_parsed(block, config)),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::config::ParseConfig;
    use crate::core::constants::dex_programs;
    use crate::types::{
        BalanceChange, SolanaInstruction, TokenAmount, TransactionMeta, TransactionStatus,
        TransferData,
    };

    fn sample_transaction() -> SolanaTransaction {
        let mut sol_changes = HashMap::new();
        sol_changes.insert(
            "user".to_string(),
            BalanceChange {
                pre: 10_000_000,
                post: 9_995_000,
                change: -5_000,
            },
        );

        let mut token_changes = HashMap::new();
        let mut signer_tokens = HashMap::new();
        signer_tokens.insert(
            "BASE".to_string(),
            BalanceChange {
                pre: 0,
                post: -1_000_000,
                change: -1_000_000,
            },
        );
        signer_tokens.insert(
            "QUOTE".to_string(),
            BalanceChange {
                pre: 0,
                post: 2_000_000,
                change: 2_000_000,
            },
        );
        token_changes.insert("user".to_string(), signer_tokens);

        SolanaTransaction {
            slot: 1,
            signature: "sample-signature".to_string(),
            block_time: 1_234_567,
            signers: vec!["user".to_string()],
            instructions: vec![SolanaInstruction {
                program_id: dex_programs::JUPITER.to_string(),
                accounts: vec!["BASE".to_string(), "QUOTE".to_string()],
                data: "swap".to_string(),
            }],
            inner_instructions: Vec::new(),
            transfers: vec![
                TransferData {
                    transfer_type: "transfer".to_string(),
                    program_id: dex_programs::JUPITER.to_string(),
                    info: crate::types::TransferInfo {
                        authority: Some("user".to_string()),
                        destination: "pool".to_string(),
                        destination_owner: Some("pool-owner".to_string()),
                        mint: "BASE".to_string(),
                        source: "user-token".to_string(),
                        token_amount: TokenAmount::new("1000000", 6, Some(1.0)),
                        source_balance: None,
                        source_pre_balance: None,
                        destination_balance: None,
                        destination_pre_balance: None,
                        sol_balance_change: None,
                    },
                    idx: "0-0".to_string(),
                    timestamp: 1_234_567,
                    signature: "sample-signature".to_string(),
                    is_fee: false,
                },
                TransferData {
                    transfer_type: "transfer".to_string(),
                    program_id: dex_programs::JUPITER.to_string(),
                    info: crate::types::TransferInfo {
                        authority: Some("pool".to_string()),
                        destination: "user".to_string(),
                        destination_owner: Some("user".to_string()),
                        mint: "QUOTE".to_string(),
                        source: "pool-token".to_string(),
                        token_amount: TokenAmount::new("2000000", 6, Some(2.0)),
                        source_balance: None,
                        source_pre_balance: None,
                        destination_balance: None,
                        destination_pre_balance: None,
                        sol_balance_change: None,
                    },
                    idx: "0-1".to_string(),
                    timestamp: 1_234_567,
                    signature: "sample-signature".to_string(),
                    is_fee: false,
                },
            ],
            pre_token_balances: Vec::new(),
            post_token_balances: Vec::new(),
            meta: TransactionMeta {
                fee: 5_000,
                compute_units: 200_000,
                status: TransactionStatus::Success,
                sol_balance_changes: sol_changes,
                token_balance_changes: token_changes,
            },
        }
    }

    #[test]
    fn parses_trade_and_aggregates() {
        let parser = DexParser::new();
        let result = parser.parse_all(sample_transaction(), None);

        assert!(result.state);
        assert_eq!(result.trades.len(), 1);
        assert!(result.aggregate_trade.is_some());
        let trade = &result.trades[0];
        assert_eq!(trade.program_id.as_deref(), Some(dex_programs::JUPITER));
        assert_eq!(trade.input_token.mint, "BASE");
        assert_eq!(trade.output_token.mint, "QUOTE");
        assert_eq!(result.fee.amount, "5000");
        assert!(result.sol_balance_change.is_some());
    }

    #[test]
    fn falls_back_to_transfers_when_no_trade() {
        let mut tx = sample_transaction();
        tx.instructions[0].program_id = "UNKNOWN_PROGRAM".to_string();
        tx.transfers.iter_mut().for_each(|transfer| {
            transfer.program_id = "UNKNOWN_PROGRAM".to_string();
        });

        let parser = DexParser::new();
        let config = ParseConfig {
            try_unknown_dex: false,
            program_ids: None,
            ignore_program_ids: None,
            aggregate_trades: false,
            throw_error: false,
        };
        let transfers = parser.parse_transfers(tx.clone(), Some(config.clone()));
        assert_eq!(transfers.len(), 2);
        assert!(parser.parse_trades(tx, Some(config)).is_empty());
    }
}
