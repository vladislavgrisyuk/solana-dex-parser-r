use crate::core::transaction_adapter::TransactionAdapter;
use crate::types::{MemeEvent, TradeType, TransferMap};

use super::MemeEventParser;

pub struct SimpleMemeParser {
    adapter: TransactionAdapter,
    transfer_actions: TransferMap,
}

impl SimpleMemeParser {
    pub fn new(adapter: TransactionAdapter, transfer_actions: TransferMap) -> Self {
        Self {
            adapter,
            transfer_actions,
        }
    }

    pub fn boxed(
        adapter: TransactionAdapter,
        transfer_actions: TransferMap,
    ) -> Box<dyn MemeEventParser> {
        Box::new(Self::new(adapter, transfer_actions))
    }
}

impl MemeEventParser for SimpleMemeParser {
    fn process_events(&mut self) -> Vec<MemeEvent> {
        self.transfer_actions
            .values()
            .flat_map(|transfers| transfers.iter())
            .map(|transfer| MemeEvent {
                event_type: TradeType::Swap,
                timestamp: transfer.timestamp,
                idx: transfer.idx.clone(),
                slot: self.adapter.slot(),
                signature: transfer.signature.clone(),
                user: transfer.info.source.clone(),
                base_mint: transfer.info.mint.clone(),
                quote_mint: transfer.info.mint.clone(),
                input_token: None,
                output_token: None,
                name: None,
                symbol: None,
                uri: None,
                decimals: Some(transfer.info.token_amount.decimals),
                total_supply: None,
                fee: None,
                protocol_fee: None,
                platform_fee: None,
                share_fee: None,
                creator_fee: None,
                protocol: Some(transfer.program_id.clone()),
                platform_config: None,
                creator: transfer.info.authority.clone(),
                bonding_curve: None,
                pool: None,
                pool_dex: None,
                pool_a_reserve: None,
                pool_b_reserve: None,
                pool_fee_rate: None,
            })
            .collect()
    }
}
