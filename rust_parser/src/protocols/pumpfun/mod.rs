pub mod binary_reader;
pub mod constants;
pub mod error;
pub mod pumpfun_event_parser;
pub mod pumpfun_instruction_parser;
pub mod pumpfun_parser;
pub mod pumpswap_event_parser;
pub mod pumpswap_instruction_parser;
pub mod pumpswap_liquidity_parser;
pub mod pumpswap_parser;
pub mod pumpswap_parser_zc;
pub mod util;

use crate::core::transaction_adapter::TransactionAdapter;
use crate::protocols::simple::{LiquidityParser, MemeEventParser, TradeParser, TransferParser};
use crate::types::{ClassifiedInstruction, DexInfo, TransferMap};

use pumpfun_parser::PumpfunParser;
use pumpswap_liquidity_parser::PumpswapLiquidityParser;
use pumpswap_parser::PumpswapParser;

pub fn build_pumpfun_trade_parser(
    adapter: TransactionAdapter,
    dex_info: DexInfo,
    transfer_actions: TransferMap,
    classified_instructions: Vec<ClassifiedInstruction>,
) -> Box<dyn TradeParser> {
    Box::new(PumpfunParser::new(
        adapter,
        dex_info,
        transfer_actions,
        classified_instructions,
    ))
}

pub fn build_pumpswap_trade_parser(
    adapter: TransactionAdapter,
    dex_info: DexInfo,
    transfer_actions: TransferMap,
    classified_instructions: Vec<ClassifiedInstruction>,
) -> Box<dyn TradeParser> {
    Box::new(PumpswapParser::new(
        adapter,
        dex_info,
        transfer_actions,
        classified_instructions,
    ))
}

pub fn build_pumpswap_liquidity_parser(
    adapter: TransactionAdapter,
    transfer_actions: TransferMap,
    classified_instructions: Vec<ClassifiedInstruction>,
) -> Box<dyn LiquidityParser> {
    Box::new(PumpswapLiquidityParser::new(
        adapter,
        transfer_actions,
        classified_instructions,
    ))
}

pub fn build_pumpfun_meme_parser(
    adapter: TransactionAdapter,
    transfer_actions: TransferMap,
) -> Box<dyn MemeEventParser> {
    Box::new(pumpfun_parser::PumpfunMemeParser::new(
        adapter,
        transfer_actions,
    ))
}

pub fn build_pumpswap_transfer_parser(
    adapter: TransactionAdapter,
    dex_info: DexInfo,
    transfer_actions: TransferMap,
    classified_instructions: Vec<ClassifiedInstruction>,
) -> Box<dyn TransferParser> {
    // Pumpswap reuses the generic transfer parser for now.
    crate::protocols::simple::SimpleTransferParser::boxed(
        adapter,
        dex_info,
        transfer_actions,
        classified_instructions,
    )
}
