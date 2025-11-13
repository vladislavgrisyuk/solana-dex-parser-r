use crate::core::transaction_adapter::TransactionAdapter;
use crate::protocols::simple::TradeParser;
use crate::types::{ClassifiedInstruction, DexInfo, TradeInfo, TransferMap};

use super::constants::program_names;
use super::meteora_dbc_event_parser::MeteoraDBCEventParser;

pub struct MeteoraDBCParser {
    adapter: TransactionAdapter,
    dex_info: DexInfo,
    transfer_actions: TransferMap,
    classified_instructions: Vec<ClassifiedInstruction>,
    event_parser: MeteoraDBCEventParser,
}

impl MeteoraDBCParser {
    pub fn new(
        adapter: TransactionAdapter,
        dex_info: DexInfo,
        transfer_actions: TransferMap,
        classified_instructions: Vec<ClassifiedInstruction>,
    ) -> Self {
        let event_parser = MeteoraDBCEventParser::new(adapter.clone(), transfer_actions.clone());
        Self {
            adapter,
            dex_info,
            transfer_actions,
            classified_instructions,
            event_parser,
        }
    }

    fn create_trade_info(&self, event: &crate::types::MemeEvent) -> TradeInfo {
        TradeInfo {
            trade_type: event.event_type.clone(),
            pool: event.pool.as_ref().map(|p| vec![p.clone()]).unwrap_or_default(),
            input_token: event.input_token.clone().unwrap_or_default(),
            output_token: event.output_token.clone().unwrap_or_default(),
            slippage_bps: None,
            fee: None,
            fees: Vec::new(),
            user: Some(event.user.clone()),
            program_id: self.dex_info.program_id.clone(),
            amm: Some(program_names::METEORA_DBC.to_string()),
            amms: Some(vec![program_names::METEORA_DBC.to_string()]),
            route: self.dex_info.route.clone(),
            slot: event.slot,
            timestamp: event.timestamp,
            signature: event.signature.clone(),
            idx: event.idx.clone(),
            signer: None,
        }
    }
}

impl TradeParser for MeteoraDBCParser {
    fn process_trades(&mut self) -> Vec<TradeInfo> {
        let events = self.event_parser.parse_instructions(&self.classified_instructions);
        
        events
            .into_iter()
            .filter(|event| {
                matches!(
                    event.event_type,
                    crate::types::TradeType::Buy | crate::types::TradeType::Sell | crate::types::TradeType::Swap
                )
            })
            .map(|event| {
                let trade = self.create_trade_info(&event);
                // Прикрепляем token transfer info
                self.event_parser.get_utils().attach_token_transfer_info(trade, &self.transfer_actions)
            })
            .collect()
    }
}

