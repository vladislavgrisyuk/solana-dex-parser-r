//! Helper methods for ZcAdapter to work with token balances and token info
//! 
//! This module provides helper methods for ZcAdapter that parse token balances
//! from meta JSON and provide a cached interface similar to TransactionAdapter.
//! 
//! NOTE: These methods parse JSON, but cache results to minimize allocations.

use std::collections::HashMap;

use crate::core::zc_adapter::ZcAdapter;
use crate::types::{TokenBalance, TokenInfo, TransferData};

/// Cached balance maps for ZcAdapter (parsed from meta JSON)
/// 
/// This structure caches token balances and transfers parsed from meta JSON,
/// providing a similar interface to TransactionAdapter::cached_balance_maps().
pub struct ZcCachedBalanceMaps {
    /// Post token balances map (account -> TokenBalance)
    pub post_balance_map: HashMap<String, TokenBalance>,
    /// Pre token balances map (account -> TokenBalance)
    pub pre_balance_map: HashMap<String, TokenBalance>,
    /// Transfer map (account -> TransferData)
    pub transfer_map: HashMap<String, TransferData>,
    /// Token info map (account -> TokenInfo)
    pub token_info_map: HashMap<String, TokenInfo>,
    /// Decimals map (mint -> decimals)
    pub decimals_map: HashMap<String, u8>,
}

impl ZcCachedBalanceMaps {
    /// Create cached balance maps from ZcAdapter
    /// 
    /// This method parses token balances from meta JSON and caches them
    /// for efficient lookup during trade parsing.
    pub fn from_adapter(adapter: &ZcAdapter) -> Self {
        // Parse post token balances
        let mut post_balance_map = HashMap::new();
        if let Some(post_balances) = adapter.post_token_balances() {
            if let Some(balances_array) = post_balances.as_array() {
                for balance in balances_array {
                    if let Some(token_balance) = Self::parse_token_balance(balance) {
                        post_balance_map.insert(token_balance.account.clone(), token_balance);
                    }
                }
            }
        }
        
        // Parse pre token balances
        let mut pre_balance_map = HashMap::new();
        if let Some(pre_balances) = adapter.pre_token_balances() {
            if let Some(balances_array) = pre_balances.as_array() {
                for balance in balances_array {
                    if let Some(token_balance) = Self::parse_token_balance(balance) {
                        pre_balance_map.insert(token_balance.account.clone(), token_balance);
                    }
                }
            }
        }
        
        // Create token info map from balances
        let mut token_info_map = HashMap::new();
        let mut decimals_map = HashMap::new();
        
        // Add token info from post balances
        for (account, balance) in &post_balance_map {
            let token_info = TokenInfo {
                mint: balance.mint.clone(),
                amount: balance.ui_token_amount.ui_amount.unwrap_or(0.0),
                amount_raw: balance.ui_token_amount.amount.clone(),
                decimals: balance.ui_token_amount.decimals,
                ..Default::default()
            };
            token_info_map.insert(account.clone(), token_info);
            decimals_map.insert(balance.mint.clone(), balance.ui_token_amount.decimals);
        }
        
        // Add token info from pre balances (if not already in map)
        for (account, balance) in &pre_balance_map {
            if !token_info_map.contains_key(account) {
                let token_info = TokenInfo {
                    mint: balance.mint.clone(),
                    amount: balance.ui_token_amount.ui_amount.unwrap_or(0.0),
                    amount_raw: balance.ui_token_amount.amount.clone(),
                    decimals: balance.ui_token_amount.decimals,
                    ..Default::default()
                };
                token_info_map.insert(account.clone(), token_info);
            }
            if !decimals_map.contains_key(&balance.mint) {
                decimals_map.insert(balance.mint.clone(), balance.ui_token_amount.decimals);
            }
        }
        
        // Create transfer map from transfer_actions (if provided)
        // For now, transfer_map is empty - transfers are parsed separately
        let transfer_map = HashMap::new();
        
        Self {
            post_balance_map,
            pre_balance_map,
            transfer_map,
            token_info_map,
            decimals_map,
        }
    }
    
    /// Create cached balance maps with transfer map
    /// 
    /// This method includes transfers from the transfer_actions map.
    pub fn from_adapter_with_transfers(
        adapter: &ZcAdapter,
        transfer_actions: &crate::types::TransferMap,
    ) -> Self {
        let mut cached = Self::from_adapter(adapter);
        
        // Add transfers to transfer map
        for (_, transfers) in transfer_actions {
            for transfer in transfers {
                cached.transfer_map.insert(transfer.info.source.clone(), transfer.clone());
                cached.transfer_map.insert(transfer.info.destination.clone(), transfer.clone());
            }
        }
        
        cached
    }
    
    /// Parse token balance from JSON value
    fn parse_token_balance(balance: &serde_json::Value) -> Option<TokenBalance> {
        use crate::types::TokenAmount;
        
        // Get account (try account string first, then accountIndex)
        let account = balance
            .get("account")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| {
                balance
                    .get("accountIndex")
                    .and_then(|v| v.as_u64())
                    .and_then(|idx| {
                        // TODO: Get account from account keys by index
                        // For now, convert index to string
                        Some(idx.to_string())
                    })
            })?;
        
        let mint = balance
            .get("mint")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        
        let owner = balance
            .get("owner")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        
        let ui_token_amount = balance
            .get("uiTokenAmount")
            .and_then(|v| {
                let amount = v.get("amount").and_then(|a| a.as_str()).unwrap_or("0");
                let decimals = v.get("decimals").and_then(|d| d.as_u64()).unwrap_or(0) as u8;
                let ui_amount = v.get("uiAmount").and_then(|u| u.as_f64());
                Some(TokenAmount::new(amount, decimals, ui_amount))
            })
            .unwrap_or_default();
        
        Some(TokenBalance {
            account,
            mint,
            owner,
            ui_token_amount,
        })
    }
    
    /// Get token account info by account key
    pub fn token_account_info(&self, account: &str) -> Option<&TokenInfo> {
        self.token_info_map.get(account)
    }
    
    /// Get token decimals by mint
    pub fn get_token_decimals(&self, mint: &str) -> u8 {
        self.decimals_map.get(mint).copied().unwrap_or(0)
    }
    
    /// Get post balance map with string references
    pub fn post_balance_map_ref(&self) -> HashMap<&str, &TokenBalance> {
        self.post_balance_map
            .iter()
            .map(|(k, v)| (k.as_str(), v))
            .collect()
    }
    
    /// Get pre balance map with string references
    pub fn pre_balance_map_ref(&self) -> HashMap<&str, &TokenBalance> {
        self.pre_balance_map
            .iter()
            .map(|(k, v)| (k.as_str(), v))
            .collect()
    }
    
    /// Get transfer map with string references
    pub fn transfer_map_ref(&self) -> HashMap<&str, &TransferData> {
        self.transfer_map
            .iter()
            .map(|(k, v)| (k.as_str(), v))
            .collect()
    }
}

