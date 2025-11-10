use crate::core::constants::dex_program_names;
use crate::core::transaction_adapter::TransactionAdapter;
use crate::types::{ClassifiedInstruction, PoolEvent, TradeType, TransferMap};

use super::LiquidityParser;

pub struct SimpleLiquidityParser {
    adapter: TransactionAdapter,
    transfer_actions: TransferMap,
    classified_instructions: Vec<ClassifiedInstruction>,
}

impl SimpleLiquidityParser {
    pub fn new(
        adapter: TransactionAdapter,
        transfer_actions: TransferMap,
        classified_instructions: Vec<ClassifiedInstruction>,
    ) -> Self {
        Self {
            adapter,
            transfer_actions,
            classified_instructions,
        }
    }

    pub fn boxed(
        adapter: TransactionAdapter,
        transfer_actions: TransferMap,
        classified_instructions: Vec<ClassifiedInstruction>,
    ) -> Box<dyn LiquidityParser> {
        Box::new(Self::new(
            adapter,
            transfer_actions,
            classified_instructions,
        ))
    }
}

impl LiquidityParser for SimpleLiquidityParser {
    fn process_liquidity(&mut self) -> Vec<PoolEvent> {
        self.classified_instructions
            .iter()
            .map(|instruction| {
                let liquidity: f64 = self
                    .transfer_actions
                    .get(&instruction.program_id)
                    .map(|transfers| {
                        transfers
                            .iter()
                            .map(|t| {
                                t.info.token_amount.ui_amount.unwrap_or_else(|| {
                                    t.info.token_amount.amount.parse::<f64>().unwrap_or(0.0)
                                })
                            })
                            .sum()
                    })
                    .unwrap_or(0.0);

                let idx = format!(
                    "{}-{}",
                    instruction.outer_index,
                    instruction.inner_index.unwrap_or(0)
                );

                let pool_id = instruction
                    .data
                    .accounts
                    .first()
                    .cloned()
                    .unwrap_or_default();
                let token1 = instruction.data.accounts.get(1).cloned();

                PoolEvent {
                    user: self.adapter.signer(),
                    event_type: TradeType::Add,
                    program_id: Some(instruction.program_id.clone()),
                    amm: Some(dex_program_names::name(&instruction.program_id).to_string()),
                    slot: self.adapter.slot(),
                    timestamp: self.adapter.block_time(),
                    signature: self.adapter.signature().to_string(),
                    idx,
                    signer: Some(self.adapter.signers().to_vec()),
                    pool_id,
                    config: None,
                    pool_lp_mint: token1.clone(),
                    token0_mint: Some(
                        instruction
                            .data
                            .accounts
                            .first()
                            .cloned()
                            .unwrap_or_default(),
                    ),
                    token0_amount: Some(liquidity),
                    token0_amount_raw: Some(liquidity.to_string()),
                    token0_balance_change: None,
                    token0_decimals: None,
                    token1_mint: token1,
                    token1_amount: None,
                    token1_amount_raw: None,
                    token1_balance_change: None,
                    token1_decimals: None,
                    lp_amount: None,
                    lp_amount_raw: None,
                }
            })
            .collect()
    }
}
