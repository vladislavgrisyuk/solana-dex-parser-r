use crate::core::transaction_adapter::TransactionAdapter;
use crate::protocols::simple::LiquidityParser;
use crate::types::{ClassifiedInstruction, PoolEvent, TradeType, TransferMap};

use super::constants::{PUMP_SWAP_PROGRAM_ID, PUMP_SWAP_PROGRAM_NAME};
use super::pumpswap_event_parser::{
    PumpswapCreatePoolEvent, PumpswapDepositEvent, PumpswapEvent, PumpswapEventData,
    PumpswapEventParser, PumpswapWithdrawEvent,
};
use super::util::convert_to_ui_amount;

pub struct PumpswapLiquidityParser {
    adapter: TransactionAdapter,
    _transfer_actions: TransferMap,
    classified_instructions: Vec<ClassifiedInstruction>,
    event_parser: PumpswapEventParser,
}

impl PumpswapLiquidityParser {
    pub fn new(
        adapter: TransactionAdapter,
        transfer_actions: TransferMap,
        classified_instructions: Vec<ClassifiedInstruction>,
    ) -> Self {
        // Event parser больше не хранит адаптер
        let event_parser = PumpswapEventParser::new();
        Self {
            adapter,
            _transfer_actions: transfer_actions,
            classified_instructions,
            event_parser,
        }
    }

    fn parse_events(&self) -> Vec<PumpswapEvent> {
        match self
            .event_parser
            .parse_instructions(&self.adapter, &self.classified_instructions)
        {
            Ok(events) => events,
            Err(_) => Vec::new()
        }
    }

    fn parse_create_event(
        &self,
        event: &PumpswapEvent,
        data: &PumpswapCreatePoolEvent,
    ) -> PoolEvent {
        PoolEvent {
            user: self.adapter.signer().to_string(),
            event_type: TradeType::Create,
            program_id: Some(PUMP_SWAP_PROGRAM_ID.to_string()),
            amm: Some(PUMP_SWAP_PROGRAM_NAME.to_string()),
            slot: event.slot,
            timestamp: event.timestamp,
            signature: (*event.signature).clone(),
            idx: event.idx.clone(),
            signer: event.signer.as_ref().map(|s| s.as_ref().clone()),
            pool_id: data.pool.clone(),
            config: None,
            pool_lp_mint: Some(data.lp_mint.clone()),
            token0_mint: Some(data.base_mint.clone()),
            token0_amount: Some(convert_to_ui_amount(
                data.base_amount_in as u128,
                data.base_mint_decimals,
            )),
            token0_amount_raw: Some(data.base_amount_in.to_string()),
            token0_balance_change: None,
            token0_decimals: Some(data.base_mint_decimals),
            token1_mint: Some(data.quote_mint.clone()),
            token1_amount: Some(convert_to_ui_amount(
                data.quote_amount_in as u128,
                data.quote_mint_decimals,
            )),
            token1_amount_raw: Some(data.quote_amount_in.to_string()),
            token1_balance_change: None,
            token1_decimals: Some(data.quote_mint_decimals),
            lp_amount: Some(convert_to_ui_amount(
                data.lp_token_amount_out as u128,
                data.base_mint_decimals,
            )),
            lp_amount_raw: Some(data.lp_token_amount_out.to_string()),
        }
    }

    fn parse_deposit_event(
        &self,
        event: &PumpswapEvent,
        data: &PumpswapDepositEvent,
    ) -> Option<PoolEvent> {
        let token0_info = self
            .adapter
            .token_account_info(&data.user_base_token_account)?;
        let token1_info = self
            .adapter
            .token_account_info(&data.user_quote_token_account)?;
        let lp_info = self
            .adapter
            .token_account_info(&data.user_pool_token_account)?;

        let token0_decimals = self
            .adapter
            .token_decimals(&token0_info.mint)
            .unwrap_or(token0_info.decimals);
        let token1_decimals = self
            .adapter
            .token_decimals(&token1_info.mint)
            .unwrap_or(token1_info.decimals);
        let lp_decimals = self
            .adapter
            .token_decimals(&lp_info.mint)
            .unwrap_or(lp_info.decimals);

        Some(PoolEvent {
            user: self.adapter.signer().to_string(),
            event_type: TradeType::Add,
            program_id: Some(PUMP_SWAP_PROGRAM_ID.to_string()),
            amm: Some(PUMP_SWAP_PROGRAM_NAME.to_string()),
            slot: event.slot,
            timestamp: event.timestamp,
            signature: (*event.signature).clone(),
            idx: event.idx.clone(),
            signer: event.signer.as_ref().map(|s| s.as_ref().clone()),
            pool_id: data.pool.clone(),
            config: None,
            pool_lp_mint: Some(lp_info.mint.clone()),
            token0_mint: Some(token0_info.mint.clone()),
            token0_amount: Some(convert_to_ui_amount(
                data.base_amount_in as u128,
                token0_decimals,
            )),
            token0_amount_raw: Some(data.base_amount_in.to_string()),
            token0_balance_change: None,
            token0_decimals: Some(token0_decimals),
            token1_mint: Some(token1_info.mint.clone()),
            token1_amount: Some(convert_to_ui_amount(
                data.quote_amount_in as u128,
                token1_decimals,
            )),
            token1_amount_raw: Some(data.quote_amount_in.to_string()),
            token1_balance_change: None,
            token1_decimals: Some(token1_decimals),
            lp_amount: Some(convert_to_ui_amount(
                data.lp_token_amount_out as u128,
                lp_decimals,
            )),
            lp_amount_raw: Some(data.lp_token_amount_out.to_string()),
        })
    }

    fn parse_withdraw_event(
        &self,
        event: &PumpswapEvent,
        data: &PumpswapWithdrawEvent,
    ) -> Option<PoolEvent> {
        let token0_info = self
            .adapter
            .token_account_info(&data.user_base_token_account)?;
        let token1_info = self
            .adapter
            .token_account_info(&data.user_quote_token_account)?;
        let lp_info = self
            .adapter
            .token_account_info(&data.user_pool_token_account)?;

        let token0_decimals = self
            .adapter
            .token_decimals(&token0_info.mint)
            .unwrap_or(token0_info.decimals);
        let token1_decimals = self
            .adapter
            .token_decimals(&token1_info.mint)
            .unwrap_or(token1_info.decimals);
        let lp_decimals = self
            .adapter
            .token_decimals(&lp_info.mint)
            .unwrap_or(lp_info.decimals);

        Some(PoolEvent {
            user: self.adapter.signer().to_string(),
            event_type: TradeType::Remove,
            program_id: Some(PUMP_SWAP_PROGRAM_ID.to_string()),
            amm: Some(PUMP_SWAP_PROGRAM_NAME.to_string()),
            slot: event.slot,
            timestamp: event.timestamp,
            signature: (*event.signature).clone(),
            idx: event.idx.clone(),
            signer: event.signer.as_ref().map(|s| s.as_ref().clone()),
            pool_id: data.pool.clone(),
            config: None,
            pool_lp_mint: Some(lp_info.mint.clone()),
            token0_mint: Some(token0_info.mint.clone()),
            token0_amount: Some(convert_to_ui_amount(
                data.base_amount_out as u128,
                token0_decimals,
            )),
            token0_amount_raw: Some(data.base_amount_out.to_string()),
            token0_balance_change: None,
            token0_decimals: Some(token0_decimals),
            token1_mint: Some(token1_info.mint.clone()),
            token1_amount: Some(convert_to_ui_amount(
                data.quote_amount_out as u128,
                token1_decimals,
            )),
            token1_amount_raw: Some(data.quote_amount_out.to_string()),
            token1_balance_change: None,
            token1_decimals: Some(token1_decimals),
            lp_amount: Some(convert_to_ui_amount(
                data.lp_token_amount_in as u128,
                lp_decimals,
            )),
            lp_amount_raw: Some(data.lp_token_amount_in.to_string()),
        })
    }
}

impl LiquidityParser for PumpswapLiquidityParser {
    fn process_liquidity(&mut self) -> Vec<PoolEvent> {
        let parsed_events = self.parse_events();
        let mut events = Vec::with_capacity(parsed_events.len());
        
        for event in parsed_events {
            match &event.data {
                PumpswapEventData::Create(data) => {
                    events.push(self.parse_create_event(&event, data));
                }
                PumpswapEventData::Deposit(data) => {
                    if let Some(pool) = self.parse_deposit_event(&event, data) {
                        events.push(pool);
                    }
                }
                PumpswapEventData::Withdraw(data) => {
                    if let Some(pool) = self.parse_withdraw_event(&event, data) {
                        events.push(pool);
                    }
                }
                _ => {}
            }
        }
        
        events
    }
}

