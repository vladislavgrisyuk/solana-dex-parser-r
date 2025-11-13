use crate::core::instruction_classifier::InstructionClassifier;
use crate::core::transaction_adapter::TransactionAdapter;
use crate::protocols::simple::{MemeEventParser, TradeParser};
use crate::types::{ClassifiedInstruction, DexInfo, MemeEvent, TradeInfo, TradeType, TransferMap};

use super::constants::PUMP_FUN_PROGRAM_ID;
use super::error::PumpfunError;
use super::pumpfun_event_parser::PumpfunEventParser;
use super::util::{attach_token_transfers, get_pumpfun_trade_info};

pub struct PumpfunParser {
    adapter: TransactionAdapter,
    dex_info: DexInfo,
    transfer_actions: TransferMap,
    classified_instructions: Vec<ClassifiedInstruction>,
    event_parser: PumpfunEventParser,
}

impl PumpfunParser {
    pub fn new(
        adapter: TransactionAdapter,
        dex_info: DexInfo,
        transfer_actions: TransferMap,
        classified_instructions: Vec<ClassifiedInstruction>,
    ) -> Self {
        // Оптимизация: event_parser не хранит адаптер, передаем по ссылке при вызове
        let event_parser = PumpfunEventParser::new();
        Self {
            adapter,
            dex_info,
            transfer_actions,
            classified_instructions,
            event_parser,
        }
    }

    fn parse_events(&self) -> Result<Vec<MemeEvent>, PumpfunError> {
        self.event_parser
            .parse_instructions(&self.adapter, &self.classified_instructions)
    }
}

impl TradeParser for PumpfunParser {
    fn process_trades(&mut self) -> Vec<TradeInfo> {
        let parse_result = self.parse_events();
        
        match parse_result {
            Ok(events) => {
                // ОПТИМИЗАЦИЯ: объединяем фильтрацию и маппинг в один проход
                // Убираем промежуточный вектор filtered_events
                events
                    .into_iter()
                    .filter(|e| matches!(e.event_type, TradeType::Buy | TradeType::Sell))
                    .map(|event| {
                        let trade = get_pumpfun_trade_info(&event, &self.adapter, &self.dex_info);
                        attach_token_transfers(&self.adapter, trade, &self.transfer_actions)
                    })
                    .collect()
            },
            Err(_) => Vec::new()
        }
    }
}

pub struct PumpfunMemeParser {
    adapter: TransactionAdapter,
    _transfer_actions: TransferMap,
}

impl PumpfunMemeParser {
    pub fn new(adapter: TransactionAdapter, transfer_actions: TransferMap) -> Self {
        Self {
            adapter,
            _transfer_actions: transfer_actions,
        }
    }
}

impl MemeEventParser for PumpfunMemeParser {
    fn process_events(&mut self) -> Vec<MemeEvent> {
        let classifier = InstructionClassifier::new(&self.adapter);
        let instructions = classifier.get_instructions(PUMP_FUN_PROGRAM_ID);
        // Оптимизация: не клонируем адаптер, передаем по ссылке
        let parser = PumpfunEventParser::new();
        match parser.parse_instructions(&self.adapter, &instructions) {
            Ok(events) => events,
            Err(err) => {
                tracing::error!("failed to parse pumpfun meme events: {err}");
                Vec::new()
            }
        }
    }
}
