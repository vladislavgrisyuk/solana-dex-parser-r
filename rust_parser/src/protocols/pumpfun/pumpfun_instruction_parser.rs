use crate::core::transaction_adapter::TransactionAdapter;
use crate::types::ClassifiedInstruction;

use super::binary_reader::BinaryReader;
use super::constants::discriminators::pumpfun_instructions;
use super::error::PumpfunError;
use super::util::{get_instruction_data, sort_by_idx, HasIdx};

#[derive(Clone, Debug, PartialEq)]
pub enum PumpfunInstructionType {
    Create,
    Migrate,
    Buy,
    Sell,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PumpfunInstructionData {
    Create(PumpfunCreateInstruction),
    Migrate(PumpfunMigrateInstruction),
    Buy(PumpfunTradeInstruction),
    Sell(PumpfunTradeInstruction),
}

#[derive(Clone, Debug, PartialEq)]
pub struct PumpfunInstruction {
    pub instruction_type: PumpfunInstructionType,
    pub data: PumpfunInstructionData,
    pub slot: u64,
    pub timestamp: u64,
    pub signature: String,
    pub idx: String,
    pub signer: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PumpfunTradeInstruction {
    pub mint: String,
    pub bonding_curve: String,
    pub token_amount: u64,
    pub sol_amount: u64,
    pub user: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PumpfunCreateInstruction {
    pub name: String,
    pub symbol: String,
    pub uri: String,
    pub mint: String,
    pub bonding_curve: String,
    pub user: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PumpfunMigrateInstruction {
    pub mint: String,
    pub bonding_curve: String,
    pub user: String,
    pub pool_mint: String,
    pub quote_mint: String,
    pub lp_mint: String,
    pub user_pool_token_account: String,
    pub pool_base_token_account: String,
    pub pool_quote_token_account: String,
}

pub struct PumpfunInstructionParser {
    adapter: TransactionAdapter,
}

impl PumpfunInstructionParser {
    pub fn new(adapter: TransactionAdapter) -> Self {
        Self { adapter }
    }

    pub fn parse_instructions(
        &self,
        instructions: &[ClassifiedInstruction],
    ) -> Result<Vec<PumpfunInstruction>, PumpfunError> {
        let mut events = Vec::new();
        for instruction in instructions {
            let data = get_instruction_data(&instruction.data)?;
            if data.len() < 8 {
                continue;
            }
            let discriminator = &data[..8];
            let payload = data[8..].to_vec();
            let parsed = if discriminator == pumpfun_instructions::CREATE {
                Some(PumpfunInstructionType::Create)
            } else if discriminator == pumpfun_instructions::MIGRATE {
                Some(PumpfunInstructionType::Migrate)
            } else if discriminator == pumpfun_instructions::BUY {
                Some(PumpfunInstructionType::Buy)
            } else if discriminator == pumpfun_instructions::SELL {
                Some(PumpfunInstructionType::Sell)
            } else {
                None
            };

            if let Some(inst_type) = parsed {
                let data = self.decode_instruction(&inst_type, instruction, payload)?;
                events.push(PumpfunInstruction {
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
        inst_type: &PumpfunInstructionType,
        instruction: &ClassifiedInstruction,
        data: Vec<u8>,
    ) -> Result<PumpfunInstructionData, PumpfunError> {
        match inst_type {
            PumpfunInstructionType::Buy => {
                let data = self.decode_trade_instruction(instruction, data)?;
                Ok(PumpfunInstructionData::Buy(data))
            }
            PumpfunInstructionType::Sell => {
                let data = self.decode_trade_instruction(instruction, data)?;
                Ok(PumpfunInstructionData::Sell(data))
            }
            PumpfunInstructionType::Create => {
                let data = self.decode_create_instruction(instruction, data)?;
                Ok(PumpfunInstructionData::Create(data))
            }
            PumpfunInstructionType::Migrate => {
                let data = self.decode_migrate_instruction(instruction)?;
                Ok(PumpfunInstructionData::Migrate(data))
            }
        }
    }

    fn decode_trade_instruction(
        &self,
        instruction: &ClassifiedInstruction,
        data: Vec<u8>,
    ) -> Result<PumpfunTradeInstruction, PumpfunError> {
        let mut reader = BinaryReader::new(data);
        let accounts = &instruction.data.accounts;
        Ok(PumpfunTradeInstruction {
            mint: accounts.get(2).cloned().unwrap_or_default(),
            bonding_curve: accounts.get(3).cloned().unwrap_or_default(),
            token_amount: reader.read_u64()?,
            sol_amount: reader.read_u64()?,
            user: accounts.get(6).cloned().unwrap_or_default(),
        })
    }

    fn decode_create_instruction(
        &self,
        instruction: &ClassifiedInstruction,
        data: Vec<u8>,
    ) -> Result<PumpfunCreateInstruction, PumpfunError> {
        let mut reader = BinaryReader::new(data);
        let accounts = &instruction.data.accounts;
        Ok(PumpfunCreateInstruction {
            name: reader.read_string()?,
            symbol: reader.read_string()?,
            uri: reader.read_string()?,
            mint: accounts.first().cloned().unwrap_or_default(),
            bonding_curve: accounts.get(2).cloned().unwrap_or_default(),
            user: accounts.get(7).cloned().unwrap_or_default(),
        })
    }

    fn decode_migrate_instruction(
        &self,
        instruction: &ClassifiedInstruction,
    ) -> Result<PumpfunMigrateInstruction, PumpfunError> {
        let accounts = &instruction.data.accounts;
        Ok(PumpfunMigrateInstruction {
            mint: accounts.get(2).cloned().unwrap_or_default(),
            bonding_curve: accounts.get(3).cloned().unwrap_or_default(),
            user: accounts.get(5).cloned().unwrap_or_default(),
            pool_mint: accounts.get(9).cloned().unwrap_or_default(),
            quote_mint: accounts.get(4).cloned().unwrap_or_default(),
            lp_mint: accounts.get(15).cloned().unwrap_or_default(),
            user_pool_token_account: accounts.get(16).cloned().unwrap_or_default(),
            pool_base_token_account: accounts.get(17).cloned().unwrap_or_default(),
            pool_quote_token_account: accounts.get(18).cloned().unwrap_or_default(),
        })
    }
}

impl HasIdx for PumpfunInstruction {
    fn idx(&self) -> &str {
        &self.idx
    }
}
