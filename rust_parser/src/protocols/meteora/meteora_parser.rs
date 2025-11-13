use crate::core::constants::dex_program_names;
use crate::core::transaction_adapter::TransactionAdapter;
use crate::core::transaction_utils::TransactionUtils;
use crate::protocols::simple::TradeParser;
use crate::types::{ClassifiedInstruction, DexInfo, TradeInfo, TransferData, TransferMap};

use super::constants::{
    discriminators::{
        meteora_damm_u64, meteora_damm_v2_u64, meteora_dlmm_u64,
    },
    program_ids,
};

pub struct MeteoraParser {
    adapter: TransactionAdapter,
    dex_info: DexInfo,
    transfer_actions: TransferMap,
    classified_instructions: Vec<ClassifiedInstruction>,
    utils: TransactionUtils,
}

impl MeteoraParser {
    pub fn new(
        adapter: TransactionAdapter,
        dex_info: DexInfo,
        transfer_actions: TransferMap,
        classified_instructions: Vec<ClassifiedInstruction>,
    ) -> Self {
        let utils = TransactionUtils::new(adapter.clone());
        Self {
            adapter,
            dex_info,
            transfer_actions,
            classified_instructions,
            utils,
        }
    }

    /// Проверяет, что инструкция не является liquidity событием
    #[inline]
    fn is_not_liquidity_event(&self, data: &[u8]) -> bool {
        if data.len() < 8 {
            return true;
        }

        // ОПТИМИЗАЦИЯ: используем u64 для быстрого сравнения
        let disc_bytes: [u8; 8] = match data[..8].try_into() {
            Ok(b) => b,
            Err(_) => return true,
        };
        let disc_u64 = u64::from_le_bytes(disc_bytes);

        // Проверяем DLMM liquidity discriminators (исключаем SWAP из проверки liquidity)
        let is_dlmm_liquidity = matches!(
            disc_u64,
            meteora_dlmm_u64::ADD_LIQUIDITY_U64
                | meteora_dlmm_u64::ADD_LIQUIDITY_BY_STRATEGY_U64
                | meteora_dlmm_u64::ADD_LIQUIDITY_BY_STRATEGY2_U64
                | meteora_dlmm_u64::ADD_LIQUIDITY_BY_STRATEGY_ONE_SIDE_U64
                | meteora_dlmm_u64::ADD_LIQUIDITY_ONE_SIDE_U64
                | meteora_dlmm_u64::ADD_LIQUIDITY_ONE_SIDE_PRECISE_U64
                | meteora_dlmm_u64::ADD_LIQUIDITY_BY_WEIGHT_U64
                | meteora_dlmm_u64::REMOVE_LIQUIDITY_U64
                | meteora_dlmm_u64::REMOVE_LIQUIDITY_BY_RANGE_U64
                | meteora_dlmm_u64::REMOVE_LIQUIDITY_BY_RANGE2_U64
                | meteora_dlmm_u64::REMOVE_ALL_LIQUIDITY_U64
                | meteora_dlmm_u64::CLAIM_FEE_U64
                | meteora_dlmm_u64::CLAIM_FEE_V2_U64
            // SWAP не включаем в liquidity check
        );

        // Проверяем DAMM liquidity discriminators
        let is_damm_liquidity = matches!(
            disc_u64,
            meteora_damm_u64::CREATE_U64
                | meteora_damm_u64::ADD_LIQUIDITY_U64
                | meteora_damm_u64::REMOVE_LIQUIDITY_U64
                | meteora_damm_u64::ADD_IMBALANCE_LIQUIDITY_U64
        );

        // Проверяем DAMM_V2 liquidity discriminators
        let is_damm_v2_liquidity = matches!(
            disc_u64,
            meteora_damm_v2_u64::INITIALIZE_POOL_U64
                | meteora_damm_v2_u64::INITIALIZE_CUSTOM_POOL_U64
                | meteora_damm_v2_u64::INITIALIZE_POOL_WITH_DYNAMIC_CONFIG_U64
                | meteora_damm_v2_u64::ADD_LIQUIDITY_U64
                | meteora_damm_v2_u64::CLAIM_POSITION_FEE_U64
                | meteora_damm_v2_u64::REMOVE_LIQUIDITY_U64
                | meteora_damm_v2_u64::REMOVE_ALL_LIQUIDITY_U64
        );

        !is_dlmm_liquidity && !is_damm_liquidity && !is_damm_v2_liquidity
    }

    /// Получает адрес пула из accounts инструкции
    #[inline]
    fn get_pool_address(&self, instruction: &crate::types::SolanaInstruction, program_id: &str) -> Option<String> {
        let accounts = self.adapter.get_instruction_accounts(instruction);
        if accounts.len() > 5 {
            match program_id {
                program_ids::METEORA_DAMM | program_ids::METEORA => accounts.get(0).cloned(),
                program_ids::METEORA_DAMM_V2 => accounts.get(1).cloned(),
                _ => None,
            }
        } else {
            None
        }
    }

    /// Получает transfers для инструкции
    /// Использует ключ в формате TypeScript: `${programId}:${outerIndex}-${innerIndex}`
    /// Получает transfers для инструкции по ключу (как в TypeScript getTransfersForInstruction)
    /// ОПТИМИЗИРОВАНО: использует itoa для форматирования ключа
    #[inline]
    fn get_transfers_for_instruction(
        &self,
        program_id: &str,
        outer_index: usize,
        inner_index: Option<usize>,
    ) -> Vec<&TransferData> {
        // В TypeScript: key = `${programId}:${outerIndex}${innerIndex == undefined ? '' : `-${innerIndex}`}`
        // В Rust версии create_transfers_from_instructions создает такие же ключи
        // ОПТИМИЗАЦИЯ: используем itoa вместо format!
        let mut key_buf = String::with_capacity(128);
        key_buf.push_str(program_id);
        key_buf.push(':');
        let mut num_buf = itoa::Buffer::new();
        key_buf.push_str(num_buf.format(outer_index));
        if let Some(inner) = inner_index {
            key_buf.push('-');
            key_buf.push_str(num_buf.format(inner));
        }
        
        // Ищем transfers по ключу (как в TypeScript)
        let transfers = self.transfer_actions.get(&key_buf).map(|v| v.as_slice()).unwrap_or(&[]);
        
        // Фильтруем только transfer и transferChecked (как в TypeScript)
        // ОПТИМИЗАЦИЯ: предварительно резервируем capacity
        let mut result = Vec::with_capacity(transfers.len());
        for t in transfers {
            if matches!(t.transfer_type.as_str(), "transfer" | "transferChecked") {
                result.push(t);
            }
        }
        result
    }
}

impl TradeParser for MeteoraParser {
    fn process_trades(&mut self) -> Vec<TradeInfo> {
        let mut trades = Vec::new();

        for classified in &self.classified_instructions {
            let program_id = &classified.program_id;
            
            // Проверяем, что это Meteor program
            if !matches!(
                program_id.as_str(),
                program_ids::METEORA | program_ids::METEORA_DAMM | program_ids::METEORA_DAMM_V2
            ) {
                continue;
            }

            // Проверяем, что это не liquidity событие
            let instruction_data = crate::core::utils::get_instruction_data(&classified.data);

            if !self.is_not_liquidity_event(&instruction_data) {
                continue;
            }

            // Получаем transfers для инструкции
            let transfers = self.get_transfers_for_instruction(
                program_id,
                classified.outer_index,
                classified.inner_index,
            );

            if transfers.len() < 2 {
                continue;
            }

            // Для Meteora нужно проверить, что есть transfers с разными mints
            // Передаем все transfers в process_swap_data, чтобы суммы считались правильно
            let unique_mints: std::collections::HashSet<&str> = transfers
                .iter()
                .filter_map(|t| {
                    if t.info.mint.is_empty() {
                        None
                    } else {
                        Some(t.info.mint.as_str())
                    }
                })
                .collect();

            if unique_mints.len() < 2 {
                continue;
            }

            // Передаем все transfers в process_swap_data, чтобы он суммировал все transfers с каждым mint
            let transfers_vec: Vec<TransferData> = transfers.iter().map(|t| (*t).clone()).collect();

            // Создаем trade через processSwapData
            let mut trade = match self.utils.process_swap_data(
                &transfers_vec,
                &DexInfo {
                    program_id: Some(program_id.clone()),
                    amm: self.dex_info.amm.clone()
                        .filter(|a| a != "Unknown DEX")
                        .or_else(|| {
                            Some(dex_program_names::name(program_id).to_string())
                        }),
                    route: self.dex_info.route.clone(),
                },
            ) {
                Some(t) => t,
                None => continue,
            };

            // Получаем pool address
            if let Some(pool) = self.get_pool_address(&classified.data, program_id) {
                trade.pool = vec![pool];
            }

            // Прикрепляем token transfer info
            let final_trade = self.utils.attach_token_transfer_info(trade, &self.transfer_actions);
            trades.push(final_trade);
        }

        trades
    }
}

