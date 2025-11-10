use std::collections::HashMap;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use crate::config::ParseConfig;

/// Representation of a raw token amount and its UI value.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TokenAmount {
    pub amount: String,
    #[serde(default)]
    pub ui_amount: Option<f64>,
    pub decimals: u8,
}

impl TokenAmount {
    pub fn new(amount: impl Into<String>, decimals: u8, ui_amount: Option<f64>) -> Self {
        Self {
            amount: amount.into(),
            ui_amount,
            decimals,
        }
    }
}

impl Default for TokenAmount {
    fn default() -> Self {
        Self {
            amount: "0".to_string(),
            ui_amount: Some(0.0),
            decimals: 9,
        }
    }
}

/// Token balance change helper struct used for SOL/token deltas.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BalanceChange {
    pub pre: i128,
    pub post: i128,
    pub change: i128,
}

/// Snapshot of a token account balance from transaction meta.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TokenBalance {
    pub account: String,
    pub mint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(rename = "uiTokenAmount")]
    pub ui_token_amount: TokenAmount,
}

/// Execution status for a Solana transaction.
#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TransactionStatus {
    #[serde(alias = "UNKNOWN")]
    Unknown,
    Success,
    Failed,
}

impl Default for TransactionStatus {
    fn default() -> Self {
        Self::Unknown
    }
}

/// Trade directions supported by the parser.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum TradeType {
    Buy,
    Sell,
    #[default]
    Swap,
    Create,
    Migrate,
    Complete,
    Add,
    Remove,
    Lock,
    Burn,
}

/// Pool event types (CREATE, ADD, REMOVE).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum PoolEventType {
    #[default]
    Create,
    Add,
    Remove,
}

/// Base pool event structure (shared fields).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PoolEventBase {
    pub user: String,
    #[serde(rename = "type")]
    pub event_type: PoolEventType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub program_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amm: Option<String>,
    pub slot: u64,
    pub timestamp: u64,
    pub signature: String,
    pub idx: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer: Option<Vec<String>>,
}

/// Detailed token information used for trades and events.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TokenInfo {
    pub mint: String,
    pub amount: f64,
    pub amount_raw: String,
    pub decimals: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authority: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination_owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination_balance: Option<TokenAmount>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination_pre_balance: Option<TokenAmount>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_balance: Option<TokenAmount>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_pre_balance: Option<TokenAmount>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination_balance_change: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_balance_change: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub balance_change: Option<String>,
}

/// Fee information associated with a trade.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FeeInfo {
    pub mint: String,
    pub amount: f64,
    pub amount_raw: String,
    pub decimals: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dex: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "type")]
    pub fee_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recipient: Option<String>,
}

/// High level trade information extracted from a transaction.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TradeInfo {
    #[serde(rename = "type")]
    pub trade_type: TradeType,
    #[serde(rename = "Pool", default)]
    pub pool: Vec<String>,
    pub input_token: TokenInfo,
    pub output_token: TokenInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slippage_bps: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fee: Option<FeeInfo>,
    #[serde(default)]
    pub fees: Vec<FeeInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub program_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amm: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amms: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route: Option<String>,
    pub slot: u64,
    pub timestamp: u64,
    pub signature: String,
    pub idx: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer: Option<Vec<String>>,
}

/// Detailed transfer information mirroring the TypeScript structure.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TransferInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authority: Option<String>,
    pub destination: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination_owner: Option<String>,
    pub mint: String,
    pub source: String,
    pub token_amount: TokenAmount,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_balance: Option<TokenAmount>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_pre_balance: Option<TokenAmount>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination_balance: Option<TokenAmount>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination_pre_balance: Option<TokenAmount>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sol_balance_change: Option<String>,
}

/// Transfer data emitted by the meta simulation.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TransferData {
    #[serde(rename = "type")]
    pub transfer_type: String,
    pub program_id: String,
    pub info: TransferInfo,
    pub idx: String,
    pub timestamp: u64,
    pub signature: String,
    #[serde(default)]
    pub is_fee: bool,
}

/// High level liquidity pool event (add/remove liquidity etc.).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PoolEvent {
    pub user: String,
    #[serde(rename = "type")]
    pub event_type: TradeType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub program_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amm: Option<String>,
    pub slot: u64,
    pub timestamp: u64,
    pub signature: String,
    pub idx: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer: Option<Vec<String>>,
    pub pool_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool_lp_mint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token0_mint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token0_amount: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token0_amount_raw: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token0_balance_change: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token0_decimals: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token1_mint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token1_amount: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token1_amount_raw: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token1_balance_change: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token1_decimals: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lp_amount: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lp_amount_raw: Option<String>,
}

/// Meme/launch events emitted by platforms such as Pumpfun.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MemeEvent {
    #[serde(rename = "type")]
    pub event_type: TradeType,
    pub timestamp: u64,
    pub idx: String,
    pub slot: u64,
    pub signature: String,
    pub user: String,
    pub base_mint: String,
    pub quote_mint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_token: Option<TokenInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_token: Option<TokenInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decimals: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_supply: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fee: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol_fee: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform_fee: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub share_fee: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creator_fee: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform_config: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creator: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bonding_curve: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool_dex: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool_a_reserve: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool_b_reserve: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool_fee_rate: Option<f64>,
}

/// Additional context information about the parsed transaction.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DexInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub program_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amm: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route: Option<String>,
}

/// Aggregated parsing result returned by the Rust parser.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ParseResult {
    pub state: bool,
    #[serde(default)]
    pub fee: TokenAmount,
    #[serde(default)]
    pub aggregate_trade: Option<TradeInfo>,
    #[serde(default)]
    pub trades: Vec<TradeInfo>,
    #[serde(default)]
    pub liquidities: Vec<PoolEvent>,
    #[serde(default)]
    pub transfers: Vec<TransferData>,
    #[serde(default)]
    pub sol_balance_change: Option<BalanceChange>,
    #[serde(default)]
    pub token_balance_change: HashMap<String, BalanceChange>,
    #[serde(default)]
    pub meme_events: Vec<MemeEvent>,
    #[serde(default)]
    pub slot: u64,
    #[serde(default)]
    pub timestamp: u64,
    #[serde(default)]
    pub signature: String,
    #[serde(default)]
    pub signer: Vec<String>,
    #[serde(default)]
    pub compute_units: u64,
    #[serde(default)]
    pub tx_status: TransactionStatus,
    #[serde(default)]
    pub msg: Option<String>,
}

impl ParseResult {
    pub fn new() -> Self {
        Self {
            state: true,
            fee: TokenAmount::default(),
            aggregate_trade: None,
            trades: Vec::new(),
            liquidities: Vec::new(),
            transfers: Vec::new(),
            sol_balance_change: None,
            token_balance_change: HashMap::new(),
            meme_events: Vec::new(),
            slot: 0,
            timestamp: 0,
            signature: String::new(),
            signer: Vec::new(),
            compute_units: 0,
            tx_status: TransactionStatus::default(),
            msg: None,
        }
    }
}

impl Default for ParseResult {
    fn default() -> Self {
        Self::new()
    }
}

/// Minimal instruction representation with bookkeeping indices.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ClassifiedInstruction {
    pub program_id: String,
    pub outer_index: usize,
    pub inner_index: Option<usize>,
    pub data: SolanaInstruction,
}

/// Basic representation of a Solana instruction.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SolanaInstruction {
    pub program_id: String,
    pub accounts: Vec<String>,
    #[serde(default)]
    pub data: String,
}

/// Inner instruction grouping mirroring the Solana RPC payload.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InnerInstruction {
    pub index: usize,
    #[serde(default)]
    pub instructions: Vec<SolanaInstruction>,
}

/// Transaction meta information used by the adapter.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TransactionMeta {
    pub fee: u64,
    pub compute_units: u64,
    pub status: TransactionStatus,
    #[serde(default)]
    pub sol_balance_changes: HashMap<String, BalanceChange>,
    #[serde(default)]
    pub token_balance_changes: HashMap<String, HashMap<String, BalanceChange>>,
}

/// Simplified transaction representation consumed by the parser.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SolanaTransaction {
    pub slot: u64,
    pub signature: String,
    pub block_time: u64,
    #[serde(default)]
    pub signers: Vec<String>,
    #[serde(default)]
    pub instructions: Vec<SolanaInstruction>,
    #[serde(default)]
    pub inner_instructions: Vec<InnerInstruction>,
    #[serde(default)]
    pub transfers: Vec<TransferData>,
    #[serde(default)]
    pub pre_token_balances: Vec<TokenBalance>,
    #[serde(default)]
    pub post_token_balances: Vec<TokenBalance>,
    #[serde(default)]
    pub meta: TransactionMeta,
}

/// Block representation for CLI parsing.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SolanaBlock {
    pub slot: u64,
    #[serde(default)]
    pub block_time: Option<u64>,
    #[serde(default)]
    pub transactions: Vec<SolanaTransaction>,
}

/// Input wrapper for CLI block parsing distinguishing between raw and parsed data.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BlockInput {
    Raw {
        transactions: Vec<serde_json::Value>,
    },
    Parsed {
        block: SolanaBlock,
    },
}

/// Wrapper returned by `parse_block` helper functions.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BlockParseResult {
    pub slot: u64,
    #[serde(default)]
    pub timestamp: Option<u64>,
    pub transactions: Vec<ParseResult>,
}

/// Convenience alias used by parsers.
pub type TransferMap = HashMap<String, Vec<TransferData>>;

/// Convenience alias used by parsers.
pub type InstructionList = Vec<ClassifiedInstruction>;

/// Helper trait for converting from raw JSON transactions.
pub trait FromJsonValue {
    /// Parse from JSON Value (for compatibility)
    fn from_value(value: &serde_json::Value, config: &ParseConfig) -> Result<SolanaTransaction>;
    
    /// Parse from bytes (faster, no string copy)
    #[inline(always)]
    fn from_slice(bytes: &[u8], _config: &ParseConfig) -> Result<SolanaTransaction> {
        serde_json::from_slice(bytes)
            .map_err(|err| anyhow!("failed to deserialize transaction from bytes: {err}"))
    }
}

impl FromJsonValue for SolanaTransaction {
    /// Optimized: deserialize from Value reference (avoids clone when possible)
    fn from_value(value: &serde_json::Value, _config: &ParseConfig) -> Result<SolanaTransaction> {
        // Use Deserializer directly to avoid clone - deserialize from reference
        use serde::de::Deserialize;
        SolanaTransaction::deserialize(value)
            .map_err(|err| anyhow!("failed to deserialize transaction: {err}"))
    }
}
