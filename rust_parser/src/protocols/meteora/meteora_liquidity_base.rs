use crate::core::transaction_adapter::TransactionAdapter;
use crate::core::transaction_utils::TransactionUtils;
use crate::protocols::simple::LiquidityParser;
use crate::types::{ClassifiedInstruction, PoolEvent, PoolEventType, TransferData, TransferMap};

/// Базовый парсер ликвидности для Meteor
pub trait MeteoraLiquidityParserBase: LiquidityParser {
    /// Определяет тип действия пула по данным инструкции
    fn get_pool_action(&self, data: &[u8]) -> Option<PoolEventType>;

    /// Парсит инструкцию создания пула
    fn parse_create_liquidity_event(
        &self,
        instruction: &crate::types::SolanaInstruction,
        index: usize,
        data: &[u8],
        transfers: &[TransferData],
    ) -> Option<PoolEvent>;

    /// Парсит инструкцию добавления ликвидности
    fn parse_add_liquidity_event(
        &self,
        instruction: &crate::types::SolanaInstruction,
        index: usize,
        data: &[u8],
        transfers: &[TransferData],
    ) -> PoolEvent;

    /// Парсит инструкцию удаления ликвидности
    fn parse_remove_liquidity_event(
        &self,
        instruction: &crate::types::SolanaInstruction,
        index: usize,
        data: &[u8],
        transfers: &[TransferData],
    ) -> PoolEvent;
}

/// Базовая реализация для Meteor liquidity парсеров
pub struct MeteoraLiquidityBase {
    pub adapter: TransactionAdapter,
    pub transfer_actions: TransferMap,
    pub classified_instructions: Vec<ClassifiedInstruction>,
    pub utils: TransactionUtils,
}

impl MeteoraLiquidityBase {
    pub fn new(
        adapter: TransactionAdapter,
        transfer_actions: TransferMap,
        classified_instructions: Vec<ClassifiedInstruction>,
    ) -> Self {
        let utils = TransactionUtils::new(adapter.clone());
        Self {
            adapter,
            transfer_actions,
            classified_instructions,
            utils,
        }
    }

    /// Получает transfers для инструкции
    #[inline]
    pub fn get_transfers_for_instruction(
        &self,
        program_id: &str,
        outer_index: usize,
        inner_index: Option<usize>,
    ) -> Vec<&TransferData> {
        let key = if let Some(inner) = inner_index {
            format!("{}:{}-{}", program_id, outer_index, inner)
        } else {
            format!("{}:{}", program_id, outer_index)
        };

        self.transfer_actions.get(&key).map(|v| v.iter().collect()).unwrap_or_default()
    }

    /// Находит инструкцию по дискриминатору
    #[inline]
    pub fn get_instruction_by_discriminator(&self, discriminator: &[u8], slice: usize) -> Option<&ClassifiedInstruction> {
        self.classified_instructions.iter().find(|i| {
            let data = crate::core::utils::get_instruction_data(&i.data);
            data.len() >= slice && &data[..slice] == discriminator
        })
    }
}

