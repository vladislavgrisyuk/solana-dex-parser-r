use crate::core::constants::TOKENS;
use crate::protocols::simple::LiquidityParser;
use crate::types::{ClassifiedInstruction, PoolEvent, PoolEventType, TradeType, TransferData, TransferMap};

use super::constants::discriminators::meteora_damm_u64;
use super::meteora_liquidity_base::MeteoraLiquidityBase;
use super::util::{convert_to_ui_amount, get_lp_transfers};
use crate::core::transaction_adapter::TransactionAdapter;

pub struct MeteoraPoolsLiquidityParser {
    base: MeteoraLiquidityBase,
}

impl MeteoraPoolsLiquidityParser {
    pub fn new(
        adapter: TransactionAdapter,
        transfer_actions: TransferMap,
        classified_instructions: Vec<ClassifiedInstruction>,
    ) -> Self {
        Self {
            base: MeteoraLiquidityBase::new(adapter, transfer_actions, classified_instructions),
        }
    }

    #[inline]
    fn get_pool_action(&self, data: &[u8]) -> Option<PoolEventType> {
        if data.len() < 8 {
            return None;
        }

        let disc_bytes: [u8; 8] = match data[..8].try_into() {
            Ok(b) => b,
            Err(_) => return None,
        };
        let disc_u64 = u64::from_le_bytes(disc_bytes);

        match disc_u64 {
            x if x == meteora_damm_u64::CREATE_U64 => Some(PoolEventType::Create),
            x if x == meteora_damm_u64::ADD_LIQUIDITY_U64 || x == meteora_damm_u64::ADD_IMBALANCE_LIQUIDITY_U64 => {
                Some(PoolEventType::Add)
            }
            x if x == meteora_damm_u64::REMOVE_LIQUIDITY_U64 => Some(PoolEventType::Remove),
            _ => None,
        }
    }

    fn parse_instruction(
        &self,
        instruction: &crate::types::SolanaInstruction,
        program_id: &str,
        outer_index: usize,
        inner_index: Option<usize>,
    ) -> Option<PoolEvent> {
        let data = crate::core::utils::get_instruction_data(instruction);
        let action = self.get_pool_action(&data)?;

        let mut transfers = self.base.get_transfers_for_instruction(program_id, outer_index, inner_index);
        if transfers.is_empty() {
            transfers = self.base.get_transfers_for_instruction(program_id, outer_index, Some(inner_index.unwrap_or(0)));
        }

        let transfers_owned: Vec<TransferData> = transfers.iter().map(|t| (*t).clone()).collect();

        match action {
            PoolEventType::Create => {
                self.parse_create_liquidity_event(instruction, outer_index, &data, &transfers_owned)
            }
            PoolEventType::Add => Some(self.parse_add_liquidity_event(instruction, outer_index, &data, &transfers_owned)),
            PoolEventType::Remove => {
                Some(self.parse_remove_liquidity_event(instruction, outer_index, &data, &transfers_owned))
            }
        }
    }

    fn parse_create_liquidity_event(
        &self,
        instruction: &crate::types::SolanaInstruction,
        index: usize,
        data: &[u8],
        transfers: &[TransferData],
    ) -> Option<PoolEvent> {
        let accounts = self.base.adapter.get_instruction_accounts(instruction);
        let lp_transfers = get_lp_transfers(transfers);
        let token0 = lp_transfers.get(0).map(|t| (*t).clone());
        let token1 = lp_transfers.get(1).map(|t| (*t).clone());
        let lp_token = transfers.iter().find(|t| t.transfer_type == "mintTo");

        let token0_mint = token0.as_ref().map(|t| t.info.mint.clone()).unwrap_or_else(|| accounts.get(3).cloned().unwrap_or_default());
        let token1_mint = token1.as_ref().map(|t| t.info.mint.clone()).unwrap_or_else(|| accounts.get(4).cloned().unwrap_or_default());
        let program_id = self.base.adapter.get_instruction_program_id(instruction);
        let token0_decimals = self.base.adapter.get_token_decimals(&token0_mint);
        let token1_decimals = self.base.adapter.get_token_decimals(&token1_mint);

        // Читаем из data: offset 16 для token0, offset 8 для token1
        let token0_amount_raw = if data.len() >= 24 {
            u64::from_le_bytes(data[16..24].try_into().ok()?)
        } else {
            token0.as_ref().and_then(|t| t.info.token_amount.amount.parse::<u64>().ok()).unwrap_or(0)
        };

        let token1_amount_raw = if data.len() >= 16 {
            u64::from_le_bytes(data[8..16].try_into().ok()?)
        } else {
            token1.as_ref().and_then(|t| t.info.token_amount.amount.parse::<u64>().ok()).unwrap_or(0)
        };

        let mut base = self.base.adapter.get_pool_event_base(PoolEventType::Create, program_id);
        base.idx = index.to_string();

        Some(PoolEvent {
            user: base.user,
            event_type: TradeType::Create,
            program_id: base.program_id,
            amm: base.amm,
            slot: base.slot,
            timestamp: base.timestamp,
            signature: base.signature,
            idx: base.idx,
            signer: base.signer,
            pool_id: accounts.get(0)?.clone(),
            config: None,
            pool_lp_mint: accounts.get(2).cloned(),
            token0_mint: Some(token0_mint),
            token0_amount: Some(
                token0.as_ref()
                    .and_then(|t| t.info.token_amount.ui_amount)
                    .unwrap_or_else(|| convert_to_ui_amount(token0_amount_raw as u128, token0_decimals)),
            ),
            token0_amount_raw: Some(
                token0.as_ref()
                    .map(|t| t.info.token_amount.amount.clone())
                    .unwrap_or_else(|| token0_amount_raw.to_string()),
            ),
            token0_balance_change: None,
            token0_decimals: Some(token0_decimals),
            token1_mint: Some(token1_mint),
            token1_amount: Some(
                token1.as_ref()
                    .and_then(|t| t.info.token_amount.ui_amount)
                    .unwrap_or_else(|| convert_to_ui_amount(token1_amount_raw as u128, token1_decimals)),
            ),
            token1_amount_raw: Some(
                token1.as_ref()
                    .map(|t| t.info.token_amount.amount.clone())
                    .unwrap_or_else(|| token1_amount_raw.to_string()),
            ),
            token1_balance_change: None,
            token1_decimals: Some(token1_decimals),
            lp_amount: lp_token
                .and_then(|t| t.info.token_amount.ui_amount)
                .or(Some(0.0)),
            lp_amount_raw: lp_token.map(|t| t.info.token_amount.amount.clone()),
        })
    }

    fn parse_add_liquidity_event(
        &self,
        instruction: &crate::types::SolanaInstruction,
        index: usize,
        data: &[u8],
        transfers: &[TransferData],
    ) -> PoolEvent {
        let accounts = self.base.adapter.get_instruction_accounts(instruction);
        let lp_transfers = get_lp_transfers(transfers);
        let token0 = lp_transfers.get(0).map(|t| (*t).clone());
        let token1 = lp_transfers.get(1).map(|t| (*t).clone());
        let lp_token = transfers.iter().find(|t| t.transfer_type == "mintTo");

        let token0_mint = token0.as_ref().map(|t| t.info.mint.clone());
        let token1_mint = token1.as_ref().map(|t| t.info.mint.clone());
        let program_id = self.base.adapter.get_instruction_program_id(instruction);
        let token0_decimals = token0_mint
            .as_ref()
            .map(|m| self.base.adapter.get_token_decimals(m))
            .unwrap_or(0);
        let token1_decimals = token1_mint
            .as_ref()
            .map(|m| self.base.adapter.get_token_decimals(m))
            .unwrap_or(0);

        // Читаем из data: offset 24 для token0, offset 16 для token1, offset 8 для lp
        let token0_amount_raw = if data.len() >= 32 {
            u64::from_le_bytes(data[24..32].try_into().unwrap_or([0; 8]))
        } else {
            token0.as_ref().and_then(|t| t.info.token_amount.amount.parse::<u64>().ok()).unwrap_or(0)
        };

        let token1_amount_raw = if data.len() >= 24 {
            u64::from_le_bytes(data[16..24].try_into().unwrap_or([0; 8]))
        } else {
            token1.as_ref().and_then(|t| t.info.token_amount.amount.parse::<u64>().ok()).unwrap_or(0)
        };

        let lp_amount_raw = if data.len() >= 16 {
            u64::from_le_bytes(data[8..16].try_into().unwrap_or([0; 8]))
        } else {
            lp_token.and_then(|t| t.info.token_amount.amount.parse::<u64>().ok()).unwrap_or(0)
        };

        let lp_mint = accounts.get(1).cloned().unwrap_or_default();
        let lp_decimals = self.base.adapter.get_token_decimals(&lp_mint);

        let mut base = self.base.adapter.get_pool_event_base(PoolEventType::Add, program_id);
        base.idx = index.to_string();

        PoolEvent {
            user: base.user,
            event_type: TradeType::Add,
            program_id: base.program_id,
            amm: base.amm,
            slot: base.slot,
            timestamp: base.timestamp,
            signature: base.signature,
            idx: base.idx,
            signer: base.signer,
            pool_id: accounts.get(0).cloned().unwrap_or_default(),
            config: None,
            pool_lp_mint: Some(lp_mint),
            token0_mint,
            token0_amount: Some(
                token0.as_ref()
                    .and_then(|t| t.info.token_amount.ui_amount)
                    .unwrap_or_else(|| convert_to_ui_amount(token0_amount_raw as u128, token0_decimals)),
            ),
            token0_amount_raw: Some(
                token0.as_ref()
                    .map(|t| t.info.token_amount.amount.clone())
                    .unwrap_or_else(|| token0_amount_raw.to_string()),
            ),
            token0_balance_change: None,
            token0_decimals: Some(token0_decimals),
            token1_mint,
            token1_amount: Some(
                token1.as_ref()
                    .and_then(|t| t.info.token_amount.ui_amount)
                    .unwrap_or_else(|| convert_to_ui_amount(token1_amount_raw as u128, token1_decimals)),
            ),
            token1_amount_raw: Some(
                token1.as_ref()
                    .map(|t| t.info.token_amount.amount.clone())
                    .unwrap_or_else(|| token1_amount_raw.to_string()),
            ),
            token1_balance_change: None,
            token1_decimals: Some(token1_decimals),
            lp_amount: Some(
                lp_token
                    .and_then(|t| t.info.token_amount.ui_amount)
                    .unwrap_or_else(|| convert_to_ui_amount(lp_amount_raw as u128, lp_decimals)),
            ),
            lp_amount_raw: Some(
                lp_token
                    .map(|t| t.info.token_amount.amount.clone())
                    .unwrap_or_else(|| lp_amount_raw.to_string()),
            ),
        }
    }

    fn parse_remove_liquidity_event(
        &self,
        instruction: &crate::types::SolanaInstruction,
        index: usize,
        data: &[u8],
        transfers: &[TransferData],
    ) -> PoolEvent {
        let accounts = self.base.adapter.get_instruction_accounts(instruction);
        let lp_transfers = get_lp_transfers(transfers);
        let token0 = lp_transfers.get(0).map(|t| (*t).clone());
        let token1 = lp_transfers.get(1).map(|t| (*t).clone());
        let lp_token = transfers.iter().find(|t| t.transfer_type == "burn");

        let token0_mint = token0.as_ref().map(|t| t.info.mint.clone());
        let token1_mint = token1.as_ref().map(|t| t.info.mint.clone());
        let program_id = self.base.adapter.get_instruction_program_id(instruction);
        let token0_decimals = token0_mint
            .as_ref()
            .map(|m| self.base.adapter.get_token_decimals(m))
            .unwrap_or(0);
        let token1_decimals = token1_mint
            .as_ref()
            .map(|m| self.base.adapter.get_token_decimals(m))
            .unwrap_or(0);

        // Читаем из data: offset 24 для token0, offset 16 для token1, offset 8 для lp
        let token0_amount_raw = if data.len() >= 32 {
            u64::from_le_bytes(data[24..32].try_into().unwrap_or([0; 8]))
        } else {
            token0.as_ref().and_then(|t| t.info.token_amount.amount.parse::<u64>().ok()).unwrap_or(0)
        };

        let token1_amount_raw = if data.len() >= 24 {
            u64::from_le_bytes(data[16..24].try_into().unwrap_or([0; 8]))
        } else {
            token1.as_ref().and_then(|t| t.info.token_amount.amount.parse::<u64>().ok()).unwrap_or(0)
        };

        let lp_amount_raw = if data.len() >= 16 {
            u64::from_le_bytes(data[8..16].try_into().unwrap_or([0; 8]))
        } else {
            lp_token.and_then(|t| t.info.token_amount.amount.parse::<u64>().ok()).unwrap_or(0)
        };

        let lp_mint = accounts.get(1).cloned().unwrap_or_default();
        let lp_decimals = self.base.adapter.get_token_decimals(&lp_mint);

        let mut base = self.base.adapter.get_pool_event_base(PoolEventType::Remove, program_id);
        base.idx = index.to_string();

        PoolEvent {
            user: base.user,
            event_type: TradeType::Remove,
            program_id: base.program_id,
            amm: base.amm,
            slot: base.slot,
            timestamp: base.timestamp,
            signature: base.signature,
            idx: base.idx,
            signer: base.signer,
            pool_id: accounts.get(0).cloned().unwrap_or_default(),
            config: None,
            pool_lp_mint: Some(lp_mint),
            token0_mint,
            token0_amount: Some(
                token0.as_ref()
                    .and_then(|t| t.info.token_amount.ui_amount)
                    .unwrap_or_else(|| convert_to_ui_amount(token0_amount_raw as u128, token0_decimals)),
            ),
            token0_amount_raw: Some(
                token0.as_ref()
                    .map(|t| t.info.token_amount.amount.clone())
                    .unwrap_or_else(|| token0_amount_raw.to_string()),
            ),
            token0_balance_change: None,
            token0_decimals: Some(token0_decimals),
            token1_mint,
            token1_amount: Some(
                token1.as_ref()
                    .and_then(|t| t.info.token_amount.ui_amount)
                    .unwrap_or_else(|| convert_to_ui_amount(token1_amount_raw as u128, token1_decimals)),
            ),
            token1_amount_raw: Some(
                token1.as_ref()
                    .map(|t| t.info.token_amount.amount.clone())
                    .unwrap_or_else(|| token1_amount_raw.to_string()),
            ),
            token1_balance_change: None,
            token1_decimals: Some(token1_decimals),
            lp_amount: Some(
                lp_token
                    .and_then(|t| t.info.token_amount.ui_amount)
                    .unwrap_or_else(|| convert_to_ui_amount(lp_amount_raw as u128, lp_decimals)),
            ),
            lp_amount_raw: Some(
                lp_token
                    .map(|t| t.info.token_amount.amount.clone())
                    .unwrap_or_else(|| lp_amount_raw.to_string()),
            ),
        }
    }
}

impl LiquidityParser for MeteoraPoolsLiquidityParser {
    fn process_liquidity(&mut self) -> Vec<PoolEvent> {
        let mut events = Vec::new();

        for classified in &self.base.classified_instructions {
            let program_id = &classified.program_id;
            if let Some(event) = self.parse_instruction(&classified.data, program_id, classified.outer_index, classified.inner_index) {
                events.push(event);
            }
        }

        events
    }
}

