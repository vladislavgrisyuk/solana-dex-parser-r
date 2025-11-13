//! Zero-copy parser for Solana transaction message format
//! 
//! This module provides zero-copy parsing of Solana message format directly from raw bytes.
//! All structures use borrowed references to avoid unnecessary allocations.
//!
//! Solana Message Format:
//! - Header (3 bytes): num_required_signatures, num_readonly_signed_accounts, num_readonly_unsigned_accounts
//! - Account keys: compact-u16 length + N * 32 bytes
//! - Recent blockhash: 32 bytes
//! - Instructions: compact-u16 length + instruction data
//!
//! Instruction Format:
//! - program_id_index: u8
//! - accounts: compact-u16 length + Vec<u8> (account indices)
//! - data: compact-u16 length + Vec<u8> (instruction data)

use std::fmt;
use arrayref::array_ref;
use bs58;
use serde_json;
use base64_simd;

/// Zero-copy message header (3 bytes)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZcMessageHeader {
    pub num_required_signatures: u8,
    pub num_readonly_signed_accounts: u8,
    pub num_readonly_unsigned_accounts: u8,
}

impl ZcMessageHeader {
    /// Parse header from 3 bytes
    #[inline(always)]
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ParseError> {
        if bytes.len() < 3 {
            return Err(ParseError::InsufficientData);
        }
        Ok(Self {
            num_required_signatures: bytes[0],
            num_readonly_signed_accounts: bytes[1],
            num_readonly_unsigned_accounts: bytes[2],
        })
    }
}

/// Zero-copy instruction that references the original buffer
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZcInstruction<'a> {
    /// Program ID index (u8)
    pub program_id_index: u8,
    /// Account indices slice (references original buffer)
    pub accounts: &'a [u8],
    /// Instruction data slice (references original buffer)
    pub data: &'a [u8],
    /// Offset in the original message buffer where this instruction starts
    pub offset: usize,
}

impl<'a> ZcInstruction<'a> {
    /// Get the length of this instruction in bytes (for debugging/validation)
    pub fn len_bytes(&self) -> usize {
        1 + // program_id_index
        compact_u16_len(self.accounts.len() as u16) +
        self.accounts.len() +
        compact_u16_len(self.data.len() as u16) +
        self.data.len()
    }
}

/// Zero-copy message that references the original buffer
pub struct ZcMessage<'a> {
    /// Original buffer (must be kept alive)
    buffer: &'a [u8],
    /// Message header
    pub header: ZcMessageHeader,
    /// Account keys slice (32 bytes each, references buffer)
    /// Access via get_account_key() for safe indexing
    account_keys_slice: &'a [u8],
    /// Number of account keys
    account_keys_count: usize,
    /// Recent blockhash (references buffer)
    pub recent_blockhash: &'a [u8; 32],
    /// Instructions (references buffer)
    pub instructions: Vec<ZcInstruction<'a>>,
    /// Start offset of message in buffer (after signatures)
    message_start: usize,
    /// End offset of message in buffer
    message_end: usize,
}

/// Parse signatures from transaction bytes
/// Returns (signatures, message_start_offset)
pub fn parse_signatures(buffer: &[u8]) -> Result<(usize, usize), ParseError> {
    let mut pos = 0;
    
    // Parse number of signatures (compact-u16)
    let (num_sigs, sig_len_size) = read_compact_u16(&buffer[pos..])?;
    pos += sig_len_size;
    
    // Each signature is 64 bytes
    let sig_bytes = num_sigs as usize * 64;
    if pos + sig_bytes > buffer.len() {
        return Err(ParseError::InsufficientData);
    }
    
    // Skip signatures (we don't need to store them in zero-copy message)
    let message_start = pos + sig_bytes;
    
    Ok((num_sigs as usize, message_start))
}

impl<'a> ZcMessage<'a> {
    /// Parse message from raw transaction bytes (after signatures)
    /// 
    /// # Arguments
    /// * `buffer` - Full transaction buffer
    /// * `message_start` - Offset where message starts (after signatures)
    /// 
    /// # Returns
    /// Parsed message with zero-copy references to buffer
    pub fn parse(buffer: &'a [u8], message_start: usize) -> Result<Self, ParseError> {
        let mut pos = message_start;
        
        // Check if we have enough data for version byte
        if pos >= buffer.len() {
            return Err(ParseError::InsufficientData);
        }
        
        // Check for versioned transaction (v0)
        let is_versioned = (buffer[pos] & 0x80) != 0;
        if is_versioned {
            pos += 1;
        }
        
        // Parse header (3 bytes)
        if pos + 3 > buffer.len() {
            return Err(ParseError::InsufficientData);
        }
        let header = ZcMessageHeader::from_bytes(&buffer[pos..pos + 3])?;
        pos += 3;
        
        // Parse account keys
        let (num_accounts, acc_len_size) = read_compact_u16(&buffer[pos..])?;
        pos += acc_len_size;
        
        let keys_bytes = num_accounts as usize * 32;
        if pos + keys_bytes > buffer.len() {
            return Err(ParseError::InsufficientData);
        }
        
        // Account keys: store as slice, use chunks_exact for safe access
        let account_keys_slice = &buffer[pos..pos + keys_bytes];
        pos += keys_bytes;
        
        // Parse recent blockhash (32 bytes)
        if pos + 32 > buffer.len() {
            return Err(ParseError::InsufficientData);
        }
        // Use array reference safely: we've already checked bounds
        let recent_blockhash = array_ref!(buffer, pos, 32);
        pos += 32;
        
        // Parse instructions
        let (num_instructions, ix_len_size) = read_compact_u16(&buffer[pos..])?;
        pos += ix_len_size;
        
        let mut instructions = Vec::with_capacity(num_instructions as usize);
        
        for _ in 0..num_instructions {
            if pos >= buffer.len() {
                return Err(ParseError::InsufficientData);
            }
            
            let instruction_start = pos;
            let program_id_index = buffer[pos];
            pos += 1;
            
            // Parse accounts
            let (acc_count, acc_len_size) = read_compact_u16(&buffer[pos..])?;
            pos += acc_len_size;
            if pos + acc_count as usize > buffer.len() {
                return Err(ParseError::InsufficientData);
            }
            let accounts = &buffer[pos..pos + acc_count as usize];
            pos += acc_count as usize;
            
            // Parse data
            let (data_len, data_len_size) = read_compact_u16(&buffer[pos..])?;
            pos += data_len_size;
            if pos + data_len as usize > buffer.len() {
                return Err(ParseError::InsufficientData);
            }
            let data = &buffer[pos..pos + data_len as usize];
            pos += data_len as usize;
            
            instructions.push(ZcInstruction {
                program_id_index,
                accounts,
                data,
                offset: instruction_start,
            });
        }
        
        // For v0 transactions, there might be address lookup tables after instructions
        // We skip them for now as they're handled separately via meta.loadedAddresses
        // The message_end is after instructions (before ALT if present)
        
        Ok(Self {
            buffer,
            header,
            account_keys_slice,
            account_keys_count: num_accounts as usize,
            recent_blockhash,
            instructions,
            message_start,
            message_end: pos,
        })
    }
    
    /// Get account key by index (safe, no unsafe)
    #[inline(always)]
    pub fn get_account_key(&self, index: usize) -> Option<&[u8; 32]> {
        if index >= self.account_keys_count {
            return None;
        }
        let offset = index * 32;
        if offset + 32 > self.account_keys_slice.len() {
            return None;
        }
        // Safe: we've checked bounds, and account keys are always 32 bytes
        Some(array_ref!(self.account_keys_slice, offset, 32))
    }
    
    /// Get program ID by instruction index
    #[inline(always)]
    pub fn get_program_id(&self, instruction: &ZcInstruction) -> Option<&[u8; 32]> {
        self.get_account_key(instruction.program_id_index as usize)
    }
    
    /// Get program ID as base58 string (for convenience)
    pub fn get_program_id_string(&self, instruction: &ZcInstruction) -> Option<String> {
        self.get_program_id(instruction)
            .map(|key| bs58::encode(key).into_string())
    }
    
    /// Get account keys for an instruction
    pub fn get_instruction_accounts(&self, instruction: &ZcInstruction) -> Vec<&[u8; 32]> {
        instruction.accounts
            .iter()
            .filter_map(|&idx| self.get_account_key(idx as usize))
            .collect()
    }
    
    /// Get the number of account keys
    #[inline(always)]
    pub fn account_keys_len(&self) -> usize {
        self.account_keys_count
    }
    
    /// Get all account keys as iterator
    pub fn account_keys_iter(&self) -> impl Iterator<Item = &[u8; 32]> + '_ {
        (0..self.account_keys_count)
            .filter_map(move |i| self.get_account_key(i))
    }
    
    /// Get the number of instructions
    #[inline(always)]
    pub fn instructions_len(&self) -> usize {
        self.instructions.len()
    }
}

impl<'a> fmt::Debug for ZcMessage<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ZcMessage")
            .field("header", &self.header)
            .field("account_keys_len", &self.account_keys_count)
            .field("recent_blockhash", &hex::encode(self.recent_blockhash))
            .field("instructions_len", &self.instructions.len())
            .field("message_start", &self.message_start)
            .field("message_end", &self.message_end)
            .finish()
    }
}

/// Parse error for zero-copy parsing
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    InsufficientData,
    InvalidCompactU16,
    InvalidHeader,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::InsufficientData => write!(f, "Insufficient data"),
            ParseError::InvalidCompactU16 => write!(f, "Invalid compact-u16 encoding"),
            ParseError::InvalidHeader => write!(f, "Invalid message header"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Read compact-u16 from buffer
/// Returns (value, bytes_read)
#[inline(always)]
fn read_compact_u16(data: &[u8]) -> Result<(u16, usize), ParseError> {
    if data.is_empty() {
        return Err(ParseError::InsufficientData);
    }
    let b0 = data[0];
    if b0 <= 0x7f {
        Ok((b0 as u16, 1))
    } else if b0 <= 0xbf {
        if data.len() < 2 {
            return Err(ParseError::InsufficientData);
        }
        let v = (((b0 & 0x3f) as u16) << 8) | data[1] as u16;
        Ok((v, 2))
    } else {
        if data.len() < 3 {
            return Err(ParseError::InsufficientData);
        }
        let v = ((((b0 & 0x1f) as u32) << 16) | ((data[1] as u32) << 8) | data[2] as u32) as u16;
        Ok((v, 3))
    }
}

/// Get the length of compact-u16 encoding for a value
#[inline(always)]
fn compact_u16_len(value: u16) -> usize {
    if value <= 0x7f {
        1
    } else if value <= 0x3fff {
        2
    } else {
        3
    }
}

/// Zero-copy transaction that references the original buffer
/// 
/// This structure combines zero-copy message parsing with meta data from JSON.
/// The message is parsed directly from raw bytes (zero-copy), while meta data
/// is deserialized from JSON (owned, but minimized copying).
pub struct ZcTransaction<'a> {
    /// Original buffer (must be kept alive)
    buffer: &'a [u8],
    /// Zero-copy message
    pub message: ZcMessage<'a>,
    /// Loaded addresses from ALT (v0 transactions, owned as they come from JSON)
    pub loaded_addresses: Vec<[u8; 32]>,
    /// Slot number
    pub slot: u64,
    /// Transaction signature (owned, needed for output)
    pub signature: String,
    /// Block time
    pub block_time: u64,
}

impl<'a> ZcTransaction<'a> {
    /// Parse transaction from raw bytes and meta JSON
    /// 
    /// # Arguments
    /// * `buffer` - Raw transaction bytes
    /// * `slot` - Slot number
    /// * `signature` - Transaction signature (base58 string)
    /// * `block_time` - Block time
    /// * `meta_json` - Optional meta JSON (from RPC response)
    /// 
    /// # Returns
    /// Parsed transaction with zero-copy message and meta data
    pub fn parse(
        buffer: &'a [u8],
        slot: u64,
        signature: &str,
        block_time: u64,
        meta_json: Option<&serde_json::Value>,
    ) -> Result<Self, ParseError> {
        // Parse signatures to find message start
        let (_num_sigs, message_start) = parse_signatures(buffer)?;
        
        // Parse message (zero-copy)
        let message = ZcMessage::parse(buffer, message_start)?;
        
        // Extract loaded addresses from ALT (v0 transactions)
        let loaded_addresses = if let Some(meta) = meta_json {
            extract_loaded_addresses(meta)?
        } else {
            Vec::new()
        };
        
        Ok(Self {
            buffer,
            message,
            loaded_addresses,
            slot,
            signature: signature.to_string(), // Owned: needed for output
            block_time,
        })
    }
    
    /// Get signers (first N account keys where N = num_required_signatures)
    /// Returns base58-encoded signer addresses
    pub fn get_signers(&self) -> Vec<String> {
        let num_signatures = self.message.header.num_required_signatures as usize;
        (0..num_signatures.min(self.message.account_keys_len()))
            .filter_map(|i| {
                self.message.get_account_key(i)
                    .map(|key| bs58::encode(key).into_string())
            })
            .collect()
    }
    
    /// Get all account keys (static + loaded from ALT)
    /// Returns base58-encoded account addresses
    pub fn get_all_account_keys(&self) -> Vec<String> {
        let mut keys = Vec::with_capacity(self.message.account_keys_len() + self.loaded_addresses.len());
        
        // Static account keys
        for i in 0..self.message.account_keys_len() {
            if let Some(key) = self.message.get_account_key(i) {
                keys.push(bs58::encode(key).into_string());
            }
        }
        
        // Loaded addresses from ALT
        for key in &self.loaded_addresses {
            keys.push(bs58::encode(key).into_string());
        }
        
        keys
    }
    
    /// Convert zero-copy instruction to SolanaInstruction (for backward compatibility)
    /// NOTE: This creates owned copies, so it's not fully zero-copy
    /// For maximum performance, use ZcInstruction directly
    pub fn instruction_to_solana_instruction(
        &self,
        zc_ix: &ZcInstruction,
    ) -> Option<crate::types::SolanaInstruction> {
        // Get program ID
        let program_id_key = self.message.get_program_id(zc_ix)?;
        let program_id = bs58::encode(program_id_key).into_string();
        
        // Get account keys (lazy: only convert when needed)
        let all_keys = self.get_all_account_keys();
        let accounts: Vec<String> = zc_ix.accounts
            .iter()
            .filter_map(|&idx| {
                all_keys.get(idx as usize).cloned()
            })
            .collect();
        
        // Encode instruction data as base64
        let data_base64 = base64_simd::STANDARD.encode_to_string(zc_ix.data);
        
        Some(crate::types::SolanaInstruction {
            program_id,
            accounts,
            data: data_base64,
        })
    }
    
    /// Get all instructions as SolanaInstruction (for backward compatibility)
    /// NOTE: This creates owned copies, so it's not fully zero-copy
    pub fn get_instructions(&self) -> Vec<crate::types::SolanaInstruction> {
        self.message.instructions
            .iter()
            .filter_map(|ix| self.instruction_to_solana_instruction(ix))
            .collect()
    }
    
    /// Get instruction by index as SolanaInstruction (for backward compatibility)
    pub fn get_instruction(&self, index: usize) -> Option<crate::types::SolanaInstruction> {
        self.message.instructions
            .get(index)
            .and_then(|ix| self.instruction_to_solana_instruction(ix))
    }
}

impl<'a> fmt::Debug for ZcTransaction<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ZcTransaction")
            .field("slot", &self.slot)
            .field("signature", &self.signature)
            .field("block_time", &self.block_time)
            .field("message", &self.message)
            .field("loaded_addresses_len", &self.loaded_addresses.len())
            .finish()
    }
}

/// Extract loaded addresses from ALT (v0 transactions)
fn extract_loaded_addresses(meta: &serde_json::Value) -> Result<Vec<[u8; 32]>, ParseError> {
    let mut addresses = Vec::new();
    
    if let Some(loaded) = meta.pointer("/loadedAddresses") {
        // Writable addresses
        if let Some(writable) = loaded.get("writable").and_then(|v| v.as_array()) {
            for addr in writable {
                if let Some(s) = addr.as_str() {
                    if let Ok(decoded) = bs58::decode(s).into_vec() {
                        if decoded.len() == 32 {
                            let mut key = [0u8; 32];
                            key.copy_from_slice(&decoded);
                            addresses.push(key);
                        }
                    }
                }
            }
        }
        
        // Readonly addresses
        if let Some(readonly) = loaded.get("readonly").and_then(|v| v.as_array()) {
            for addr in readonly {
                if let Some(s) = addr.as_str() {
                    if let Ok(decoded) = bs58::decode(s).into_vec() {
                        if decoded.len() == 32 {
                            let mut key = [0u8; 32];
                            key.copy_from_slice(&decoded);
                            addresses.push(key);
                        }
                    }
                }
            }
        }
    }
    
    Ok(addresses)
}

/// Convert ZcTransaction to SolanaTransaction (for backward compatibility)
/// 
/// NOTE: This function creates owned copies of all data, so it's not zero-copy.
/// This is only for backward compatibility with existing code that uses SolanaTransaction.
/// For maximum performance, use ZcTransaction directly.
pub fn convert_zc_to_solana_tx(
    zc_tx: &ZcTransaction,
    meta_json: Option<&serde_json::Value>,
) -> Result<crate::types::SolanaTransaction, ParseError> {
    use crate::types::{
        SolanaInstruction, SolanaTransaction,
        TransactionMeta, TransactionStatus,
    };
    use std::collections::HashMap;
    
    // OPTIMIZATION: Get all account keys once and reuse (cached)
    let all_account_keys = zc_tx.get_all_account_keys();
    let account_keys_len = all_account_keys.len();
    
    // OPTIMIZATION: Pre-allocate instructions vector with known capacity
    let instructions_capacity = zc_tx.message.instructions.len();
    let mut instructions: Vec<SolanaInstruction> = Vec::with_capacity(instructions_capacity);
    
    // OPTIMIZATION: Convert instructions with minimal allocations
    for ix in &zc_tx.message.instructions {
        // Get program ID (cached lookup)
        let program_id = if (ix.program_id_index as usize) < account_keys_len {
            all_account_keys[ix.program_id_index as usize].clone()
        } else {
            "".to_string()
        };
        
        // OPTIMIZATION: Pre-allocate accounts vector with known capacity
        let accounts_capacity = ix.accounts.len().min(account_keys_len);
        let mut accounts: Vec<String> = Vec::with_capacity(accounts_capacity);
        for &idx in ix.accounts.iter() {
            if (idx as usize) < account_keys_len {
                accounts.push(all_account_keys[idx as usize].clone());
            }
        }
        
        // OPTIMIZATION: Encode instruction data as base64 (fast SIMD encoding)
        let data_base64 = base64_simd::STANDARD.encode_to_string(ix.data);
        
        instructions.push(SolanaInstruction {
            program_id,
            accounts,
            data: data_base64,
        });
    }
    
    // Extract inner instructions from meta if present
    let inner_instructions = if let Some(meta_val) = meta_json {
        extract_inner_instructions_from_meta(meta_val, &all_account_keys)
    } else {
        Vec::new()
    };
    
    // Extract token balances from meta if present
    let (pre_token_balances, post_token_balances) = if let Some(meta_val) = meta_json {
        let pre = extract_token_balances_from_meta(meta_val.pointer("/preTokenBalances"), &all_account_keys);
        let post = extract_token_balances_from_meta(meta_val.pointer("/postTokenBalances"), &all_account_keys);
        (pre, post)
    } else {
        (Vec::new(), Vec::new())
    };
    
    // Extract transaction meta
    let tx_meta = if let Some(meta_val) = meta_json {
        extract_transaction_meta_from_json(meta_val, &all_account_keys)
    } else {
        TransactionMeta {
            fee: 0,
            compute_units: 0,
            status: TransactionStatus::Success,
            sol_balance_changes: HashMap::new(),
            token_balance_changes: HashMap::new(),
        }
    };
    
    Ok(SolanaTransaction {
        slot: zc_tx.slot,
        signature: zc_tx.signature.clone(),
        block_time: zc_tx.block_time,
        signers: zc_tx.get_signers(),
        instructions,
        inner_instructions,
        transfers: Vec::new(), // Will be populated by DexParser
        pre_token_balances,
        post_token_balances,
        meta: tx_meta,
    })
}

/// Extract inner instructions from meta JSON
fn extract_inner_instructions_from_meta(
    meta: &serde_json::Value,
    account_keys: &[String],
) -> Vec<crate::types::InnerInstruction> {
    use crate::types::{InnerInstruction, SolanaInstruction};
    use base64_simd::STANDARD as B64;
    
    let mut result = Vec::new();
    
    if let Some(inner_arr) = meta.get("innerInstructions").and_then(|v| v.as_array()) {
        for group in inner_arr {
            let index = group.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            
            let mut instructions = Vec::new();
            if let Some(ixs) = group.get("instructions").and_then(|v| v.as_array()) {
                for ix_val in ixs {
                    let program_id = ix_val
                        .get("programId")
                        .and_then(|v| v.as_str())
                        .or_else(|| {
                            ix_val
                                .get("programIdIndex")
                                .and_then(|idx| idx.as_u64())
                                .and_then(|idx| account_keys.get(idx as usize))
                                .map(|s| s.as_str())
                        })
                        .unwrap_or("")
                        .to_string();
                    
                    let accounts: Vec<String> = if let Some(acc_arr) =
                        ix_val.get("accounts").and_then(|v| v.as_array())
                    {
                        acc_arr
                            .iter()
                            .filter_map(|v| {
                                if let Some(s) = v.as_str() {
                                    Some(s.to_string())
                                } else if let Some(idx) = v.as_u64() {
                                    account_keys.get(idx as usize).cloned()
                                } else {
                                    None
                                }
                            })
                            .collect()
                    } else {
                        Vec::new()
                    };
                    
                    // Data might be base58 or base64 - encode as base64 for consistency
                    let data = ix_val
                        .get("data")
                        .and_then(|v| v.as_str())
                        .map(|s| {
                            // If it's base58, decode and re-encode as base64
                            if let Ok(bytes) = bs58::decode(s).into_vec() {
                                B64.encode_to_string(&bytes)
                            } else {
                                // Assume it's already base64 or empty
                                s.to_string()
                            }
                        })
                        .unwrap_or_default();
                    
                    instructions.push(SolanaInstruction {
                        program_id,
                        accounts,
                        data,
                    });
                }
            }
            
            if !instructions.is_empty() {
                result.push(InnerInstruction {
                    index,
                    instructions,
                });
            }
        }
    }
    
    result
}

/// Extract token balances from meta JSON
fn extract_token_balances_from_meta(
    meta_opt: Option<&serde_json::Value>,
    account_keys: &[String],
) -> Vec<crate::types::TokenBalance> {
    use crate::types::{TokenAmount, TokenBalance};
    
    let mut result = Vec::new();
    
    if let Some(balances) = meta_opt.and_then(|v| v.as_array()) {
        for bal_val in balances {
            let account = bal_val
                .get("accountIndex")
                .and_then(|v| v.as_u64())
                .and_then(|idx| account_keys.get(idx as usize))
                .cloned()
                .or_else(|| {
                    bal_val
                        .get("account")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                })
                .or_else(|| {
                    bal_val
                        .get("account")
                        .and_then(|v| v.as_u64())
                        .and_then(|idx| account_keys.get(idx as usize))
                        .cloned()
                })
                .unwrap_or_else(|| "".to_string());
            
            let mint = bal_val
                .get("mint")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            
            let owner = bal_val
                .get("owner")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            
            let ui_amount = bal_val
                .get("uiTokenAmount")
                .and_then(|v| {
                    let amount = v.get("amount").and_then(|a| a.as_str()).unwrap_or("0");
                    let decimals = v.get("decimals").and_then(|d| d.as_u64()).unwrap_or(0) as u8;
                    let ui_amount = v.get("uiAmount").and_then(|u| u.as_f64());
                    Some(TokenAmount::new(amount, decimals, ui_amount))
                })
                .unwrap_or_default();
            
            result.push(TokenBalance {
                account,
                mint,
                owner,
                ui_token_amount: ui_amount,
            });
        }
    }
    
    result
}

/// Extract transaction meta from JSON
fn extract_transaction_meta_from_json(
    meta: &serde_json::Value,
    account_keys: &[String],
) -> crate::types::TransactionMeta {
    use crate::types::{TransactionMeta, TransactionStatus};
    use std::collections::HashMap;
    
    let fee = meta.get("fee").and_then(|v| v.as_u64()).unwrap_or(0);
    
    let compute_units = meta
        .get("computeUnitsConsumed")
        .or_else(|| meta.get("computeUnits"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    
    // Check status: if err exists and is not null, then Failed
    let status = if let Some(err_val) = meta.get("err") {
        if err_val.is_null() {
            TransactionStatus::Success
        } else {
            TransactionStatus::Failed
        }
    } else {
        TransactionStatus::Success
    };
    
    let sol_balance_changes = extract_sol_balance_changes_from_json(meta, account_keys);
    
    TransactionMeta {
        fee,
        compute_units,
        status,
        sol_balance_changes,
        token_balance_changes: HashMap::new(), // Will be populated by DexParser
    }
}

/// Extract SOL balance changes from JSON
fn extract_sol_balance_changes_from_json(
    meta: &serde_json::Value,
    account_keys: &[String],
) -> std::collections::HashMap<String, crate::types::BalanceChange> {
    use crate::types::BalanceChange;
    use std::collections::HashMap;
    
    let mut result = HashMap::new();
    
    let pre_balances = meta.get("preBalances").and_then(|v| v.as_array());
    let post_balances = meta.get("postBalances").and_then(|v| v.as_array());
    
    if let Some(balances) = pre_balances {
        for (idx, pre_val) in balances.iter().enumerate() {
            let pre = pre_val.as_i64().unwrap_or(0) as i128;
            let post = post_balances
                .and_then(|arr| arr.get(idx))
                .and_then(|v| v.as_i64())
                .unwrap_or(0) as i128;
            
            if pre != post {
                let account = account_keys
                    .get(idx)
                    .cloned()
                    .unwrap_or_else(|| format!("unknown_{}", idx));
                
                result.insert(
                    account,
                    BalanceChange {
                        pre,
                        post,
                        change: post - pre,
                    },
                );
            }
        }
    }
    
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_read_compact_u16() {
        // Single byte
        assert_eq!(read_compact_u16(&[0x7f]).unwrap(), (0x7f, 1));
        assert_eq!(read_compact_u16(&[0x00]).unwrap(), (0x00, 1));
        
        // Two bytes
        assert_eq!(read_compact_u16(&[0x80, 0x01]).unwrap(), (0x01, 2));
        assert_eq!(read_compact_u16(&[0xbf, 0xff]).unwrap(), (0x3fff, 2));
        
        // Three bytes
        assert_eq!(read_compact_u16(&[0xc0, 0x00, 0x01]).unwrap(), (0x4000, 3));
        assert_eq!(read_compact_u16(&[0xff, 0xff, 0xff]).unwrap(), (0xffff, 3));
    }
    
    #[test]
    fn test_compact_u16_len() {
        assert_eq!(compact_u16_len(0x7f), 1);
        assert_eq!(compact_u16_len(0x3fff), 2);
        assert_eq!(compact_u16_len(0x4000), 3);
        assert_eq!(compact_u16_len(0xffff), 3);
    }
}

