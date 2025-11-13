//! Zero-copy transaction adapter that works with ZcTransaction
//! 
//! This adapter provides zero-copy access to transaction data by working directly
//! with ZcTransaction and meta JSON. All data access is optimized to minimize allocations.

use std::collections::HashMap;

use crate::config::ParseConfig;
use crate::core::constants::TOKENS;
use crate::core::zero_copy::ZcTransaction;
use crate::types::{
    BalanceChange, InnerInstruction, SolanaInstruction, TokenAmount, TokenBalance, TokenInfo,
    PoolEventBase, PoolEventType, TransactionStatus, TransferData, TransferMap,
};
use bs58;
use base64_simd::STANDARD as B64;
use serde_json::Value;

/// Zero-copy transaction adapter that works with ZcTransaction
/// 
/// This adapter provides access to transaction data with minimal allocations.
/// It works directly with zero-copy message data and parses meta from JSON once.
pub struct ZcTransactionAdapter<'a> {
    zc_tx: &'a ZcTransaction<'a>,
    config: ParseConfig,
    meta_json: Option<&'a Value>,
    
    // Cached account keys (computed once)
    account_keys: Vec<String>,
    
    // Cached token maps (computed once)
    spl_token_map: HashMap<String, TokenInfo>,
    spl_decimals_map: HashMap<String, u8>,
    
    // Cached inner instructions (parsed once from meta)
    inner_instructions: Vec<InnerInstruction>,
    
    // Cached token balances (parsed once from meta)
    pre_token_balances: Vec<TokenBalance>,
    post_token_balances: Vec<TokenBalance>,
    
    // Cached transaction meta (parsed once from JSON)
    cached_meta: Option<CachedMeta>,
    
    // Cached signers (computed once)
    cached_signers: Vec<String>,
}

/// Cached transaction meta (parsed from JSON once)
struct CachedMeta {
    fee: u64,
    compute_units: u64,
    status: TransactionStatus,
    sol_balance_changes: HashMap<String, BalanceChange>,
}

impl<'a> ZcTransactionAdapter<'a> {
    /// Create new zero-copy transaction adapter
    pub fn new(
        zc_tx: &'a ZcTransaction<'a>,
        config: ParseConfig,
        meta_json: Option<&'a Value>,
    ) -> Self {
        // Cache account keys (computed once)
        let account_keys = zc_tx.get_all_account_keys();
        
        // Parse inner instructions from meta (once)
        let inner_instructions = if let Some(meta) = meta_json {
            Self::extract_inner_instructions(meta, &account_keys)
        } else {
            Vec::new()
        };
        
        // Parse token balances from meta (once)
        let (pre_token_balances, post_token_balances) = if let Some(meta) = meta_json {
            let pre = Self::extract_token_balances(meta.pointer("/preTokenBalances"), &account_keys);
            let post = Self::extract_token_balances(meta.pointer("/postTokenBalances"), &account_keys);
            (pre, post)
        } else {
            (Vec::new(), Vec::new())
        };
        
        // Extract token maps (once)
        let (spl_token_map, spl_decimals_map) = Self::extract_token_maps(
            &inner_instructions,
            &pre_token_balances,
            &post_token_balances,
            &account_keys,
        );
        
        // Parse transaction meta (once)
        let cached_meta = if let Some(meta) = meta_json {
            Some(Self::extract_transaction_meta(meta, &account_keys))
        } else {
            None
        };
        
        // Cache signers (computed once)
        let cached_signers = zc_tx.get_signers();
        
        Self {
            zc_tx,
            config,
            meta_json,
            account_keys,
            spl_token_map,
            spl_decimals_map,
            inner_instructions,
            pre_token_balances,
            post_token_balances,
            cached_meta,
            cached_signers,
        }
    }
    
    /* ----------------------- базовая информация ----------------------- */
    
    pub fn slot(&self) -> u64 {
        self.zc_tx.slot
    }
    
    pub fn block_time(&self) -> u64 {
        self.zc_tx.block_time
    }
    
    pub fn signature(&self) -> &str {
        &self.zc_tx.signature
    }
    
    pub fn signers(&self) -> &[String] {
        // NOTE: Returns cached signers (computed once)
        &self.cached_signers
    }
    
    /// Первый подписант или "" (под TS get signer)
    pub fn signer(&self) -> &str {
        self.cached_signers.first().map(|s| s.as_str()).unwrap_or("")
    }
    
    pub fn instructions(&self) -> Vec<SolanaInstruction> {
        // NOTE: This creates owned copies, but necessary for backward compatibility
        // For full zero-copy, we'd need to return &[ZcInstruction] with lifetime parameters
        self.zc_tx.get_instructions()
    }
    
    pub fn inner_instructions(&self) -> &[InnerInstruction] {
        &self.inner_instructions
    }
    
    pub fn config(&self) -> &ParseConfig {
        &self.config
    }
    
    pub fn fee(&self) -> TokenAmount {
        let fee = self.cached_meta.as_ref()
            .map(|m| m.fee)
            .unwrap_or(0);
        TokenAmount::new(fee.to_string(), 9, Some(fee as f64 / 1e9))
    }
    
    pub fn compute_units(&self) -> u64 {
        self.cached_meta.as_ref()
            .map(|m| m.compute_units)
            .unwrap_or(0)
    }
    
    pub fn tx_status(&self) -> TransactionStatus {
        self.cached_meta.as_ref()
            .map(|m| m.status)
            .unwrap_or(TransactionStatus::Success)
    }
    
    /* ----------------------- account keys ----------------------- */
    
    pub fn account_keys(&self) -> &[String] {
        &self.account_keys
    }
    
    pub fn get_account_key(&self, index: usize) -> Option<&str> {
        self.account_keys.get(index).map(|s| s.as_str())
    }
    
    pub fn get_account_index(&self, address: &str) -> Option<usize> {
        self.account_keys.iter().position(|k| k == address)
    }
    
    /* ----------------------- инструкции ----------------------- */
    
    pub fn get_instruction(&self, index: usize) -> Option<SolanaInstruction> {
        self.zc_tx.get_instruction(index)
    }
    
    pub fn get_inner_instruction(&self, outer_index: usize, inner_index: usize) -> Option<&SolanaInstruction> {
        self.inner_instructions()
            .iter()
            .find(|s| s.index == outer_index)
            .and_then(|s| s.instructions.get(inner_index))
    }
    
    pub fn get_instruction_accounts<'b>(&self, instruction: &'b SolanaInstruction) -> &'b [String] {
        &instruction.accounts
    }
    
    pub fn is_compiled_instruction(&self, _instruction: &SolanaInstruction) -> bool {
        true
    }
    
    pub fn get_instruction_type(&self, instruction: &SolanaInstruction) -> Option<String> {
        let data = crate::core::utils::get_instruction_data(instruction);
        data.first().map(|b| b.to_string())
    }
    
    pub fn get_instruction_program_id<'b>(&self, instruction: &'b SolanaInstruction) -> &'b str {
        &instruction.program_id
    }
    
    /* ----------------------- балансы ----------------------- */
    
    pub fn pre_balances(&self) -> Option<Vec<u64>> {
        if let Some(meta) = &self.cached_meta {
            let mut balances: Vec<(String, u64)> = meta.sol_balance_changes
                .iter()
                .map(|(key, change)| (key.clone(), change.pre as u64))
                .collect();
            
            balances.sort_by_key(|(key, _)| {
                self.get_account_index(key).unwrap_or(usize::MAX)
            });
            
            if balances.is_empty() {
                None
            } else {
                Some(balances.into_iter().map(|(_, bal)| bal).collect())
            }
        } else {
            None
        }
    }
    
    pub fn post_balances(&self) -> Option<Vec<u64>> {
        if let Some(meta) = &self.cached_meta {
            let mut balances: Vec<(String, u64)> = meta.sol_balance_changes
                .iter()
                .map(|(key, change)| (key.clone(), change.post as u64))
                .collect();
            
            balances.sort_by_key(|(key, _)| {
                self.get_account_index(key).unwrap_or(usize::MAX)
            });
            
            if balances.is_empty() {
                None
            } else {
                Some(balances.into_iter().map(|(_, bal)| bal).collect())
            }
        } else {
            None
        }
    }
    
    pub fn pre_token_balances(&self) -> &[TokenBalance] {
        &self.pre_token_balances
    }
    
    pub fn post_token_balances(&self) -> &[TokenBalance] {
        &self.post_token_balances
    }
    
    pub fn get_token_account_owner(&self, account_key: &str) -> Option<&str> {
        if let Some(b) = self.post_token_balances().iter().find(|b| b.account == account_key) {
            return b.owner.as_deref();
        }
        if let Some(b) = self.pre_token_balances().iter().find(|b| b.account == account_key) {
            return b.owner.as_deref();
        }
        None
    }
    
    pub fn get_account_balance(&self, account_keys: &[String]) -> Vec<Option<TokenAmount>> {
        if let Some(meta) = &self.cached_meta {
            account_keys
                .iter()
                .map(|key| {
                    let change = meta.sol_balance_changes.get(key)?;
                    let lamports = change.post as u64;
                    Some(TokenAmount::new(lamports.to_string(), 9, Some(lamports as f64 / 1e9)))
                })
                .collect()
        } else {
            vec![None; account_keys.len()]
        }
    }
    
    pub fn get_account_pre_balance(&self, account_keys: &[String]) -> Vec<Option<TokenAmount>> {
        if let Some(meta) = &self.cached_meta {
            account_keys
                .iter()
                .map(|key| {
                    let change = meta.sol_balance_changes.get(key)?;
                    let lamports = change.pre as u64;
                    Some(TokenAmount::new(lamports.to_string(), 9, Some(lamports as f64 / 1e9)))
                })
                .collect()
        } else {
            vec![None; account_keys.len()]
        }
    }
    
    pub fn get_token_account_balance(&self, account_keys: &[String]) -> Vec<Option<TokenAmount>> {
        account_keys
            .iter()
            .map(|key| {
                self.post_token_balances()
                    .iter()
                    .find(|b| &b.account == key)
                    .map(|b| b.ui_token_amount.clone())
            })
            .collect()
    }
    
    pub fn get_token_account_pre_balance(&self, account_keys: &[String]) -> Vec<Option<TokenAmount>> {
        account_keys
            .iter()
            .map(|key| {
                self.pre_token_balances()
                    .iter()
                    .find(|b| &b.account == key)
                    .map(|b| b.ui_token_amount.clone())
            })
            .collect()
    }
    
    /* ----------------------- карты токенов (как в TS) ----------------------- */
    
    pub fn spl_token_map(&self) -> &HashMap<String, TokenInfo> {
        &self.spl_token_map
    }
    
    pub fn spl_decimals_map(&self) -> &HashMap<String, u8> {
        &self.spl_decimals_map
    }
    
    pub fn get_token_decimals(&self, mint: &str) -> u8 {
        *self.spl_decimals_map.get(mint).unwrap_or(&0)
    }
    
    pub fn token_decimals(&self, mint: &str) -> Option<u8> {
        self.spl_decimals_map.get(mint).copied()
    }
    
    pub fn token_account_info(&self, account: &str) -> Option<&TokenInfo> {
        self.spl_token_map.get(account)
    }
    
    pub fn is_supported_token(&self, mint: &str) -> bool {
        TOKENS.values().iter().any(|m| *m == mint)
    }
    
    pub fn signer_sol_balance_change(&self) -> Option<BalanceChange> {
        let signer = self.signer();
        if signer.is_empty() {
            return None;
        }
        self.cached_meta.as_ref()
            .and_then(|m| m.sol_balance_changes.get(signer))
            .cloned()
    }
    
    pub fn signer_token_balance_changes(&self) -> Option<HashMap<String, BalanceChange>> {
        let signer = self.signer();
        if signer.is_empty() {
            return None;
        }
        
        let pre_balances = self.pre_token_balances();
        let post_balances = self.post_token_balances();
        let estimated_capacity = (pre_balances.len().max(post_balances.len()) / 4).max(4);
        
        let mut changes = HashMap::with_capacity(estimated_capacity);
        
        let mut pre_map: HashMap<String, i128> = HashMap::with_capacity(estimated_capacity);
        for b in pre_balances {
            if let Some(owner) = &b.owner {
                if owner == &signer && !b.mint.is_empty() {
                    if let Ok(raw) = b.ui_token_amount.amount.parse::<i128>() {
                        pre_map.insert(b.mint.clone(), raw);
                    }
                }
            }
        }
        
        for b in post_balances {
            if let Some(owner) = &b.owner {
                if owner == &signer && !b.mint.is_empty() {
                    if let Ok(post_raw) = b.ui_token_amount.amount.parse::<i128>() {
                        let mint_clone = b.mint.clone();
                        let pre_raw = pre_map.remove(&mint_clone).unwrap_or(0);
                        let diff = post_raw - pre_raw;
                        
                        if diff != 0 {
                            changes.insert(mint_clone, BalanceChange {
                                pre: pre_raw,
                                post: post_raw,
                                change: diff,
                            });
                        }
                    }
                }
            }
        }
        
        for (mint, pre_raw) in pre_map {
            if pre_raw != 0 {
                changes.insert(mint, BalanceChange {
                    pre: pre_raw,
                    post: 0,
                    change: -pre_raw,
                });
            }
        }
        
        if changes.is_empty() {
            None
        } else {
            Some(changes)
        }
    }
    
    pub fn cached_balance_maps(&self) -> (
        HashMap<&str, &TokenBalance>, 
        HashMap<&str, &TokenBalance>,
        HashMap<&str, &TransferData>
    ) {
        let post_balances = self.post_token_balances();
        let pre_balances = self.pre_token_balances();
        // NOTE: transfers are created later from instructions by TransactionUtils
        // For now, return empty HashMap for transfers
        // This is fine because transfers are created on-demand
        
        let post_capacity = post_balances.len();
        let pre_capacity = pre_balances.len();
        
        let mut post_map = HashMap::with_capacity(post_capacity);
        let mut pre_map = HashMap::with_capacity(pre_capacity);
        
        // Build maps with references to balances (lifetime tied to &self)
        for b in post_balances {
            // Safe: b.account is a String inside TokenBalance, which is owned by self
            // The reference to the string slice is valid as long as self is valid
            post_map.insert(b.account.as_str(), b);
        }
        
        for b in pre_balances {
            pre_map.insert(b.account.as_str(), b);
        }
        
        // Empty transfer map (transfers created later)
        let transfer_map = HashMap::new();
        
        (post_map, pre_map, transfer_map)
    }
    
    pub fn get_account_sol_balance_changes(&self, is_owner: bool) -> HashMap<String, BalanceChange> {
        if let Some(meta) = &self.cached_meta {
            let estimated_size = self.account_keys.len().min(meta.sol_balance_changes.len());
            let mut out = HashMap::with_capacity(estimated_size);
            
            for key in &self.account_keys {
                let account_key = if is_owner {
                    self.get_token_account_owner(key).map(|s| s.to_string()).unwrap_or_else(|| key.clone())
                } else {
                    key.clone()
                };
                
                if let Some(change) = meta.sol_balance_changes.get(&account_key) {
                    if change.change != 0 {
                        out.insert(account_key, change.clone());
                    }
                }
            }
            out
        } else {
            HashMap::new()
        }
    }
    
    pub fn get_account_token_balance_changes(&self, is_owner: bool) -> HashMap<String, HashMap<String, BalanceChange>> {
        let pre_balances = self.pre_token_balances();
        let post_balances = self.post_token_balances();
        
        let estimated_accounts = (pre_balances.len().max(post_balances.len()) / 2).max(4);
        let mut out: HashMap<String, HashMap<String, BalanceChange>> = HashMap::with_capacity(estimated_accounts);
        
        let pre_capacity = pre_balances.len();
        let mut pre_map: HashMap<(String, String), i128> = HashMap::with_capacity(pre_capacity);
        for b in pre_balances {
            if b.mint.is_empty() {
                continue;
            }
            let account = if is_owner {
                self.get_token_account_owner(&b.account).map(|s| s.to_string()).unwrap_or_else(|| b.account.clone())
            } else {
                b.account.clone()
            };
            if let Ok(raw) = b.ui_token_amount.amount.parse::<i128>() {
                pre_map.insert((account, b.mint.clone()), raw);
            }
        }
        
        let post_capacity = post_balances.len();
        let mut tmp: HashMap<(String, String), (i128, i128)> = HashMap::with_capacity(post_capacity);
        for b in post_balances {
            if b.mint.is_empty() {
                continue;
            }
            let account = if is_owner {
                self.get_token_account_owner(&b.account).map(|s| s.to_string()).unwrap_or_else(|| b.account.clone())
            } else {
                b.account.clone()
            };
            if let Ok(post_raw) = b.ui_token_amount.amount.parse::<i128>() {
                let mint_clone = b.mint.clone();
                let pre_raw = pre_map.remove(&(account.clone(), mint_clone.clone())).unwrap_or(0);
                tmp.insert((account, mint_clone), (pre_raw, post_raw));
            }
        }
        
        for ((account, mint), (pre_raw, post_raw)) in tmp {
            let diff = post_raw - pre_raw; // Fixed: was post_raw - post_raw
            if diff == 0 { continue; }
            out.entry(account).or_insert_with(|| HashMap::with_capacity(4)).insert(
                mint,
                BalanceChange { pre: pre_raw, post: post_raw, change: diff },
            );
        }
        
        out
    }
    
    /* ----------------------- transfers / transfer map ----------------------- */
    
    pub fn transfers(&self) -> &[TransferData] {
        // NOTE: Transfers are created from instructions, not from meta
        // This is handled by TransactionUtils::create_transfers_from_instructions
        // For now, return empty slice (transfers will be created later)
        &[]
    }
    
    pub fn get_transfer_actions(&self) -> TransferMap {
        // NOTE: Transfers are created from instructions
        // This is handled by TransactionUtils::create_transfers_from_instructions
        HashMap::new()
    }
    
    pub fn get_pool_event_base(&self, r#type: PoolEventType, program_id: &str) -> PoolEventBase {
        PoolEventBase {
            user: self.signer().to_string(),
            event_type: r#type,
            program_id: Some(program_id.to_string()),
            amm: Some(crate::core::utils::get_program_name(program_id).to_string()),
            slot: self.slot(),
            timestamp: self.block_time(),
            signature: self.signature().to_string(),
            idx: String::new(),
            signer: Some(self.signers().to_vec()),
        }
    }
    
    /* ----------------------- внутренние: парсинг meta ----------------------- */
    
    fn extract_inner_instructions(
        meta: &Value,
        account_keys: &[String],
    ) -> Vec<InnerInstruction> {
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
                        
                        let data = ix_val
                            .get("data")
                            .and_then(|v| v.as_str())
                            .map(|s| {
                                if let Ok(bytes) = bs58::decode(s).into_vec() {
                                    B64.encode_to_string(&bytes)
                                } else {
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
    
    fn extract_token_balances(
        meta_opt: Option<&Value>,
        account_keys: &[String],
    ) -> Vec<TokenBalance> {
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
    
    fn extract_transaction_meta(
        meta: &Value,
        account_keys: &[String],
    ) -> CachedMeta {
        use crate::types::TransactionStatus;
        use std::collections::HashMap;
        
        let fee = meta.get("fee").and_then(|v| v.as_u64()).unwrap_or(0);
        
        let compute_units = meta
            .get("computeUnitsConsumed")
            .or_else(|| meta.get("computeUnits"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        
        let status = if let Some(err_val) = meta.get("err") {
            if err_val.is_null() {
                TransactionStatus::Success
            } else {
                TransactionStatus::Failed
            }
        } else {
            TransactionStatus::Success
        };
        
        let sol_balance_changes = Self::extract_sol_balance_changes(meta, account_keys);
        
        CachedMeta {
            fee,
            compute_units,
            status,
            sol_balance_changes,
        }
    }
    
    fn extract_sol_balance_changes(
        meta: &Value,
        account_keys: &[String],
    ) -> HashMap<String, BalanceChange> {
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
    
    fn extract_token_maps(
        inner_instructions: &[InnerInstruction],
        pre_token_balances: &[TokenBalance],
        post_token_balances: &[TokenBalance],
        account_keys: &[String],
    ) -> (HashMap<String, TokenInfo>, HashMap<String, u8>) {
        use crate::core::constants::TOKENS;
        use crate::types::TokenInfo;
        use std::collections::HashMap;
        
        let estimated_capacity = inner_instructions.iter().map(|i| i.instructions.len()).sum::<usize>()
            + post_token_balances.len()
            + pre_token_balances.len();
        let mut accounts: HashMap<String, TokenInfo> = HashMap::with_capacity(estimated_capacity);
        let mut decimals: HashMap<String, u8> = HashMap::with_capacity(estimated_capacity / 2);
        
        // Extract from post balances
        for b in post_token_balances {
            if b.mint.is_empty() {
                continue;
            }
            if !b.account.is_empty() {
                accounts.entry(b.account.clone()).or_insert_with(|| Self::token_info_from_balance(b));
                decimals.entry(b.mint.clone()).or_insert(b.ui_token_amount.decimals);
            }
        }
        
        // Extract from pre balances
        for b in pre_token_balances {
            if b.mint.is_empty() {
                continue;
            }
            if !b.account.is_empty() {
                accounts.entry(b.account.clone()).or_insert_with(|| Self::token_info_from_balance(b));
                decimals.entry(b.mint.clone()).or_insert(b.ui_token_amount.decimals);
            }
        }
        
        // Extract from instructions
        Self::extract_token_from_instructions(inner_instructions, &mut accounts, &mut decimals);
        
        // Guarantee SOL exists
        accounts.entry(TOKENS.SOL.to_string()).or_insert(TokenInfo {
            mint: TOKENS.SOL.to_string(),
            amount: 0.0,
            amount_raw: "0".into(),
            decimals: 9,
            ..TokenInfo::default()
        });
        decimals.entry(TOKENS.SOL.to_string()).or_insert(9);
        
        (accounts, decimals)
    }
    
    fn extract_token_from_instructions(
        inner_instructions: &[InnerInstruction],
        accounts: &mut HashMap<String, TokenInfo>,
        decimals: &mut HashMap<String, u8>,
    ) {
        use crate::core::constants::TOKENS;
        use crate::core::utils::get_instruction_data;
        use crate::types::TokenInfo;
        
        const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
        const TOKEN_2022_PROGRAM_ID: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";
        
        const TRANSFER: u8 = 3;
        const TRANSFER_CHECKED: u8 = 12;
        const INITIALIZE_MINT: u8 = 0;
        const MINT_TO: u8 = 7;
        const MINT_TO_CHECKED: u8 = 14;
        const BURN: u8 = 8;
        const BURN_CHECKED: u8 = 15;
        const CLOSE_ACCOUNT: u8 = 9;
        
        let mut set_token_info = |source: Option<&str>, destination: Option<&str>, mint: Option<&str>, decimals_val: Option<u8>| {
            if let Some(src) = source {
                if !src.is_empty() {
                    if accounts.contains_key(src) {
                        if let (Some(m), Some(d)) = (mint, decimals_val) {
                            accounts.insert(src.to_string(), TokenInfo {
                                mint: m.to_string(),
                                amount: 0.0,
                                amount_raw: "0".to_string(),
                                decimals: d,
                                ..TokenInfo::default()
                            });
                            decimals.insert(m.to_string(), d);
                        }
                    } else {
                        let mint_str = mint.unwrap_or(TOKENS.SOL);
                        let decimals_val = decimals_val.unwrap_or(9);
                        accounts.insert(src.to_string(), TokenInfo {
                            mint: mint_str.to_string(),
                            amount: 0.0,
                            amount_raw: "0".to_string(),
                            decimals: decimals_val,
                            ..TokenInfo::default()
                        });
                        if let Some(m) = mint {
                            decimals.insert(m.to_string(), decimals_val);
                        }
                    }
                }
            }
            if let Some(dest) = destination {
                if !dest.is_empty() {
                    if accounts.contains_key(dest) {
                        if let (Some(m), Some(d)) = (mint, decimals_val) {
                            accounts.insert(dest.to_string(), TokenInfo {
                                mint: m.to_string(),
                                amount: 0.0,
                                amount_raw: "0".to_string(),
                                decimals: d,
                                ..TokenInfo::default()
                            });
                            decimals.insert(m.to_string(), d);
                        }
                    } else {
                        let mint_str = mint.unwrap_or(TOKENS.SOL);
                        let decimals_val = decimals_val.unwrap_or(9);
                        accounts.insert(dest.to_string(), TokenInfo {
                            mint: mint_str.to_string(),
                            amount: 0.0,
                            amount_raw: "0".to_string(),
                            decimals: decimals_val,
                            ..TokenInfo::default()
                        });
                        if let Some(m) = mint {
                            decimals.insert(m.to_string(), decimals_val);
                        }
                    }
                }
            }
            if let (Some(m), Some(d)) = (mint, decimals_val) {
                decimals.entry(m.to_string()).or_insert(d);
            }
        };
        
        for inner_set in inner_instructions {
            for ix in &inner_set.instructions {
                if ix.program_id != TOKEN_PROGRAM_ID && ix.program_id != TOKEN_2022_PROGRAM_ID {
                    continue;
                }
                
                let data = get_instruction_data(ix);
                if data.is_empty() {
                    continue;
                }
                
                let instruction_type = data[0];
                let accounts_vec = &ix.accounts;
                
                match instruction_type {
                    TRANSFER => {
                        if accounts_vec.len() >= 3 {
                            let source = accounts_vec.get(0);
                            let destination = accounts_vec.get(1);
                            set_token_info(
                                source.map(|s| s.as_str()),
                                destination.map(|d| d.as_str()),
                                None,
                                None,
                            );
                        }
                    }
                    TRANSFER_CHECKED => {
                        if accounts_vec.len() >= 4 {
                            let source = accounts_vec.get(0);
                            let mint = accounts_vec.get(1);
                            let destination = accounts_vec.get(2);
                            let decimals_val = if data.len() >= 10 { Some(data[9]) } else { None };
                            set_token_info(
                                source.map(|s| s.as_str()),
                                destination.map(|d| d.as_str()),
                                mint.map(|m| m.as_str()),
                                decimals_val,
                            );
                        }
                    }
                    INITIALIZE_MINT => {
                        if accounts_vec.len() >= 2 {
                            let mint = accounts_vec.get(0);
                            let destination = accounts_vec.get(1);
                            let decimals_val = if data.len() >= 2 { Some(data[1]) } else { None };
                            set_token_info(
                                None,
                                destination.map(|d| d.as_str()),
                                mint.map(|m| m.as_str()),
                                decimals_val,
                            );
                        }
                    }
                    MINT_TO => {
                        if accounts_vec.len() >= 2 {
                            let mint = accounts_vec.get(0);
                            let destination = accounts_vec.get(1);
                            set_token_info(
                                None,
                                destination.map(|d| d.as_str()),
                                mint.map(|m| m.as_str()),
                                None,
                            );
                        }
                    }
                    MINT_TO_CHECKED => {
                        if accounts_vec.len() >= 3 {
                            let mint = accounts_vec.get(0);
                            let destination = accounts_vec.get(1);
                            let decimals_val = if data.len() >= 10 { Some(data[9]) } else { None };
                            set_token_info(
                                None,
                                destination.map(|d| d.as_str()),
                                mint.map(|m| m.as_str()),
                                decimals_val,
                            );
                        }
                    }
                    BURN => {
                        if accounts_vec.len() >= 2 {
                            let source = accounts_vec.get(0);
                            let mint = accounts_vec.get(1);
                            set_token_info(
                                source.map(|s| s.as_str()),
                                None,
                                mint.map(|m| m.as_str()),
                                None,
                            );
                        }
                    }
                    BURN_CHECKED => {
                        if accounts_vec.len() >= 3 {
                            let source = accounts_vec.get(0);
                            let mint = accounts_vec.get(1);
                            let decimals_val = if data.len() >= 10 { Some(data[9]) } else { None };
                            set_token_info(
                                source.map(|s| s.as_str()),
                                None,
                                mint.map(|m| m.as_str()),
                                decimals_val,
                            );
                        }
                    }
                    CLOSE_ACCOUNT => {
                        if accounts_vec.len() >= 3 {
                            let source = accounts_vec.get(0);
                            let destination = accounts_vec.get(1);
                            set_token_info(
                                source.map(|s| s.as_str()),
                                destination.map(|d| d.as_str()),
                                None,
                                None,
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    
    fn token_info_from_balance(b: &TokenBalance) -> TokenInfo {
        let amount = b.ui_token_amount.ui_amount.unwrap_or_else(|| {
            let raw = b.ui_token_amount.amount.parse::<f64>().unwrap_or(0.0);
            if b.ui_token_amount.decimals == 0 { raw } else {
                raw / 10f64.powi(b.ui_token_amount.decimals as i32)
            }
        });
        TokenInfo {
            mint: b.mint.clone(),
            amount,
            amount_raw: b.ui_token_amount.amount.clone(),
            decimals: b.ui_token_amount.decimals,
            authority: b.owner.clone(),
            destination: Some(b.account.clone()),
            destination_owner: b.owner.clone(),
            source: Some(b.account.clone()),
            ..TokenInfo::default()
        }
    }
}


