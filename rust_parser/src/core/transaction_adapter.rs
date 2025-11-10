use std::collections::{HashMap, HashSet};

use crate::config::ParseConfig;
use crate::core::constants::TOKENS;
use crate::types::{
    BalanceChange, InnerInstruction, SolanaInstruction, SolanaTransaction, TokenAmount, TokenBalance, TokenInfo,
    PoolEventBase, PoolEventType, TransactionStatus, TransferData, TransferMap,
};

/// Унифицированный адаптер доступа к данным транзакции.
/// ВНИМАНИЕ: работает с уже НОРМАЛИЗОВАННЫМИ типами:
/// - SolanaInstruction: { program_id: String, accounts: Vec<String>, data: Vec<u8> }
/// - SolanaTransaction: { slot, signature, block_time, signers: Vec<String>,
///                       instructions: Vec<SolanaInstruction>,
///                       inner_instructions: Vec<InnerInstruction>,
///                       pre_token_balances, post_token_balances, meta{ pre_balances, post_balances, fee, ...},
///                       transfers: Vec<TransferData> }
#[derive(Clone, Debug)]
pub struct TransactionAdapter {
    tx: SolanaTransaction,
    config: ParseConfig,

    // Собранные ключи аккаунтов (uniq)
    account_keys: Vec<String>,

    // Карты как в TS: токен-аккаунт -> инфо, и mint -> decimals
    spl_token_map: HashMap<String, TokenInfo>,
    spl_decimals_map: HashMap<String, u8>,
}

impl TransactionAdapter {
    pub fn new(tx: SolanaTransaction, config: ParseConfig) -> Self {
        let t0 = std::time::Instant::now();
        let account_keys = Self::extract_account_keys(&tx);
        let t1 = std::time::Instant::now();
        let account_keys_time = (t1 - t0).as_secs_f64() * 1000.0;
        
        let t2 = std::time::Instant::now();
        let (spl_token_map, spl_decimals_map) = Self::extract_token_maps(&tx);
        let t3 = std::time::Instant::now();
        let token_maps_time = (t3 - t2).as_secs_f64() * 1000.0;
        
        let total_time = (t3 - t0).as_secs_f64() * 1000.0;
        tracing::info!(
            "⏱️  TransactionAdapter::new: extract_account_keys={:.3}ms, extract_token_maps={:.3}ms, total={:.3}ms",
            account_keys_time, token_maps_time, total_time
        );

        Self {
            tx,
            config,
            account_keys,
            spl_token_map,
            spl_decimals_map,
        }
    }

    /* ----------------------- базовая информация ----------------------- */

    pub fn slot(&self) -> u64 {
        self.tx.slot
    }

    pub fn block_time(&self) -> u64 {
        self.tx.block_time
    }

    pub fn signature(&self) -> &str {
        &self.tx.signature
    }

    pub fn signers(&self) -> &[String] {
        &self.tx.signers
    }

    /// Первый подписант или "" (под TS get signer)
    pub fn signer(&self) -> String {
        self.tx.signers.first().cloned().unwrap_or_default()
    }

    pub fn instructions(&self) -> &[SolanaInstruction] {
        &self.tx.instructions
    }

    pub fn inner_instructions(&self) -> &[InnerInstruction] {
        &self.tx.inner_instructions
    }

    pub fn config(&self) -> &ParseConfig {
        &self.config
    }

    pub fn fee(&self) -> TokenAmount {
        let fee = self.tx.meta.fee as u64;
        TokenAmount::new(fee.to_string(), 9, Some(fee as f64 / 1e9))
    }

    pub fn compute_units(&self) -> u64 {
        self.tx.meta.compute_units
    }

    pub fn tx_status(&self) -> TransactionStatus {
        self.tx.meta.status
    }

    /* ----------------------- account keys ----------------------- */

    /// Собираем уникальные адреса только из instructions/inner_instructions + signers
    fn extract_account_keys(tx: &SolanaTransaction) -> Vec<String> {
        let t0 = std::time::Instant::now();
        // Pre-allocate with estimated capacity
        let estimated_capacity = tx.signers.len() 
            + tx.instructions.len() * 3  // program_id + ~2 accounts per instruction
            + tx.inner_instructions.iter().map(|i| i.instructions.len() * 3).sum::<usize>();
        let mut set: HashSet<String> = HashSet::with_capacity(estimated_capacity);
        let t1 = std::time::Instant::now();

        let t2 = std::time::Instant::now();
        for s in &tx.signers {
            set.insert(s.clone());
        }
        let t3 = std::time::Instant::now();

        let t4 = std::time::Instant::now();
        for ix in &tx.instructions {
            set.insert(ix.program_id.clone());
            for a in &ix.accounts {
                set.insert(a.clone());
            }
        }
        let t5 = std::time::Instant::now();

        let t6 = std::time::Instant::now();
        for set_inner in &tx.inner_instructions {
            for ix in &set_inner.instructions {
                set.insert(ix.program_id.clone());
                for a in &ix.accounts {
                    set.insert(a.clone());
                }
            }
        }
        let t7 = std::time::Instant::now();

        let t8 = std::time::Instant::now();
        let mut out: Vec<String> = set.into_iter().collect();
        out.sort();
        let t9 = std::time::Instant::now();

        let alloc_time = (t1 - t0).as_secs_f64() * 1000.0;
        let signers_time = (t3 - t2).as_secs_f64() * 1000.0;
        let outer_time = (t5 - t4).as_secs_f64() * 1000.0;
        let inner_time = (t7 - t6).as_secs_f64() * 1000.0;
        let sort_time = (t9 - t8).as_secs_f64() * 1000.0;
        let total_time = (t9 - t0).as_secs_f64() * 1000.0;
        
        tracing::info!(
            "⏱️  extract_account_keys: alloc={:.3}ms, signers={:.3}ms ({}), outer={:.3}ms ({}), inner={:.3}ms ({}), sort={:.3}ms, total={:.3}ms",
            alloc_time,
            signers_time, tx.signers.len(),
            outer_time, tx.instructions.len(),
            inner_time, tx.inner_instructions.iter().map(|i| i.instructions.len()).sum::<usize>(),
            sort_time,
            total_time
        );

        out
    }

    pub fn account_keys(&self) -> &[String] {
        &self.account_keys
    }

    pub fn get_account_key(&self, index: usize) -> String {
        self.account_keys.get(index).cloned().unwrap_or_default()
    }

    pub fn get_account_index(&self, address: &str) -> Option<usize> {
        self.account_keys.iter().position(|k| k == address)
    }

    /* ----------------------- инструкции ----------------------- */

    /// В нормализованных типах `SolanaInstruction` уже унифицирован.
    pub fn get_instruction(&self, instruction: &SolanaInstruction) -> SolanaInstruction {
        instruction.clone()
    }

    pub fn get_inner_instruction(&self, outer_index: usize, inner_index: usize) -> Option<&SolanaInstruction> {
        self.inner_instructions()
            .iter()
            .find(|s| s.index == outer_index)
            .and_then(|s| s.instructions.get(inner_index))
    }

    pub fn get_instruction_accounts<'a>(&self, instruction: &'a SolanaInstruction) -> &'a [String] {
        &instruction.accounts
    }

    /// У нас нет parsed/compiled разделения – считаем, что инструкции «compiled»
    pub fn is_compiled_instruction(&self, _instruction: &SolanaInstruction) -> bool {
        true
    }

    /// Аналог TS getInstructionType: первый байт data → строка
    pub fn get_instruction_type(&self, instruction: &SolanaInstruction) -> Option<String> {
        let data = crate::core::utils::get_instruction_data(instruction);
        data.first().map(|b| b.to_string())
    }

    pub fn get_instruction_program_id<'a>(&self, instruction: &'a SolanaInstruction) -> &'a str {
        &instruction.program_id
    }

    /* ----------------------- балансы ----------------------- */

    /// Get pre-balances from sol_balance_changes
    pub fn pre_balances(&self) -> Option<Vec<u64>> {
        // Extract pre balances from sol_balance_changes
        let mut balances: Vec<(String, u64)> = self.tx.meta.sol_balance_changes
            .iter()
            .map(|(key, change)| (key.clone(), change.pre as u64))
            .collect();
        
        // Sort by account key index to maintain order
        balances.sort_by_key(|(key, _)| {
            self.get_account_index(key).unwrap_or(usize::MAX)
        });
        
        if balances.is_empty() {
            None
        } else {
            Some(balances.into_iter().map(|(_, bal)| bal).collect())
        }
    }

    /// Get post-balances from sol_balance_changes
    pub fn post_balances(&self) -> Option<Vec<u64>> {
        // Extract post balances from sol_balance_changes
        let mut balances: Vec<(String, u64)> = self.tx.meta.sol_balance_changes
            .iter()
            .map(|(key, change)| (key.clone(), change.post as u64))
            .collect();
        
        // Sort by account key index to maintain order
        balances.sort_by_key(|(key, _)| {
            self.get_account_index(key).unwrap_or(usize::MAX)
        });
        
        if balances.is_empty() {
            None
        } else {
            Some(balances.into_iter().map(|(_, bal)| bal).collect())
        }
    }

    pub fn pre_token_balances(&self) -> &[TokenBalance] {
        &self.tx.pre_token_balances
    }

    pub fn post_token_balances(&self) -> &[TokenBalance] {
        &self.tx.post_token_balances
    }

    /// Владелец токен-аккаунта по post/pre token balances
    pub fn get_token_account_owner(&self, account_key: &str) -> Option<String> {
        if let Some(b) = self.post_token_balances().iter().find(|b| b.account == account_key) {
            return b.owner.clone();
        }
        if let Some(b) = self.pre_token_balances().iter().find(|b| b.account == account_key) {
            return b.owner.clone();
        }
        None
    }

    pub fn get_account_balance(&self, account_keys: &[String]) -> Vec<Option<TokenAmount>> {
        account_keys
            .iter()
            .map(|key| {
                let change = self.tx.meta.sol_balance_changes.get(key)?;
                let lamports = change.post as u64;
                Some(TokenAmount::new(lamports.to_string(), 9, Some(lamports as f64 / 1e9)))
            })
            .collect()
    }

    pub fn get_account_pre_balance(&self, account_keys: &[String]) -> Vec<Option<TokenAmount>> {
        account_keys
            .iter()
            .map(|key| {
                let change = self.tx.meta.sol_balance_changes.get(key)?;
                let lamports = change.pre as u64;
                Some(TokenAmount::new(lamports.to_string(), 9, Some(lamports as f64 / 1e9)))
            })
            .collect()
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

    /// Алиас для старого кода: Option-версия
    pub fn token_decimals(&self, mint: &str) -> Option<u8> {
        self.spl_decimals_map.get(mint).copied()
    }

    /// Алиас для старого кода
    pub fn token_account_info(&self, account: &str) -> Option<&TokenInfo> {
        self.spl_token_map.get(account)
    }

    pub fn is_supported_token(&self, mint: &str) -> bool {
        TOKENS.values().iter().any(|m| *m == mint)
    }

    /// Get SOL balance change for the signer account
    pub fn signer_sol_balance_change(&self) -> Option<BalanceChange> {
        let signer = self.signer();
        let changes = self.get_account_sol_balance_changes(false);
        changes.get(&signer).cloned()
    }

    /// Get token balance changes for the signer account
    pub fn signer_token_balance_changes(&self) -> Option<HashMap<String, BalanceChange>> {
        let signer = self.signer();
        let changes = self.get_account_token_balance_changes(false);
        changes.get(&signer).cloned()
    }

    /// База события пула (аналог TS getPoolEventBase)
    pub fn get_pool_event_base(&self, r#type: PoolEventType, program_id: &str) -> PoolEventBase {
        PoolEventBase {
            user: self.signer(),
            event_type: r#type,
            program_id: Some(program_id.to_string()),
            amm: Some(crate::core::utils::get_program_name(program_id).to_string()),
            slot: self.slot(),
            timestamp: self.block_time(),
            signature: self.signature().to_string(),
            idx: String::new(), // Should be set by caller
            signer: Some(self.signers().to_vec()),
        }
    }

    /* ----------------------- balance changes: i128 ----------------------- */

    /// Полный аналог по смыслу, но под твой `BalanceChange` (pre/post/change: i128)
    pub fn get_account_sol_balance_changes(&self, is_owner: bool) -> HashMap<String, BalanceChange> {
        let mut out = HashMap::new();

        for key in &self.account_keys {
            let account_key = if is_owner {
                self.get_token_account_owner(key).unwrap_or_else(|| key.clone())
            } else {
                key.clone()
            };

            if let Some(change) = self.tx.meta.sol_balance_changes.get(&account_key) {
                if change.change != 0 {
                    out.insert(account_key, change.clone());
                }
            }
        }
        out
    }

    pub fn get_account_token_balance_changes(&self, is_owner: bool) -> HashMap<String, HashMap<String, BalanceChange>> {
        // outer: account -> (mint -> BalanceChange{i128})
        let mut out: HashMap<String, HashMap<String, BalanceChange>> = HashMap::new();

        // подготовим словарь pre по (account,mint) -> raw
        let mut pre_map: HashMap<(String, String), i128> = HashMap::new();
        for b in self.pre_token_balances() {
            let account = if is_owner {
                self.get_token_account_owner(&b.account).unwrap_or_else(|| b.account.clone())
            } else {
                b.account.clone()
            };
            if b.mint.is_empty() {
                continue;
            }
            let raw = b.ui_token_amount.amount.parse::<i128>().unwrap_or(0);
            pre_map.insert((account, b.mint.clone()), raw);
        }

        // post: обновим/посчитаем diff
        let mut tmp: HashMap<(String, String), (i128, i128)> = HashMap::new(); // (pre, post)
        for b in self.post_token_balances() {
            let account = if is_owner {
                self.get_token_account_owner(&b.account).unwrap_or_else(|| b.account.clone())
            } else {
                b.account.clone()
            };
            if b.mint.is_empty() {
                continue;
            }
            let post_raw = b.ui_token_amount.amount.parse::<i128>().unwrap_or(0);
            let pre_raw = *pre_map.get(&(account.clone(), b.mint.clone())).unwrap_or(&0);
            tmp.insert((account, b.mint.clone()), (pre_raw, post_raw));
        }

        // соберём в нужную иерархию
        for ((account, mint), (pre_raw, post_raw)) in tmp {
            let diff = post_raw - pre_raw;
            if diff == 0 { continue; }
            out.entry(account).or_default().insert(
                mint,
                BalanceChange { pre: pre_raw, post: post_raw, change: diff },
            );
        }

        out
    }

    /* ----------------------- transfers / transfer map ----------------------- */

    /// Алиас: отдать «сырые» TransferData из транзакции
    pub fn transfers(&self) -> &[TransferData] {
        &self.tx.transfers
    }

    /// Сгруппировать трансферы по program_id
    pub fn get_transfer_actions(&self) -> TransferMap {
        let start = std::time::Instant::now();
        let mut map: TransferMap = HashMap::new();
        
        let t0 = std::time::Instant::now();
        for t in &self.tx.transfers {
            map.entry(t.program_id.clone()).or_default().push(t.clone());
        }
        let t1 = std::time::Instant::now();
        
        let transfer_count: usize = map.values().map(|v| v.len()).sum();
        let duration = start.elapsed();
        tracing::debug!(
            "⏱️  get_transfer_actions: grouping={:.3}μs, total={:.3}μs, programs={}, total_transfers={}",
            (t1 - t0).as_secs_f64() * 1_000_000.0,
            duration.as_secs_f64() * 1_000_000.0,
            map.len(),
            transfer_count
        );
        
        map
    }

    /* ----------------------- внутренние: сбор карт токенов ----------------------- */

    fn extract_token_maps(tx: &SolanaTransaction) -> (HashMap<String, TokenInfo>, HashMap<String, u8>) {
        // Pre-allocate with estimated capacity
        let estimated_capacity = tx.transfers.len() 
            + tx.post_token_balances.len() 
            + tx.pre_token_balances.len()
            + tx.instructions.len() + tx.inner_instructions.iter().map(|i| i.instructions.len()).sum::<usize>();
        let mut accounts: HashMap<String, TokenInfo> = HashMap::with_capacity(estimated_capacity);
        let mut decimals: HashMap<String, u8> = HashMap::with_capacity(estimated_capacity / 2);

        let t0 = std::time::Instant::now();
        // 1) transfers
        for transfer in &tx.transfers {
            let info = &transfer.info;
            let amount = info.token_amount.ui_amount.unwrap_or_else(|| {
                info.token_amount.amount.parse::<f64>().unwrap_or(0.0)
            });

            let token_info = TokenInfo {
                mint: info.mint.clone(),
                amount,
                amount_raw: info.token_amount.amount.clone(),
                decimals: info.token_amount.decimals,
                authority: info.authority.clone(),
                destination: Some(info.destination.clone()),
                destination_owner: info.destination_owner.clone(),
                destination_balance: info.destination_balance.clone(),
                destination_pre_balance: info.destination_pre_balance.clone(),
                source: Some(info.source.clone()),
                source_balance: info.source_balance.clone(),
                source_pre_balance: info.source_pre_balance.clone(),
                destination_balance_change: None,
                source_balance_change: None,
                balance_change: info.sol_balance_change.clone(),
            };

            accounts.entry(info.source.clone()).or_insert_with(|| token_info.clone());
            accounts.entry(info.destination.clone()).or_insert_with(|| token_info.clone());
            decimals.entry(info.mint.clone()).or_insert(info.token_amount.decimals);
        }
        let t1 = std::time::Instant::now();

        // 2) post balances (as in TypeScript: extractTokenBalances)
        for b in &tx.post_token_balances {
            if b.mint.is_empty() {
                continue;
            }
            // In TypeScript: const accountKey = this.accountKeys[balance.accountIndex];
            // We already extracted account correctly in extract_token_balances using accountIndex
            if !b.account.is_empty() {
                accounts.entry(b.account.clone()).or_insert_with(|| Self::token_info_from_balance(b));
                decimals.entry(b.mint.clone()).or_insert(b.ui_token_amount.decimals);
            }
        }
        let t2 = std::time::Instant::now();

        // 3) pre balances
        for b in &tx.pre_token_balances {
            if b.mint.is_empty() {
                continue;
            }
            if !b.account.is_empty() {
                accounts.entry(b.account.clone()).or_insert_with(|| Self::token_info_from_balance(b));
                decimals.entry(b.mint.clone()).or_insert(b.ui_token_amount.decimals);
            }
        }
        let t3 = std::time::Instant::now();

        // 4) Extract from instructions (as in TypeScript: extractTokenFromInstructions)
        Self::extract_token_from_instructions(tx, &mut accounts, &mut decimals);
        let t4 = std::time::Instant::now();

        // 5) гарантируем наличие SOL
        accounts.entry(TOKENS.SOL.to_string()).or_insert(TokenInfo {
            mint: TOKENS.SOL.to_string(),
            amount: 0.0,
            amount_raw: "0".into(),
            decimals: 9,
            ..TokenInfo::default()
        });
        decimals.entry(TOKENS.SOL.to_string()).or_insert(9);
        let t5 = std::time::Instant::now();
        
        let transfers_time = (t1 - t0).as_secs_f64() * 1000.0;
        let post_balances_time = (t2 - t1).as_secs_f64() * 1000.0;
        let pre_balances_time = (t3 - t2).as_secs_f64() * 1000.0;
        let instructions_time = (t4 - t3).as_secs_f64() * 1000.0;
        let sol_time = (t5 - t4).as_secs_f64() * 1000.0;
        let total_time = (t5 - t0).as_secs_f64() * 1000.0;
        let total_instructions = tx.instructions.len() + tx.inner_instructions.iter().map(|i| i.instructions.len()).sum::<usize>();
        
        tracing::info!(
            "⏱️  extract_token_maps: transfers={:.3}ms ({}), post_balances={:.3}ms ({}), pre_balances={:.3}ms ({}), instructions={:.3}ms ({}), sol={:.3}ms, total={:.3}ms",
            transfers_time, tx.transfers.len(),
            post_balances_time, tx.post_token_balances.len(),
            pre_balances_time, tx.pre_token_balances.len(),
            instructions_time, total_instructions,
            sol_time,
            total_time,
        );

        (accounts, decimals)
    }

    /// Extract token info from instructions (as in TypeScript extractTokenFromInstructions)
    fn extract_token_from_instructions(
        tx: &SolanaTransaction,
        accounts: &mut HashMap<String, TokenInfo>,
        decimals: &mut HashMap<String, u8>,
    ) {
        let t0 = std::time::Instant::now();
        const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
        const TOKEN_2022_PROGRAM_ID: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";
        
        // SPL Token instruction types
        const TRANSFER: u8 = 3;
        const TRANSFER_CHECKED: u8 = 12;
        const INITIALIZE_MINT: u8 = 0;
        const MINT_TO: u8 = 7;
        const MINT_TO_CHECKED: u8 = 14;
        const BURN: u8 = 8;
        const BURN_CHECKED: u8 = 15;
        const CLOSE_ACCOUNT: u8 = 9;

        // Helper to set token info (as in TypeScript setTokenInfo)
        // In TypeScript: if (this.splTokenMap.has(source) && mint && decimals) { update }
        // else if (!this.splTokenMap.has(source)) { create with mint || TOKENS.SOL }
        let mut set_token_info = |source: Option<&str>, destination: Option<&str>, mint: Option<&str>, decimals_val: Option<u8>| {
            if let Some(src) = source {
                if !src.is_empty() {
                    // Check if account already exists in map
                    if accounts.contains_key(src) {
                        // If mint and decimals provided, update existing entry
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
                        // Create new entry with mint || TOKENS.SOL
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
                    // Check if account already exists in map
                    if accounts.contains_key(dest) {
                        // If mint and decimals provided, update existing entry
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
                        // Create new entry with mint || TOKENS.SOL
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
            // Store decimals for mint if provided
            if let (Some(m), Some(d)) = (mint, decimals_val) {
                decimals.entry(m.to_string()).or_insert(d);
            }
        };

        let t1 = std::time::Instant::now();
        let mut outer_processed = 0;
        // Process outer instructions
        for ix in &tx.instructions {
            if ix.program_id != TOKEN_PROGRAM_ID && ix.program_id != TOKEN_2022_PROGRAM_ID {
                continue;
            }

            let data = crate::core::utils::get_instruction_data(ix);
            if data.is_empty() {
                continue;
            }

            outer_processed += 1;
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
                        // decimals is in data[1]
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

        let t2 = std::time::Instant::now();
        let mut inner_processed = 0;
        // Process inner instructions
        for inner in &tx.inner_instructions {
            for ix in &inner.instructions {
                if ix.program_id != TOKEN_PROGRAM_ID && ix.program_id != TOKEN_2022_PROGRAM_ID {
                    continue;
                }

                let data = crate::core::utils::get_instruction_data(ix);
                if data.is_empty() {
                    continue;
                }

                inner_processed += 1;
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
        let t3 = std::time::Instant::now();
        
        let outer_time = (t2 - t1).as_secs_f64() * 1000.0;
        let inner_time = (t3 - t2).as_secs_f64() * 1000.0;
        let total_time = (t3 - t0).as_secs_f64() * 1000.0;
        
        tracing::info!(
            "⏱️  extract_token_from_instructions: outer={:.3}ms ({} processed), inner={:.3}ms ({} processed), total={:.3}ms",
            outer_time, outer_processed,
            inner_time, inner_processed,
            total_time
        );
    }

    fn token_info_from_balance(b: &TokenBalance) -> TokenInfo {
        // ui_amount может быть None → пересчитаем из amount/decimals
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
