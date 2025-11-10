use crate::core::transaction_adapter::TransactionAdapter;
use crate::core::transaction_utils::TransactionUtils;
use crate::types::{ClassifiedInstruction, DexInfo, TradeInfo, TransferMap};

use super::TradeParser;

pub struct SimpleTradeParser {
    utils: TransactionUtils,
    dex_info: DexInfo,
    transfer_actions: TransferMap,
    classified_instructions: Vec<ClassifiedInstruction>,
}

impl SimpleTradeParser {
    pub fn new(
        adapter: TransactionAdapter,
        dex_info: DexInfo,
        transfer_actions: TransferMap,
        classified_instructions: Vec<ClassifiedInstruction>,
    ) -> Self {
        Self {
            utils: TransactionUtils::new(adapter),
            dex_info,
            transfer_actions,
            classified_instructions,
        }
    }

    pub fn boxed(
        adapter: TransactionAdapter,
        dex_info: DexInfo,
        transfer_actions: TransferMap,
        classified_instructions: Vec<ClassifiedInstruction>,
    ) -> Box<dyn TradeParser> {
        Box::new(Self::new(
            adapter,
            dex_info,
            transfer_actions,
            classified_instructions,
        ))
    }
}

impl TradeParser for SimpleTradeParser {
    fn process_trades(&mut self) -> Vec<TradeInfo> {
        let mut trades = Vec::new();
        if let Some(program_id) = self.dex_info.program_id.clone() {
            if let Some(transfers) = self.transfer_actions.get(&program_id) {
                if let Some(trade) = self.utils.process_swap_data(transfers, &self.dex_info) {
                    trades.push(trade);
                }
            }
        } else if let Some(first) = self.classified_instructions.first() {
            if let Some(transfers) = self.transfer_actions.get(&first.program_id) {
                if let Some(trade) = self.utils.process_swap_data(transfers, &self.dex_info) {
                    trades.push(trade);
                }
            }
        }
        trades
    }
}
