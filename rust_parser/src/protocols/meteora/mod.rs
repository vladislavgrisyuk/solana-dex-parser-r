pub mod constants;
pub mod meteora_damm_v2_liquidity;
pub mod meteora_dbc_event_parser;
pub mod meteora_dbc_parser;
pub mod meteora_dlmm_liquidity;
pub mod meteora_liquidity_base;
pub mod meteora_parser;
pub mod meteora_pools_liquidity;
pub mod util;

use crate::core::transaction_adapter::TransactionAdapter;
use crate::protocols::simple::{LiquidityParser, MemeEventParser, TradeParser};
use crate::types::{ClassifiedInstruction, DexInfo, TransferMap};

use meteora_dbc_event_parser::MeteoraDBCEventParser;
use meteora_dbc_parser::MeteoraDBCParser;
use meteora_damm_v2_liquidity::MeteoraDAMMV2LiquidityParser;
use meteora_dlmm_liquidity::MeteoraDLMMLiquidityParser;
use meteora_parser::MeteoraParser;
use meteora_pools_liquidity::MeteoraPoolsLiquidityParser;

pub fn build_meteora_trade_parser(
    adapter: TransactionAdapter,
    dex_info: DexInfo,
    transfer_actions: TransferMap,
    classified_instructions: Vec<ClassifiedInstruction>,
) -> Box<dyn TradeParser> {
    Box::new(MeteoraParser::new(
        adapter,
        dex_info,
        transfer_actions,
        classified_instructions,
    ))
}

pub fn build_meteora_dbc_trade_parser(
    adapter: TransactionAdapter,
    dex_info: DexInfo,
    transfer_actions: TransferMap,
    classified_instructions: Vec<ClassifiedInstruction>,
) -> Box<dyn TradeParser> {
    Box::new(MeteoraDBCParser::new(
        adapter,
        dex_info,
        transfer_actions,
        classified_instructions,
    ))
}

pub fn build_meteora_dlmm_liquidity_parser(
    adapter: TransactionAdapter,
    transfer_actions: TransferMap,
    classified_instructions: Vec<ClassifiedInstruction>,
) -> Box<dyn LiquidityParser> {
    Box::new(MeteoraDLMMLiquidityParser::new(
        adapter,
        transfer_actions,
        classified_instructions,
    ))
}

pub fn build_meteora_pools_liquidity_parser(
    adapter: TransactionAdapter,
    transfer_actions: TransferMap,
    classified_instructions: Vec<ClassifiedInstruction>,
) -> Box<dyn LiquidityParser> {
    Box::new(MeteoraPoolsLiquidityParser::new(
        adapter,
        transfer_actions,
        classified_instructions,
    ))
}

pub fn build_meteora_damm_v2_liquidity_parser(
    adapter: TransactionAdapter,
    transfer_actions: TransferMap,
    classified_instructions: Vec<ClassifiedInstruction>,
) -> Box<dyn LiquidityParser> {
    Box::new(MeteoraDAMMV2LiquidityParser::new(
        adapter,
        transfer_actions,
        classified_instructions,
    ))
}

pub fn build_meteora_dbc_meme_parser(
    adapter: TransactionAdapter,
    transfer_actions: TransferMap,
) -> Box<dyn MemeEventParser> {
    Box::new(MeteoraDBCEventParser::new(adapter, transfer_actions))
}

