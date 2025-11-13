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
        let account_keys = Self::extract_account_keys(&tx);
        let (spl_token_map, spl_decimals_map) = Self::extract_token_maps(&tx);

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
        // Pre-allocate with estimated capacity
        let estimated_capacity = tx.signers.len() 
            + tx.instructions.len() * 3  // program_id + ~2 accounts per instruction
            + tx.inner_instructions.iter().map(|i| i.instructions.len() * 3).sum::<usize>();
        let mut set: HashSet<String> = HashSet::with_capacity(estimated_capacity);
        for s in &tx.signers {
            set.insert(s.clone());
        }

        for ix in &tx.instructions {
            set.insert(ix.program_id.clone());
            for a in &ix.accounts {
                set.insert(a.clone());
            }
        }

        for set_inner in &tx.inner_instructions {
            for ix in &set_inner.instructions {
                set.insert(ix.program_id.clone());
                for a in &ix.accounts {
                    set.insert(a.clone());
                }
            }
        }

        // Оптимизация: предварительно резервируем capacity для Vec
        let set_size = set.len();
        let mut out = Vec::with_capacity(set_size);
        out.extend(set.into_iter());
        // Оптимизация: используем unstable sort для немного большей скорости
        out.sort_unstable();

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

    /// Get SOL balance change for the signer account (optimized: direct lookup)
    pub fn signer_sol_balance_change(&self) -> Option<BalanceChange> {
        let signer = self.signer();
        if signer.is_empty() {
            return None;
        }
        // Оптимизация: прямой доступ к балансу signer без итерации по всем аккаунтам
        self.tx.meta.sol_balance_changes.get(&signer).cloned()
    }

    /// Get token balance changes for the signer account (optimized: only process signer balances)
    /// Минимум аллокаций: предварительно резервируем capacity, избегаем лишних клонов
    pub fn signer_token_balance_changes(&self) -> Option<HashMap<String, BalanceChange>> {
        let signer = self.signer();
        if signer.is_empty() {
            return None;
        }
        
        // Оптимизация: предварительно оцениваем размер для минимизации реаллокаций
        let pre_balances = self.pre_token_balances();
        let post_balances = self.post_token_balances();
        let estimated_capacity = (pre_balances.len().max(post_balances.len()) / 4).max(4);
        
        let mut changes = HashMap::with_capacity(estimated_capacity);
        
        // Оптимизация: создаем карту pre-balances ТОЛЬКО для signer (фильтруем сразу)
        // Используем with_capacity для минимизации реаллокаций
        let mut pre_map: HashMap<String, i128> = HashMap::with_capacity(estimated_capacity);
        for b in pre_balances {
            // Проверяем owner сразу, без дополнительных вызовов
            if let Some(owner) = &b.owner {
                if owner == &signer && !b.mint.is_empty() {
                    // Оптимизация: используем parse::<i128> напрямую, избегаем unwrap_or когда возможно
                    if let Ok(raw) = b.ui_token_amount.amount.parse::<i128>() {
                        pre_map.insert(b.mint.clone(), raw);
                    }
                }
            }
        }
        
        // Оптимизация: обрабатываем post-balances ТОЛЬКО для signer
        for b in post_balances {
            if let Some(owner) = &b.owner {
                if owner == &signer && !b.mint.is_empty() {
                    if let Ok(post_raw) = b.ui_token_amount.amount.parse::<i128>() {
                        let mint_clone = b.mint.clone();
                        let pre_raw = pre_map.remove(&mint_clone).unwrap_or(0);
                        let diff = post_raw - pre_raw;
                        
                        if diff != 0 {
                            // Оптимизация: используем remove вместо get для очистки pre_map
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
        
        // Проверяем закрытые аккаунты (есть в pre, но нет в post)
        // Оптимизация: используем into_iter для перемещения вместо клонирования
        for (mint, pre_raw) in pre_map {
            if pre_raw != 0 {
                // Аккаунт был закрыт - баланс стал 0
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
    
    /// Создает кэшированные карты балансов для быстрого доступа
    /// Оптимизация: возвращает ссылки на существующие данные, минимум аллокаций
    /// Возвращает (post_map, pre_map, transfer_map) где ключ - account address
    pub fn cached_balance_maps(&self) -> (
        HashMap<&str, &TokenBalance>, 
        HashMap<&str, &TokenBalance>,
        HashMap<&str, &TransferData>
    ) {
        let post_balances = self.post_token_balances();
        let pre_balances = self.pre_token_balances();
        let transfers = self.transfers();
        
        // Оптимизация: предварительно резервируем capacity для минимизации реаллокаций
        let post_capacity = post_balances.len();
        let pre_capacity = pre_balances.len();
        let transfer_capacity = transfers.len() * 2; // source + destination
        
        let mut post_map = HashMap::with_capacity(post_capacity);
        let mut pre_map = HashMap::with_capacity(pre_capacity);
        let mut transfer_map = HashMap::with_capacity(transfer_capacity);
        
        // Оптимизация: используем ссылки на строки из TokenBalance, избегаем клонов
        for b in post_balances {
            post_map.insert(b.account.as_str(), b);
        }
        
        for b in pre_balances {
            pre_map.insert(b.account.as_str(), b);
        }
        
        // Оптимизация: создаем карту трансферов по source и destination
        for t in transfers {
            transfer_map.insert(t.info.source.as_str(), t);
            transfer_map.insert(t.info.destination.as_str(), t);
        }
        
        (post_map, pre_map, transfer_map)
    }
    
    /// Получает все изменения балансов для signer (SOL + токены) одним вызовом
    /// Оптимизация: объединенный вызов для минимизации overhead
    pub fn signer_all_balance_changes(&self) -> (Option<BalanceChange>, Option<HashMap<String, BalanceChange>>) {
        (
            self.signer_sol_balance_change(),
            self.signer_token_balance_changes(),
        )
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
    /// Оптимизация: предварительно резервируем capacity для минимизации реаллокаций
    pub fn get_account_sol_balance_changes(&self, is_owner: bool) -> HashMap<String, BalanceChange> {
        // Оптимизация: оцениваем размер на основе количества аккаунтов с изменениями
        let estimated_size = self.account_keys.len().min(self.tx.meta.sol_balance_changes.len());
        let mut out = HashMap::with_capacity(estimated_size);

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
        let pre_balances = self.pre_token_balances();
        let post_balances = self.post_token_balances();
        
        // Оптимизация: предварительно оцениваем размеры для минимизации реаллокаций
        let estimated_accounts = (pre_balances.len().max(post_balances.len()) / 2).max(4);
        let mut out: HashMap<String, HashMap<String, BalanceChange>> = HashMap::with_capacity(estimated_accounts);

        // Оптимизация: подготовим словарь pre по (account,mint) -> raw с предварительным capacity
        let pre_capacity = pre_balances.len();
        let mut pre_map: HashMap<(String, String), i128> = HashMap::with_capacity(pre_capacity);
        for b in pre_balances {
            if b.mint.is_empty() {
                continue;
            }
            let account = if is_owner {
                self.get_token_account_owner(&b.account).unwrap_or_else(|| b.account.clone())
            } else {
                b.account.clone()
            };
            // Оптимизация: используем parse::<i128> напрямую, избегаем unwrap_or когда возможно
            if let Ok(raw) = b.ui_token_amount.amount.parse::<i128>() {
                pre_map.insert((account, b.mint.clone()), raw);
            }
        }

        // Оптимизация: post: обновим/посчитаем diff, используем remove для очистки pre_map
        let post_capacity = post_balances.len();
        let mut tmp: HashMap<(String, String), (i128, i128)> = HashMap::with_capacity(post_capacity); // (pre, post)
        for b in post_balances {
            if b.mint.is_empty() {
                continue;
            }
            let account = if is_owner {
                self.get_token_account_owner(&b.account).unwrap_or_else(|| b.account.clone())
            } else {
                b.account.clone()
            };
            if let Ok(post_raw) = b.ui_token_amount.amount.parse::<i128>() {
                let mint_clone = b.mint.clone();
                let pre_raw = pre_map.remove(&(account.clone(), mint_clone.clone())).unwrap_or(0);
                tmp.insert((account, mint_clone), (pre_raw, post_raw));
            }
        }

        // Оптимизация: соберём в нужную иерархию, используем into_iter для перемещения
        for ((account, mint), (pre_raw, post_raw)) in tmp {
            let diff = post_raw - pre_raw;
            if diff == 0 { continue; }
            out.entry(account).or_insert_with(|| HashMap::with_capacity(4)).insert(
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
    /// Оптимизация: предварительно резервируем capacity для минимизации реаллокаций
    pub fn get_transfer_actions(&self) -> TransferMap {
        let transfers = &self.tx.transfers;
        
        // Оптимизация: оцениваем количество уникальных program_id (обычно 1-3)
        let estimated_programs = transfers.len().min(8);
        let mut map: TransferMap = HashMap::with_capacity(estimated_programs);
        
        for t in transfers {
            map.entry(t.program_id.clone()).or_insert_with(|| Vec::with_capacity(4)).push(t.clone());
        }
        
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

        // 4) Extract from instructions (as in TypeScript: extractTokenFromInstructions)
        Self::extract_token_from_instructions(tx, &mut accounts, &mut decimals);

        // 5) гарантируем наличие SOL
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

    /// Extract token info from instructions (as in TypeScript extractTokenFromInstructions)
    fn extract_token_from_instructions(
        tx: &SolanaTransaction,
        accounts: &mut HashMap<String, TokenInfo>,
        decimals: &mut HashMap<String, u8>,
    ) {
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

        // Process outer instructions
        for ix in &tx.instructions {
            if ix.program_id != TOKEN_PROGRAM_ID && ix.program_id != TOKEN_2022_PROGRAM_ID {
                continue;
            }

            let data = crate::core::utils::get_instruction_data(ix);
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
