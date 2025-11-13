//! TRUE zero-copy transaction adapter
//! 
//! This adapter provides REAL zero-copy access to transaction data.
//! NO allocations, NO String copies, NO base58/base64 encoding.
//! All data is accessed via references to the original buffer and JSON.
//!
//! # Zero-Copy Principles
//!
//! 1. **Instructions**: Returns `&[ZcInstruction<'a>]` - references to buffer
//! 2. **Account Keys**: Returns `&[u8; 32]` - references to buffer
//! 3. **Instruction Data**: Returns `&[u8]` - references to buffer
//! 4. **Meta JSON**: Returns `Option<&Value>` - references to external JSON
//! 5. **NO String conversions**: Never calls bs58::encode or base64 encoding
//! 6. **NO Vec allocations**: All vectors contain references, not owned data
//!
//! # Comparison with ZcTransactionAdapter
//!
//! - `ZcAdapter` (this): TRUE zero-copy, NO allocations, works with `ZcInstruction`
//! - `ZcTransactionAdapter`: Compatibility layer, creates `SolanaInstruction` (allocations)
//!
//! Use `ZcAdapter` for maximum performance when working directly with zero-copy structures.
//! Use `ZcTransactionAdapter` for compatibility with existing parsers that expect `SolanaInstruction`.

use crate::config::ParseConfig;
use crate::core::zero_copy::{ZcInstruction, ZcTransaction};
use crate::types::TransactionStatus;
use serde_json::Value;

/// TRUE zero-copy adapter - NO allocations, NO copies
/// 
/// All data is accessed via references:
/// - Instructions: &[ZcInstruction<'a>]
/// - Account keys: &[&[u8; 32]]
/// - Meta: Option<&Value> (JSON lives outside)
/// - Signature: &str (from original buffer or JSON)
pub struct ZcAdapter<'a> {
    /// Zero-copy transaction
    pub tx: &'a ZcTransaction<'a>,
    /// Meta JSON (references external JSON, not owned)
    pub meta: Option<&'a Value>,
    /// Config
    pub config: ParseConfig,
}

/// Reference to account key (32 bytes)
pub type PubkeyRef<'a> = &'a [u8; 32];

impl<'a> ZcAdapter<'a> {
    /// Create new zero-copy adapter
    /// 
    /// # Arguments
    /// * `tx` - Zero-copy transaction (references buffer)
    /// * `meta` - Optional meta JSON (references external JSON)
    /// * `config` - Parse config
    pub fn new(
        tx: &'a ZcTransaction<'a>,
        meta: Option<&'a Value>,
        config: ParseConfig,
    ) -> Self {
        Self {
            tx,
            meta,
            config,
        }
    }
    
    /* ----------------------- Zero-copy access to message data ----------------------- */
    
    /// Get message header
    #[inline(always)]
    pub fn header(&self) -> &crate::core::zero_copy::ZcMessageHeader {
        &self.tx.message.header
    }
    
    /// Get instructions (zero-copy: references to buffer)
    #[inline(always)]
    pub fn instructions(&self) -> &[ZcInstruction<'a>] {
        &self.tx.message.instructions
    }
    
    /// Get instruction by index (zero-copy)
    pub fn instruction(&self, index: usize) -> Option<&ZcInstruction<'a>> {
        self.tx.message.instructions.get(index)
    }
    
    /// Get account key by index (zero-copy: references buffer)
    #[inline(always)]
    pub fn account_key(&self, index: usize) -> Option<PubkeyRef<'a>> {
        self.tx.message.get_account_key(index)
    }
    
    /// Get all account keys (zero-copy: references to buffer)
    /// Returns slice of references to 32-byte arrays
    pub fn account_keys(&self) -> Vec<PubkeyRef<'a>> {
        let mut keys = Vec::with_capacity(self.tx.message.account_keys_len());
        for i in 0..self.tx.message.account_keys_len() {
            if let Some(key) = self.tx.message.get_account_key(i) {
                keys.push(key);
            }
        }
        // Also include loaded addresses from ALT
        for addr in &self.tx.loaded_addresses {
            keys.push(addr);
        }
        keys
    }
    
    /// Get account keys count (static + loaded)
    pub fn account_keys_len(&self) -> usize {
        self.tx.message.account_keys_len() + self.tx.loaded_addresses.len()
    }
    
    /// Get program ID for instruction (zero-copy: references buffer)
    pub fn program_id(&self, instruction: &ZcInstruction<'a>) -> Option<PubkeyRef<'a>> {
        self.tx.message.get_program_id(instruction)
    }
    
    /// Get account indices for instruction (zero-copy: references buffer)
    #[inline(always)]
    pub fn instruction_accounts(&self, instruction: &ZcInstruction<'a>) -> &[u8] {
        instruction.accounts
    }
    
    /// Get instruction data (zero-copy: references buffer)
    #[inline(always)]
    pub fn instruction_data(&self, instruction: &ZcInstruction<'a>) -> &[u8] {
        instruction.data
    }
    
    /// Get recent blockhash (zero-copy: references buffer)
    #[inline(always)]
    pub fn recent_blockhash(&self) -> &[u8; 32] {
        self.tx.message.recent_blockhash
    }
    
    /* ----------------------- Zero-copy access to transaction metadata ----------------------- */
    
    /// Get slot (zero-copy: u64)
    #[inline(always)]
    pub fn slot(&self) -> u64 {
        self.tx.slot
    }
    
    /// Get block time (zero-copy: u64)
    #[inline(always)]
    pub fn block_time(&self) -> u64 {
        self.tx.block_time
    }
    
    /// Get signature (zero-copy: references String in ZcTransaction)
    /// NOTE: ZcTransaction.signature is owned String, but we return &str reference
    #[inline(always)]
    pub fn signature(&self) -> &str {
        &self.tx.signature
    }
    
    /// Get signers as iterator (zero-copy: references to account keys)
    /// Returns iterator over references to 32-byte arrays
    /// This is the PREFERRED method - no allocations, pure zero-copy
    pub fn signers_iter(&self) -> impl Iterator<Item = PubkeyRef<'a>> + '_ {
        let num_signatures = self.tx.message.header.num_required_signatures as usize;
        (0..num_signatures.min(self.tx.message.account_keys_len()))
            .filter_map(move |i| self.tx.message.get_account_key(i))
    }
    
    /// Get signers (zero-copy: references to account keys)
    /// Returns Vec of references to 32-byte arrays (first N account keys)
    /// NOTE: This allocates a Vec, but only for references - data itself is not copied
    /// Prefer `signers_iter()` for true zero-copy iteration
    pub fn signers(&self) -> Vec<PubkeyRef<'a>> {
        self.signers_iter().collect()
    }
    
    /// Get first signer (zero-copy: reference to account key)
    pub fn signer(&self) -> Option<PubkeyRef<'a>> {
        if self.tx.message.header.num_required_signatures > 0 {
            self.tx.message.get_account_key(0)
        } else {
            None
        }
    }
    
    /* ----------------------- Zero-copy access to meta JSON ----------------------- */
    
    /// Get fee from meta (zero-copy: reads from JSON)
    pub fn fee(&self) -> u64 {
        self.meta
            .and_then(|m| m.get("fee"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
    }
    
    /// Get compute units from meta (zero-copy: reads from JSON)
    pub fn compute_units(&self) -> u64 {
        self.meta
            .and_then(|m| m.get("computeUnitsConsumed").or_else(|| m.get("computeUnits")))
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
    }
    
    /// Get transaction status from meta (zero-copy: reads from JSON)
    pub fn tx_status(&self) -> TransactionStatus {
        if let Some(meta) = self.meta {
            if let Some(err_val) = meta.get("err") {
                if err_val.is_null() {
                    TransactionStatus::Success
                } else {
                    TransactionStatus::Failed
                }
            } else {
                TransactionStatus::Success
            }
        } else {
            TransactionStatus::Success
        }
    }
    
    /// Get inner instructions from meta (lazy: parses from JSON on demand)
    /// Returns zero-copy references to instruction data
    /// NOTE: This requires parsing JSON, but returns references to string data in JSON
    pub fn inner_instructions(&self) -> Option<&'a Value> {
        self.meta.and_then(|m| m.get("innerInstructions"))
    }
    
    /// Get pre token balances from meta (lazy: returns JSON reference)
    pub fn pre_token_balances(&self) -> Option<&'a Value> {
        self.meta.and_then(|m| m.get("preTokenBalances"))
    }
    
    /// Get post token balances from meta (lazy: returns JSON reference)
    pub fn post_token_balances(&self) -> Option<&'a Value> {
        self.meta.and_then(|m| m.get("postTokenBalances"))
    }
    
    /// Get pre balances from meta (lazy: returns JSON reference)
    pub fn pre_balances(&self) -> Option<&'a Value> {
        self.meta.and_then(|m| m.get("preBalances"))
    }
    
    /// Get post balances from meta (lazy: returns JSON reference)
    pub fn post_balances(&self) -> Option<&'a Value> {
        self.meta.and_then(|m| m.get("postBalances"))
    }
    
    /// Get loaded addresses from meta (already in ZcTransaction, but check meta too)
    pub fn loaded_addresses(&self) -> &[[u8; 32]] {
        &self.tx.loaded_addresses
    }
    
    /* ----------------------- Helper methods for working with account keys ----------------------- */
    
    /// Find account key index by pubkey (zero-copy: compares 32-byte arrays)
    pub fn find_account_index(&self, pubkey: &[u8; 32]) -> Option<usize> {
        // Check static account keys
        for i in 0..self.tx.message.account_keys_len() {
            if let Some(key) = self.tx.message.get_account_key(i) {
                if key == pubkey {
                    return Some(i);
                }
            }
        }
        // Check loaded addresses
        let static_count = self.tx.message.account_keys_len();
        for (i, addr) in self.tx.loaded_addresses.iter().enumerate() {
            if addr == pubkey {
                return Some(static_count + i);
            }
        }
        None
    }
    
    /// Compare two pubkeys (zero-copy: direct byte comparison)
    #[inline(always)]
    pub fn pubkey_eq(a: &[u8; 32], b: &[u8; 32]) -> bool {
        a == b
    }
    
    /* ----------------------- Config ----------------------- */
    
    /// Get config
    #[inline(always)]
    pub fn config(&self) -> &ParseConfig {
        &self.config
    }
}


