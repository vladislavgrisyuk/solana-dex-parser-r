use crate::core::transaction_adapter::TransactionAdapter;
use crate::types::ClassifiedInstruction;

use super::binary_reader::BinaryReader;
use super::constants::discriminators::pumpswap_instructions;
use super::error::PumpfunError;
use super::pumpswap_event_parser::{
    PumpswapBuyEvent, PumpswapCreatePoolEvent, PumpswapDepositEvent, PumpswapSellEvent,
    PumpswapWithdrawEvent,
};
use super::util::{get_instruction_data, sort_by_idx, HasIdx};

#[derive(Clone, Debug, PartialEq)]
pub enum PumpswapInstructionType {
    Create,
    Add,
    Remove,
    Buy,
    Sell,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PumpswapInstructionData {
    Create(PumpswapCreatePoolEvent),
    Add(PumpswapDepositEvent),
    Remove(PumpswapWithdrawEvent),
    Buy(PumpswapBuyEvent),
    Sell(PumpswapSellEvent),
}

#[derive(Clone, Debug, PartialEq)]
pub struct PumpswapInstruction {
    pub instruction_type: PumpswapInstructionType,
    pub data: PumpswapInstructionData,
    pub slot: u64,
    pub timestamp: u64,
    pub signature: String,
    pub idx: String,
    pub signer: Vec<String>,
}

pub struct PumpswapInstructionParser {
    adapter: TransactionAdapter,
}

impl PumpswapInstructionParser {
    pub fn new(adapter: TransactionAdapter) -> Self {
        Self { adapter }
    }

    pub fn parse_instructions(
        &self,
        instructions: &[ClassifiedInstruction],
    ) -> Result<Vec<PumpswapInstruction>, PumpfunError> {
        let mut events = Vec::new();
        for instruction in instructions {
            let data = get_instruction_data(&instruction.data)?;
            if data.len() < 8 {
                continue;
            }
            let discriminator = &data[..8];
            let payload = data[8..].to_vec();
            let parsed = if discriminator == pumpswap_instructions::CREATE_POOL {
                Some(PumpswapInstructionType::Create)
            } else if discriminator == pumpswap_instructions::ADD_LIQUIDITY {
                Some(PumpswapInstructionType::Add)
            } else if discriminator == pumpswap_instructions::REMOVE_LIQUIDITY {
                Some(PumpswapInstructionType::Remove)
            } else if discriminator == pumpswap_instructions::BUY {
                Some(PumpswapInstructionType::Buy)
            } else if discriminator == pumpswap_instructions::SELL {
                Some(PumpswapInstructionType::Sell)
            } else {
                None
            };

            if let Some(inst_type) = parsed {
                let data = self.decode_instruction(&inst_type, instruction, payload)?;
                events.push(PumpswapInstruction {
                    instruction_type: inst_type,
                    data,
                    slot: self.adapter.slot(),
                    timestamp: self.adapter.block_time(),
                    signature: self.adapter.signature().to_string(),
                    idx: format!(
                        "{}-{}",
                        instruction.outer_index,
                        instruction.inner_index.unwrap_or(0)
                    ),
                    signer: self.adapter.signers().to_vec(),
                });
            }
        }

        Ok(sort_by_idx(events))
    }

    fn decode_instruction(
        &self,
        inst_type: &PumpswapInstructionType,
        instruction: &ClassifiedInstruction,
        data: Vec<u8>,
    ) -> Result<PumpswapInstructionData, PumpfunError> {
        match inst_type {
            PumpswapInstructionType::Create => {
                let event = self.decode_create_instruction(instruction, data)?;
                Ok(PumpswapInstructionData::Create(event))
            }
            PumpswapInstructionType::Add => {
                let event = self.decode_add_instruction(instruction, data)?;
                Ok(PumpswapInstructionData::Add(event))
            }
            PumpswapInstructionType::Remove => {
                let event = self.decode_remove_instruction(instruction, data)?;
                Ok(PumpswapInstructionData::Remove(event))
            }
            PumpswapInstructionType::Buy => {
                let event = self.decode_buy_instruction(instruction, data)?;
                Ok(PumpswapInstructionData::Buy(event))
            }
            PumpswapInstructionType::Sell => {
                let event = self.decode_sell_instruction(instruction, data)?;
                Ok(PumpswapInstructionData::Sell(event))
            }
        }
    }

    fn decode_buy_instruction(
        &self,
        instruction: &ClassifiedInstruction,
        data: Vec<u8>,
    ) -> Result<PumpswapBuyEvent, PumpfunError> {
        let mut reader = BinaryReader::new(data);
        let accounts = &instruction.data.accounts;
        Ok(PumpswapBuyEvent {
            timestamp: reader.read_i64()? as u64,
            base_amount_out: reader.read_u64()?,
            max_quote_amount_in: reader.read_u64()?,
            user_base_token_reserves: reader.read_u64()?,
            user_quote_token_reserves: reader.read_u64()?,
            pool_base_token_reserves: reader.read_u64()?,
            pool_quote_token_reserves: reader.read_u64()?,
            quote_amount_in: reader.read_u64()?,
            lp_fee_basis_points: reader.read_u64()?,
            lp_fee: reader.read_u64()?,
            protocol_fee_basis_points: reader.read_u64()?,
            protocol_fee: reader.read_u64()?,
            quote_amount_in_with_lp_fee: reader.read_u64()?,
            user_quote_amount_in: reader.read_u64()?,
            pool: accounts.first().cloned().unwrap_or_default(),
            user: accounts.get(1).cloned().unwrap_or_default(),
            user_base_token_account: accounts.get(5).cloned().unwrap_or_default(),
            user_quote_token_account: accounts.get(6).cloned().unwrap_or_default(),
            protocol_fee_recipient: accounts.get(9).cloned().unwrap_or_default(),
            protocol_fee_recipient_token_account: accounts.get(10).cloned().unwrap_or_default(),
            coin_creator: accounts
                .get(11)
                .cloned()
                .unwrap_or_else(|| "11111111111111111111111111111111".to_string()),
            coin_creator_fee_basis_points: reader.read_u64().unwrap_or(0),
            coin_creator_fee: reader.read_u64().unwrap_or(0),
        })
    }

    fn decode_sell_instruction(
        &self,
        instruction: &ClassifiedInstruction,
        data: Vec<u8>,
    ) -> Result<PumpswapSellEvent, PumpfunError> {
        let mut reader = BinaryReader::new(data);
        let accounts = &instruction.data.accounts;
        Ok(PumpswapSellEvent {
            timestamp: reader.read_i64()? as u64,
            base_amount_in: reader.read_u64()?,
            min_quote_amount_out: reader.read_u64()?,
            user_base_token_reserves: reader.read_u64()?,
            user_quote_token_reserves: reader.read_u64()?,
            pool_base_token_reserves: reader.read_u64()?,
            pool_quote_token_reserves: reader.read_u64()?,
            quote_amount_out: reader.read_u64()?,
            lp_fee_basis_points: reader.read_u64()?,
            lp_fee: reader.read_u64()?,
            protocol_fee_basis_points: reader.read_u64()?,
            protocol_fee: reader.read_u64()?,
            quote_amount_out_without_lp_fee: reader.read_u64()?,
            user_quote_amount_out: reader.read_u64()?,
            pool: accounts.first().cloned().unwrap_or_default(),
            user: accounts.get(1).cloned().unwrap_or_default(),
            user_base_token_account: accounts.get(5).cloned().unwrap_or_default(),
            user_quote_token_account: accounts.get(6).cloned().unwrap_or_default(),
            protocol_fee_recipient: accounts.get(9).cloned().unwrap_or_default(),
            protocol_fee_recipient_token_account: accounts.get(10).cloned().unwrap_or_default(),
            coin_creator: accounts
                .get(11)
                .cloned()
                .unwrap_or_else(|| "11111111111111111111111111111111".to_string()),
            coin_creator_fee_basis_points: reader.read_u64().unwrap_or(0),
            coin_creator_fee: reader.read_u64().unwrap_or(0),
        })
    }

    fn decode_add_instruction(
        &self,
        instruction: &ClassifiedInstruction,
        data: Vec<u8>,
    ) -> Result<PumpswapDepositEvent, PumpfunError> {
        let mut reader = BinaryReader::new(data);
        let accounts = &instruction.data.accounts;
        Ok(PumpswapDepositEvent {
            timestamp: reader.read_i64()? as u64,
            lp_token_amount_out: reader.read_u64()?,
            max_base_amount_in: reader.read_u64()?,
            max_quote_amount_in: reader.read_u64()?,
            user_base_token_reserves: reader.read_u64()?,
            user_quote_token_reserves: reader.read_u64()?,
            pool_base_token_reserves: reader.read_u64()?,
            pool_quote_token_reserves: reader.read_u64()?,
            base_amount_in: reader.read_u64()?,
            quote_amount_in: reader.read_u64()?,
            lp_mint_supply: reader.read_u64()?,
            pool: accounts.first().cloned().unwrap_or_default(),
            user: accounts.get(2).cloned().unwrap_or_default(),
            user_base_token_account: accounts.get(6).cloned().unwrap_or_default(),
            user_quote_token_account: accounts.get(7).cloned().unwrap_or_default(),
            user_pool_token_account: accounts.get(8).cloned().unwrap_or_default(),
        })
    }

    fn decode_create_instruction(
        &self,
        instruction: &ClassifiedInstruction,
        data: Vec<u8>,
    ) -> Result<PumpswapCreatePoolEvent, PumpfunError> {
        let mut reader = BinaryReader::new(data);
        let accounts = &instruction.data.accounts;
        reader.read_u16()?; // consume padding index already accounted for in event parser
        Ok(PumpswapCreatePoolEvent {
            timestamp: reader.read_i64()? as u64,
            index: 0,
            creator: accounts.get(2).cloned().unwrap_or_default(),
            base_mint: accounts.get(3).cloned().unwrap_or_default(),
            quote_mint: accounts.get(4).cloned().unwrap_or_default(),
            base_mint_decimals: reader.read_u8()?,
            quote_mint_decimals: reader.read_u8()?,
            base_amount_in: reader.read_u64()?,
            quote_amount_in: reader.read_u64()?,
            pool_base_amount: reader.read_u64()?,
            pool_quote_amount: reader.read_u64()?,
            minimum_liquidity: reader.read_u64()?,
            initial_liquidity: reader.read_u64()?,
            lp_token_amount_out: reader.read_u64()?,
            pool_bump: reader.read_u8()?,
            pool: accounts.first().cloned().unwrap_or_default(),
            lp_mint: accounts.get(5).cloned().unwrap_or_default(),
            user_base_token_account: accounts.get(6).cloned().unwrap_or_default(),
            user_quote_token_account: accounts.get(7).cloned().unwrap_or_default(),
        })
    }

    fn decode_remove_instruction(
        &self,
        instruction: &ClassifiedInstruction,
        data: Vec<u8>,
    ) -> Result<PumpswapWithdrawEvent, PumpfunError> {
        let mut reader = BinaryReader::new(data);
        let accounts = &instruction.data.accounts;
        Ok(PumpswapWithdrawEvent {
            timestamp: reader.read_i64()? as u64,
            lp_token_amount_in: reader.read_u64()?,
            min_base_amount_out: reader.read_u64()?,
            min_quote_amount_out: reader.read_u64()?,
            user_base_token_reserves: reader.read_u64()?,
            user_quote_token_reserves: reader.read_u64()?,
            pool_base_token_reserves: reader.read_u64()?,
            pool_quote_token_reserves: reader.read_u64()?,
            base_amount_out: reader.read_u64()?,
            quote_amount_out: reader.read_u64()?,
            lp_mint_supply: reader.read_u64()?,
            pool: accounts.first().cloned().unwrap_or_default(),
            user: accounts.get(2).cloned().unwrap_or_default(),
            user_base_token_account: accounts.get(6).cloned().unwrap_or_default(),
            user_quote_token_account: accounts.get(7).cloned().unwrap_or_default(),
            user_pool_token_account: accounts.get(8).cloned().unwrap_or_default(),
        })
    }
}

impl HasIdx for PumpswapInstruction {
    fn idx(&self) -> &str {
        &self.idx
    }
}
