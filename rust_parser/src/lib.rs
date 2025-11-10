//! Core library entry point exposing the parser and public data types.

pub mod config;
pub mod core;
pub mod protocols;
pub mod rpc;
pub mod types;

pub use crate::config::ParseConfig;
pub use crate::core::dex_parser::DexParser;
pub use crate::types::{
    BalanceChange, BlockInput, BlockParseResult, ClassifiedInstruction, DexInfo, MemeEvent,
    ParseResult, PoolEvent, SolanaBlock, SolanaInstruction, SolanaTransaction, TokenAmount,
    TradeInfo, TransactionMeta, TransactionStatus, TransferData,
};
