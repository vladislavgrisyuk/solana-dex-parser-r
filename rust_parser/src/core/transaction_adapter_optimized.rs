// Оптимизированные версии методов TransactionAdapter
// Эти функции можно интегрировать в transaction_adapter.rs

use std::collections::HashMap;
use crate::types::{BalanceChange, TokenBalance};

impl TransactionAdapter {
    /// Оптимизированная версия: вычисляет изменения балансов токенов ТОЛЬКО для signer
    /// Вместо вычисления для всех аккаунтов и последующей фильтрации
    pub fn signer_token_balance_changes_optimized(&self) -> Option<HashMap<String, BalanceChange>> {
        let signer = self.signer();
        if signer.is_empty() {
            return None;
        }
        
        let start = std::time::Instant::now();
        let mut changes = HashMap::new();
        
        // Создаем карту pre-balances ТОЛЬКО для signer (фильтруем сразу)
        let mut pre_map: HashMap<String, i128> = HashMap::new();
        for b in self.pre_token_balances() {
            // Проверяем owner сразу, без дополнительных вызовов
            if let Some(owner) = &b.owner {
                if owner == &signer && !b.mint.is_empty() {
                    let raw = b.ui_token_amount.amount.parse::<i128>().unwrap_or(0);
                    pre_map.insert(b.mint.clone(), raw);
                }
            }
        }
        
        // Обрабатываем post-balances ТОЛЬКО для signer
        for b in self.post_token_balances() {
            if let Some(owner) = &b.owner {
                if owner == &signer && !b.mint.is_empty() {
                    let post_raw = b.ui_token_amount.amount.parse::<i128>().unwrap_or(0);
                    let pre_raw = pre_map.get(&b.mint).copied().unwrap_or(0);
                    let diff = post_raw - pre_raw;
                    
                    if diff != 0 {
                        changes.insert(b.mint.clone(), BalanceChange {
                            pre: pre_raw,
                            post: post_raw,
                            change: diff,
                        });
                    }
                }
            }
        }
        
        // Также проверяем закрытые аккаунты (есть в pre, но нет в post)
        for (mint, pre_raw) in pre_map {
            if !changes.contains_key(&mint) && pre_raw != 0 {
                // Аккаунт был закрыт - баланс стал 0
                changes.insert(mint, BalanceChange {
                    pre: pre_raw,
                    post: 0,
                    change: -pre_raw,
                });
            }
        }
        
        let duration = start.elapsed();
        if !changes.is_empty() {
            tracing::debug!(
                "⏱️  signer_token_balance_changes_optimized: {:.3}μs, found {} token changes",
                duration.as_secs_f64() * 1_000_000.0,
                changes.len()
            );
        }
        
        if changes.is_empty() {
            None
        } else {
            Some(changes)
        }
    }

    /// Создает кэшированные карты балансов для быстрого доступа
    /// Возвращает (post_map, pre_map) где ключ - account address
    pub fn cached_balance_maps(&self) -> (HashMap<&str, &TokenBalance>, HashMap<&str, &TokenBalance>) {
        let post_balances = self.post_token_balances();
        let pre_balances = self.pre_token_balances();
        
        let post_map: HashMap<&str, &TokenBalance> = post_balances
            .iter()
            .map(|b| (b.account.as_str(), b))
            .collect();
        
        let pre_map: HashMap<&str, &TokenBalance> = pre_balances
            .iter()
            .map(|b| (b.account.as_str(), b))
            .collect();
        
        (post_map, pre_map)
    }

    /// Получает изменения балансов SOL для signer (уже оптимизировано, но можно улучшить)
    pub fn signer_sol_balance_change_optimized(&self) -> Option<BalanceChange> {
        let signer = self.signer();
        if signer.is_empty() {
            return None;
        }
        
        // Прямой доступ к изменениям баланса signer без итерации по всем аккаунтам
        self.tx.meta.sol_balance_changes.get(&signer).cloned()
    }

    /// Получает все изменения балансов для signer (SOL + токены) одним вызовом
    /// Возвращает (sol_change, token_changes)
    pub fn signer_all_balance_changes(&self) -> (Option<BalanceChange>, Option<HashMap<String, BalanceChange>>) {
        (
            self.signer_sol_balance_change_optimized(),
            self.signer_token_balance_changes_optimized(),
        )
    }
}

