// Temporary file - will replace dex_parser.rs
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
        let adapter = TransactionAdapter::new(tx, config.clone());
        let utils = TransactionUtils::new(adapter);
        let classifier = InstructionClassifier::new(&utils.adapter);
        let dex_info = utils.get_dex_info(&classifier);
        let transfer_actions = utils.get_transfer_actions();
        let all_program_ids = classifier.get_all_program_ids();

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

        if let Some(program_filter) = config.program_ids.as_ref() {
            if !program_filter.iter().any(|id| all_program_ids.contains(id)) {
                result.state = false;
                return Ok(result);
            }
        }

        if parse_type.includes_trades() {
            for program_id in &all_program_ids {
                if let Some(filter) = config.program_ids.as_ref() {
                    if !filter.iter().any(|id| id == program_id) {
                        continue;
                    }
                }
                if let Some(ignore) = config.ignore_program_ids.as_ref() {
                    if ignore.iter().any(|id| id == program_id) {
                        continue;
                    }
                }

                if let Some(builder) = self.trade_parsers.get(program_id) {
                    let program_id_str = program_id.as_str();
                    let amm_name = dex_info.amm.as_deref()
                        .or_else(|| dex_program_names::name(program_id_str).into())
                        .map(String::from);
                    let program_info = DexInfo {
                        program_id: Some(program_id_str.to_string()),
                        amm: amm_name,
                        route: None,
                    };
                    
                    let adapter_clone = utils.adapter.clone();
                    let transfer_clone = transfer_actions.clone();
                    let classified_instructions = classifier.get_instructions(program_id);
                    
                    let mut parser = builder(
                        adapter_clone,
                        program_info,
                        transfer_clone,
                        classified_instructions,
                    );
                    
                    let trades = parser.process_trades();
                    result.trades.extend(trades);
                } else if config.try_unknown_dex {
                    if let Some(transfers) = transfer_actions.get(program_id) {
                        let has_supported = transfers
                            .iter()
                            .any(|transfer| utils.adapter.is_supported_token(&transfer.info.mint));
                        
                        if transfers.len() >= 2 && has_supported {
                            let program_info = DexInfo {
                                program_id: Some(program_id.clone()),
                                amm: dex_info.amm.clone().or_else(|| Some(dex_program_names::name(program_id).to_string())),
                                route: None,
                            };
                            
                            if let Some(trade) = utils.process_swap_data(transfers, &program_info) {
                                let trade = utils.attach_token_transfer_info(trade, &transfer_actions);
                                result.trades.push(trade);
                            }
                        }
                    }
                }
            }
        }

        if parse_type.includes_liquidity() {
            for program_id in &all_program_ids {
                if let Some(filter) = config.program_ids.as_ref() {
                    if !filter.iter().any(|id| id == program_id) {
                        continue;
                    }
                }
                if let Some(ignore) = config.ignore_program_ids.as_ref() {
                    if ignore.iter().any(|id| id == program_id) {
                        continue;
                    }
                }

                if let Some(builder) = self.liquidity_parsers.get(program_id) {
                    let adapter_clone = utils.adapter.clone();
                    let transfer_clone = transfer_actions.clone();
                    let classified_instructions = classifier.get_instructions(program_id);
                    
                    let mut parser = builder(
                        adapter_clone,
                        transfer_clone,
                        classified_instructions,
                    );
                    
                    let liquidities = parser.process_liquidity();
                    result.liquidities.extend(liquidities);
                }
            }
        }

        if parse_type == ParseType::All {
            for program_id in &all_program_ids {
                if let Some(filter) = config.program_ids.as_ref() {
                    if !filter.iter().any(|id| id == program_id) {
                        continue;
                    }
                }
                if let Some(ignore) = config.ignore_program_ids.as_ref() {
                    if ignore.iter().any(|id| id == program_id) {
                        continue;
                    }
                }

                if let Some(builder) = self.meme_parsers.get(program_id) {
                    let mut parser = builder(utils.adapter.clone(), transfer_actions.clone());
                    let events = parser.process_events();
                    result.meme_events.extend(events);
                }
            }
        }

        if result.trades.is_empty()
            && result.liquidities.is_empty()
            && parse_type.includes_transfer()
        {
            if let Some(program_id) = dex_info.program_id.clone() {
                if let Some(builder) = self.transfer_parsers.get(&program_id) {
                    let classified_instructions = classifier.get_instructions(&program_id);
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
                    let transfers = parser.process_transfers();
                    result.transfers.extend(transfers);
                }
            }

            if result.transfers.is_empty() {
                let fallback_transfers: Vec<_> = transfer_actions.values().flatten().cloned().collect();
                result.transfers.extend(fallback_transfers);
            }
        }

        if !result.trades.is_empty() {
            let before_dedup = result.trades.len();
            let mut seen: HashSet<(String, String)> = HashSet::with_capacity(before_dedup);
            let mut deduped_trades = Vec::with_capacity(before_dedup);
            
            for trade in result.trades {
                let key = (trade.signature.clone(), trade.idx.clone());
                if seen.insert(key) {
                    deduped_trades.push(trade);
                }
            }
            
            result.trades = deduped_trades;
            result.trades.sort_unstable_by(|a, b| a.idx.cmp(&b.idx));
            
            if utils.adapter.config().aggregate_trades {
                if let Some(last_trade) = result.trades.last().cloned() {
                    let trade_with_fee = utils.attach_trade_fee(last_trade);
                    result.aggregate_trade = Some(trade_with_fee);
                }
            }
        }

        Ok(result)
    }

    fn parse_with_classifier(
        &self,
        tx: SolanaTransaction,
        config: Option<ParseConfig>,
        parse_type: ParseType,
    ) -> ParseResult {
        let config = config.unwrap_or_default();
        let config_clone = config.clone();
        match self.try_parse(tx, config_clone, parse_type) {
            Ok(result) => result,
            Err(err) => {
                if config.throw_error {
                    tracing::error!("parser error: {err}");
                }
                let mut result = ParseResult::new();
                result.state = false;
                result.msg = Some(err.to_string());
                result
            }
        }
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
    
    pub fn parse_block_raw_bytes(
        &self,
        transactions_json: &[u8],
        config: Option<ParseConfig>,
    ) -> Result<BlockParseResult, ParserError> {
        let cfg = config.unwrap_or_default();
        let transactions: Vec<Value> = serde_json::from_slice(transactions_json)
            .map_err(|err| ParserError::generic(format!("failed to parse transactions array: {err}")))?;
        
        let mut results = Vec::with_capacity(transactions.len());
        for tx_value in &transactions {
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

