use crate::core::constants::TOKENS;
use crate::protocols::simple::LiquidityParser;
use crate::types::{ClassifiedInstruction, PoolEvent, PoolEventType, TradeType, TransferData, TransferMap};

use super::constants::discriminators::{meteora_damm_v2_events, meteora_damm_v2_u64};
use super::meteora_liquidity_base::MeteoraLiquidityBase;
use super::util::get_lp_transfers;
use crate::core::transaction_adapter::TransactionAdapter;

pub struct MeteoraDAMMV2LiquidityParser {
    base: MeteoraLiquidityBase,
}

impl MeteoraDAMMV2LiquidityParser {
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
            x if x == meteora_damm_v2_u64::INITIALIZE_POOL_U64
                || x == meteora_damm_v2_u64::INITIALIZE_CUSTOM_POOL_U64
                || x == meteora_damm_v2_u64::INITIALIZE_POOL_WITH_DYNAMIC_CONFIG_U64 =>
            {
                Some(PoolEventType::Create)
            }
            x if x == meteora_damm_v2_u64::ADD_LIQUIDITY_U64 => Some(PoolEventType::Add),
            x if x == meteora_damm_v2_u64::CLAIM_POSITION_FEE_U64
                || x == meteora_damm_v2_u64::REMOVE_LIQUIDITY_U64
                || x == meteora_damm_v2_u64::REMOVE_ALL_LIQUIDITY_U64 =>
            {
                Some(PoolEventType::Remove)
            }
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
        let discriminator = &data[..8];
        let disc_u64 = u64::from_le_bytes(discriminator.try_into().ok()?);

        // Находим event instruction по дискриминатору
        let event_instruction = match self.base.get_instruction_by_discriminator(
            &meteora_damm_v2_events::CREATE_POSITION_EVENT,
            16,
        ) {
            Some(inst) => inst,
            None => return None,
        };

        let event_transfers = self.base.get_transfers_for_instruction(
            &event_instruction.program_id,
            event_instruction.outer_index,
            event_instruction.inner_index,
        );
        let event_transfers_owned: Vec<TransferData> = event_transfers.iter().map(|t| (*t).clone()).collect();
        let lp_transfers = get_lp_transfers(&event_transfers_owned);
        let token0 = lp_transfers.get(0).map(|t| (*t).clone());
        let token1 = lp_transfers.get(1).map(|t| (*t).clone());
        let lp_token = transfers.iter().find(|t| t.transfer_type == "mintTo");

        let accounts = self.base.adapter.get_instruction_accounts(instruction);
        let token0_mint = token0
            .as_ref()
            .map(|t| t.info.mint.clone())
            .unwrap_or_else(|| {
                if disc_u64 == meteora_damm_v2_u64::INITIALIZE_CUSTOM_POOL_U64 {
                    accounts.get(7).cloned().unwrap_or_default()
                } else {
                    accounts.get(8).cloned().unwrap_or_default()
                }
            });
        let token1_mint = token1
            .as_ref()
            .map(|t| t.info.mint.clone())
            .unwrap_or_else(|| {
                if disc_u64 == meteora_damm_v2_u64::INITIALIZE_CUSTOM_POOL_U64 {
                    accounts.get(8).cloned().unwrap_or_default()
                } else {
                    accounts.get(9).cloned().unwrap_or_default()
                }
            });

        let program_id = self.base.adapter.get_instruction_program_id(instruction);
        let token0_decimals = self.base.adapter.get_token_decimals(&token0_mint);
        let token1_decimals = self.base.adapter.get_token_decimals(&token1_mint);

        let pool_id = if disc_u64 == meteora_damm_v2_u64::INITIALIZE_CUSTOM_POOL_U64 {
            accounts.get(5).cloned().unwrap_or_default()
        } else if disc_u64 == meteora_damm_v2_u64::INITIALIZE_POOL_WITH_DYNAMIC_CONFIG_U64 {
            accounts.get(7).cloned().unwrap_or_default()
        } else {
            accounts.get(6).cloned().unwrap_or_default()
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
            pool_id,
            config: None,
            pool_lp_mint: lp_token
                .map(|t| t.info.mint.clone())
                .or_else(|| accounts.get(1).cloned()),
            token0_mint: Some(token0_mint),
            token0_amount: token0.as_ref().and_then(|t| t.info.token_amount.ui_amount),
            token0_amount_raw: token0.as_ref().map(|t| t.info.token_amount.amount.clone()),
            token0_balance_change: None,
            token0_decimals: Some(token0_decimals),
            token1_mint: Some(token1_mint),
            token1_amount: token1.as_ref().and_then(|t| t.info.token_amount.ui_amount),
            token1_amount_raw: token1.as_ref().map(|t| t.info.token_amount.amount.clone()),
            token1_balance_change: None,
            token1_decimals: Some(token1_decimals),
            lp_amount: lp_token
                .and_then(|t| t.info.token_amount.ui_amount)
                .or(Some(1.0)),
            lp_amount_raw: lp_token
                .map(|t| t.info.token_amount.amount.clone())
                .or(Some("1".to_string())),
        })
    }

    fn normalize_tokens(&self, transfers: &[TransferData]) -> (Option<TransferData>, Option<TransferData>) {
        let mut lp_transfers = get_lp_transfers(transfers);
        let token0 = lp_transfers.get(0).map(|t| (*t).clone());
        let token1 = lp_transfers.get(1).map(|t| (*t).clone());

        // Если только один transfer и это SOL, то это token1
        if transfers.len() == 1 && transfers[0].info.mint == TOKENS.SOL {
            return (None, Some(transfers[0].clone()));
        }

        (token0, token1)
    }

    fn parse_add_liquidity_event(
        &self,
        instruction: &crate::types::SolanaInstruction,
        index: usize,
        _data: &[u8],
        transfers: &[TransferData],
    ) -> PoolEvent {
        let (token0, token1) = self.normalize_tokens(transfers);
        let program_id = self.base.adapter.get_instruction_program_id(instruction);
        let accounts = self.base.adapter.get_instruction_accounts(instruction);

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
            pool_lp_mint: accounts.get(1).cloned(),
            token0_mint: token0.as_ref().map(|t| t.info.mint.clone()),
            token0_amount: token0.as_ref().and_then(|t| t.info.token_amount.ui_amount).or(Some(0.0)),
            token0_amount_raw: token0.as_ref().map(|t| t.info.token_amount.amount.clone()),
            token0_balance_change: None,
            token0_decimals: token0
                .as_ref()
                .map(|t| self.base.adapter.get_token_decimals(&t.info.mint))
                .or(Some(0)),
            token1_mint: token1.as_ref().map(|t| t.info.mint.clone()),
            token1_amount: token1.as_ref().and_then(|t| t.info.token_amount.ui_amount).or(Some(0.0)),
            token1_amount_raw: token1.as_ref().map(|t| t.info.token_amount.amount.clone()),
            token1_balance_change: None,
            token1_decimals: token1
                .as_ref()
                .map(|t| self.base.adapter.get_token_decimals(&t.info.mint))
                .or(Some(0)),
            lp_amount: None,
            lp_amount_raw: None,
        }
    }

    fn parse_remove_liquidity_event(
        &self,
        instruction: &crate::types::SolanaInstruction,
        index: usize,
        _data: &[u8],
        transfers: &[TransferData],
    ) -> PoolEvent {
        let accounts = self.base.adapter.get_instruction_accounts(instruction);
        let (mut token0, mut token1) = self.normalize_tokens(transfers);

        // Специальная логика для remove: если token1 отсутствует и token0.mint == accounts[8], то token1 = token0
        if token1.is_none() {
            if let Some(ref t0) = token0 {
                if t0.info.mint == accounts.get(8).cloned().unwrap_or_default() {
                    token1 = token0.clone();
                    token0 = None;
                }
            }
        }

        // Если token0 отсутствует и token1.mint == accounts[7], то token0 = token1
        if token0.is_none() {
            if let Some(ref t1) = token1 {
                if t1.info.mint == accounts.get(7).cloned().unwrap_or_default() {
                    token0 = token1.clone();
                    token1 = None;
                }
            }
        }

        let token0_mint = token0
            .as_ref()
            .map(|t| t.info.mint.clone())
            .unwrap_or_else(|| accounts.get(7).cloned().unwrap_or_default());
        let token1_mint = token1
            .as_ref()
            .map(|t| t.info.mint.clone())
            .unwrap_or_else(|| accounts.get(8).cloned().unwrap_or_default());
        let program_id = self.base.adapter.get_instruction_program_id(instruction);

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
            pool_id: accounts.get(1).cloned().unwrap_or_default(),
            config: None,
            pool_lp_mint: accounts.get(2).cloned(),
            token0_mint: Some(
                token0.as_ref()
                    .map(|t| t.info.mint.clone())
                    .unwrap_or_else(|| accounts.get(7).cloned().unwrap_or_default()),
            ),
            token0_amount: token0.as_ref().and_then(|t| t.info.token_amount.ui_amount).or(Some(0.0)),
            token0_amount_raw: token0.as_ref().map(|t| t.info.token_amount.amount.clone()),
            token0_balance_change: None,
            token0_decimals: Some(self.base.adapter.get_token_decimals(&token0_mint)),
            token1_mint: Some(
                token1.as_ref()
                    .map(|t| t.info.mint.clone())
                    .unwrap_or_else(|| accounts.get(8).cloned().unwrap_or_default()),
            ),
            token1_amount: token1.as_ref().and_then(|t| t.info.token_amount.ui_amount).or(Some(0.0)),
            token1_amount_raw: token1.as_ref().map(|t| t.info.token_amount.amount.clone()),
            token1_balance_change: None,
            token1_decimals: Some(self.base.adapter.get_token_decimals(&token1_mint)),
            lp_amount: None,
            lp_amount_raw: None,
        }
    }
}

impl LiquidityParser for MeteoraDAMMV2LiquidityParser {
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

