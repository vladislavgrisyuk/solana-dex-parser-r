use crate::core::transaction_adapter::TransactionAdapter;
use crate::types::{ClassifiedInstruction, DexInfo, TransferData, TransferMap};

use super::TransferParser;

pub struct SimpleTransferParser {
    adapter: TransactionAdapter,
    dex_info: DexInfo,
    transfer_actions: TransferMap,
    classified_instructions: Vec<ClassifiedInstruction>,
}

impl SimpleTransferParser {
    pub fn new(
        adapter: TransactionAdapter,
        dex_info: DexInfo,
        transfer_actions: TransferMap,
        classified_instructions: Vec<ClassifiedInstruction>,
    ) -> Self {
        Self {
            adapter,
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
    ) -> Box<dyn TransferParser> {
        Box::new(Self::new(
            adapter,
            dex_info,
            transfer_actions,
            classified_instructions,
        ))
    }
}

impl TransferParser for SimpleTransferParser {
    fn process_transfers(&mut self) -> Vec<TransferData> {
        if let Some(program_id) = self.dex_info.program_id.clone() {
            return self
                .transfer_actions
                .get(&program_id)
                .cloned()
                .unwrap_or_default();
        }

        self.classified_instructions
            .first()
            .and_then(|instruction| self.transfer_actions.get(&instruction.program_id))
            .cloned()
            .unwrap_or_else(|| self.adapter.transfers().to_vec())
    }
}
