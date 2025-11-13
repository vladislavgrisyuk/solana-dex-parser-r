use std::sync::Arc;

use crate::core::instruction_classifier::InstructionClassifier;
use crate::core::transaction_adapter::TransactionAdapter;
use crate::core::transaction_utils::TransactionUtils;
use crate::protocols::simple::MemeEventParser;
use crate::types::{ClassifiedInstruction, MemeEvent, TradeType, TransferData, TransferMap};

use super::constants::{
    discriminators::meteora_dbc_u64,
    program_ids, program_names,
};
use crate::protocols::pumpfun::binary_reader::BinaryReaderRef;
use crate::protocols::pumpfun::util::{build_token_info, get_trade_type, sort_by_idx};

pub struct MeteoraDBCEventParser {
    adapter: TransactionAdapter,
    transfer_actions: TransferMap,
    utils: TransactionUtils,
}

impl MeteoraDBCEventParser {
    pub fn new(adapter: TransactionAdapter, transfer_actions: TransferMap) -> Self {
        let utils = TransactionUtils::new(adapter.clone());
        Self {
            adapter,
            transfer_actions,
            utils,
        }
    }

    pub fn parse_instructions(&self, instructions: &[ClassifiedInstruction]) -> Vec<MemeEvent> {
        let mut events = Vec::with_capacity(instructions.len());
        let signature_arc = Arc::new(self.adapter.signature().to_string());
        let slot = self.adapter.slot();
        let timestamp = self.adapter.block_time();

        for classified in instructions {
            let data = match crate::core::utils::get_instruction_data(&classified.data) {
                d if d.is_empty() => continue,
                d => d,
            };

            if data.len() < 8 {
                continue;
            }

            // ОПТИМИЗАЦИЯ: используем u64 для быстрого сравнения
            let disc_bytes: [u8; 8] = match data[..8].try_into() {
                Ok(b) => b,
                Err(_) => continue,
            };
            let disc_u64 = u64::from_le_bytes(disc_bytes);

            let payload = &data[8..];

            let mut event = if disc_u64 == meteora_dbc_u64::SWAP_U64 || disc_u64 == meteora_dbc_u64::SWAP_V2_U64 {
                self.decode_trade_event(payload, &classified.data).ok()
            } else if disc_u64 == meteora_dbc_u64::INITIALIZE_VIRTUAL_POOL_WITH_SPL_TOKEN_U64
                || disc_u64 == meteora_dbc_u64::INITIALIZE_VIRTUAL_POOL_WITH_TOKEN2022_U64
            {
                self.decode_create_event(payload, &classified.data).ok()
            } else if disc_u64 == meteora_dbc_u64::METEORA_DBC_MIGRATE_DAMM_U64 {
                self.decode_dbc_migrate_damm_event(&classified.data).ok()
            } else if disc_u64 == meteora_dbc_u64::METEORA_DBC_MIGRATE_DAMM_V2_U64 {
                self.decode_dbc_migrate_damm_v2_event(&classified.data).ok()
            } else {
                None
            };

            if let Some(ref mut meme_event) = event {
                // Для trade событий пытаемся получить более точные данные из transfers
                if matches!(meme_event.event_type, TradeType::Buy | TradeType::Sell | TradeType::Swap) {
                    let transfers = self.get_transfers_for_instruction(
                        &classified.program_id,
                        classified.outer_index,
                        classified.inner_index,
                    );

                    if transfers.len() >= 2 {
                        let transfer_vec: Vec<TransferData> = transfers.iter().take(2).map(|t| (*t).clone()).collect();
                        if let Some(trade) = self.utils.process_swap_data(&transfer_vec, &crate::types::DexInfo::default()) {
                            meme_event.input_token = Some(trade.input_token);
                            meme_event.output_token = Some(trade.output_token);
                        }
                    }
                }

                meme_event.protocol = Some(program_names::METEORA_DBC.to_string());
                meme_event.signature = (*signature_arc).clone();
                meme_event.slot = slot;
                meme_event.timestamp = timestamp;
                meme_event.idx = format!(
                    "{}-{}",
                    classified.outer_index,
                    classified.inner_index.unwrap_or(0)
                );

                events.push(meme_event.clone());
            }
        }

        sort_by_idx(events)
    }

    fn decode_trade_event(
        &self,
        data: &[u8],
        instruction: &crate::types::SolanaInstruction,
    ) -> Result<MemeEvent, String> {
        // Note: transfers будут получены позже в parse_instructions
        let mut reader = BinaryReaderRef::new_ref(data);
        let accounts = self.adapter.get_instruction_accounts(instruction);

        if accounts.len() < 10 {
            return Err("insufficient accounts".to_string());
        }

        let input_amount = reader.read_u64().map_err(|e| format!("read_u64 failed: {:?}", e))?;
        let output_amount = reader.read_u64().map_err(|e| format!("read_u64 failed: {:?}", e))?;

        let user_account = accounts.get(9).ok_or("missing user account")?.clone();
        let base_mint = accounts.get(7).ok_or("missing base mint")?.clone();
        let quote_mint = accounts.get(8).ok_or("missing quote mint")?.clone();
        let input_token_account = accounts.get(3).ok_or("missing input token account")?.clone();
        let output_token_account = accounts.get(4).ok_or("missing output token account")?.clone();

        // Определяем тип трейда
        let trade_type = self.get_account_trade_type(
            &user_account,
            &base_mint,
            &input_token_account,
            &output_token_account,
        );

        let (input_mint, output_mint) = if trade_type == TradeType::Sell {
            (base_mint.clone(), quote_mint.clone())
        } else {
            (quote_mint.clone(), base_mint.clone())
        };

        Ok(MemeEvent {
            event_type: trade_type,
            timestamp: 0,
            idx: String::new(),
            slot: 0,
            signature: String::new(),
            user: user_account.clone(),
            base_mint: base_mint.clone(),
            quote_mint: quote_mint.clone(),
            input_token: Some(build_token_info(&input_mint, input_amount as u128, 0, None)),
            output_token: Some(build_token_info(&output_mint, output_amount as u128, 0, None)),
            bonding_curve: accounts.get(2).cloned(),
            pool: accounts.get(2).cloned(),
            ..Default::default()
        })
    }

    fn decode_create_event(
        &self,
        data: &[u8],
        instruction: &crate::types::SolanaInstruction,
    ) -> Result<MemeEvent, String> {
        let mut reader = BinaryReaderRef::new_ref(data);
        let accounts = self.adapter.get_instruction_accounts(instruction);

        if accounts.len() < 10 {
            return Err("insufficient accounts".to_string());
        }

        let name = reader.read_string().map_err(|e| format!("read_string failed: {:?}", e))?;
        let symbol = reader.read_string().map_err(|e| format!("read_string failed: {:?}", e))?;
        let uri = reader.read_string().map_err(|e| format!("read_string failed: {:?}", e))?;

        Ok(MemeEvent {
            event_type: TradeType::Create,
            timestamp: 0,
            idx: String::new(),
            slot: 0,
            signature: String::new(),
            user: accounts.get(2).cloned().unwrap_or_default(),
            base_mint: accounts.get(3).cloned().unwrap_or_default(),
            quote_mint: accounts.get(4).cloned().unwrap_or_default(),
            name: Some(name),
            symbol: Some(symbol),
            uri: Some(uri),
            pool: accounts.get(5).cloned(),
            bonding_curve: accounts.get(5).cloned(),
            platform_config: accounts.get(0).cloned(),
            ..Default::default()
        })
    }

    fn decode_dbc_migrate_damm_event(
        &self,
        instruction: &crate::types::SolanaInstruction,
    ) -> Result<MemeEvent, String> {
        let accounts = self.adapter.get_instruction_accounts(instruction);

        Ok(MemeEvent {
            event_type: TradeType::Migrate,
            timestamp: 0,
            idx: String::new(),
            slot: 0,
            signature: String::new(),
            user: String::new(),
            base_mint: accounts.get(7).cloned().unwrap_or_default(),
            quote_mint: accounts.get(8).cloned().unwrap_or_default(),
            platform_config: accounts.get(2).cloned(),
            bonding_curve: accounts.get(0).cloned(),
            pool: accounts.get(4).cloned(),
            pool_dex: Some(program_names::METEORA_DAMM.to_string()),
            ..Default::default()
        })
    }

    fn decode_dbc_migrate_damm_v2_event(
        &self,
        instruction: &crate::types::SolanaInstruction,
    ) -> Result<MemeEvent, String> {
        let accounts = self.adapter.get_instruction_accounts(instruction);

        Ok(MemeEvent {
            event_type: TradeType::Migrate,
            timestamp: 0,
            idx: String::new(),
            slot: 0,
            signature: String::new(),
            user: String::new(),
            base_mint: accounts.get(13).cloned().unwrap_or_default(),
            quote_mint: accounts.get(14).cloned().unwrap_or_default(),
            platform_config: accounts.get(2).cloned(),
            bonding_curve: accounts.get(0).cloned(),
            pool: accounts.get(4).cloned(),
            pool_dex: Some(program_names::METEORA_DAMM_V2.to_string()),
            ..Default::default()
        })
    }

    /// Получает transfers для инструкции
    #[inline]
    fn get_transfers_for_instruction(
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

        self.transfer_actions
            .get(&key)
            .map(|transfers| {
                transfers
                    .iter()
                    .filter(|t| {
                        matches!(
                            t.transfer_type.as_str(),
                            "transfer" | "transferChecked"
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Определяет тип трейда по аккаунтам (аналог GetAccountTradeType)
    fn get_account_trade_type(
        &self,
        _user_account: &str,
        _base_mint: &str,
        _input_user_account: &str,
        _output_user_account: &str,
    ) -> TradeType {
        // Упрощенная версия: в реальной реализации нужно вычислять ATA адреса
        // Для упрощения используем Swap, детали будут уточнены из transfers
        TradeType::Swap
    }

    /// Публичный метод для доступа к utils (для DBC parser)
    pub fn get_utils(&self) -> &TransactionUtils {
        &self.utils
    }
}

impl MemeEventParser for MeteoraDBCEventParser {
    fn process_events(&mut self) -> Vec<MemeEvent> {
        let classifier = InstructionClassifier::new(&self.adapter);
        let instructions = classifier.get_instructions(program_ids::METEORA_DBC);
        self.parse_instructions(&instructions)
    }
}

