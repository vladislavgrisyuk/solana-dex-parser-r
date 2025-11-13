use std::collections::HashMap;

use crate::core::transaction_adapter::TransactionAdapter;
use crate::protocols::simple::TradeParser;
use crate::types::{ClassifiedInstruction, DexInfo, TradeInfo, TransferMap};

use super::pumpswap_event_parser::{
    PumpswapEvent, PumpswapEventData, PumpswapEventParser, PumpswapEventType,
};
use super::util::{attach_token_transfers, build_pumpswap_buy_trade, build_pumpswap_sell_trade};

pub struct PumpswapParser {
    adapter: TransactionAdapter,
    dex_info: DexInfo,
    transfer_actions: TransferMap,
    classified_instructions: Vec<ClassifiedInstruction>,
    event_parser: PumpswapEventParser,
}

impl PumpswapParser {
    pub fn new(
        adapter: TransactionAdapter,
        dex_info: DexInfo,
        transfer_actions: TransferMap,
        classified_instructions: Vec<ClassifiedInstruction>,
    ) -> Self {
        // Event parser больше не хранит адаптер - это статическая структура
        let event_parser = PumpswapEventParser::new();
        Self {
            adapter,
            dex_info,
            transfer_actions,
            classified_instructions,
            event_parser,
        }
    }

    fn parse_events(&self) -> Vec<PumpswapEvent> {
        if self.classified_instructions.is_empty() {
            return Vec::new();
        }
        
        let parse_result = self.event_parser.parse_instructions(&self.adapter, &self.classified_instructions);
        
        match parse_result {
            Ok(events) => {
                // Оптимизация: предварительно резервируем capacity для фильтрованных событий
                // Обычно все события являются Buy/Sell, поэтому резервируем полный размер
                let events_count = events.len();
                let mut filtered = Vec::with_capacity(events_count);
                
                // Оптимизация: используем matches! макрос для быстрой проверки
                for event in events {
                    if matches!(event.event_type, PumpswapEventType::Buy | PumpswapEventType::Sell) {
                        filtered.push(event);
                    }
                }
                filtered
            }
            Err(_) => Vec::new()
        }
    }

    #[inline]
    fn decimals_or(&self, mint: &str, default_decimals: u8) -> u8 {
        let d = self.adapter.get_token_decimals(mint);
        if d == 0 { default_decimals } else { d }
    }

    /// Создает трейд для BUY события (оптимизированная версия с кэшированными картами)
    fn create_buy_trade_cached(
        &self,
        event: &PumpswapEvent,
        buy: &super::pumpswap_event_parser::PumpswapBuyEvent,
        post_balance_map: &HashMap<&str, &crate::types::TokenBalance>,
        pre_balance_map: &HashMap<&str, &crate::types::TokenBalance>,
        transfer_map: &HashMap<&str, &crate::types::TransferData>,
    ) -> Option<TradeInfo> {
        // Оптимизация: Get token info - try spl_token_map first, then fallback to cached maps
        let input_info = self.adapter
            .token_account_info(&buy.user_quote_token_account)
            .cloned()
            .or_else(|| {
                post_balance_map.get(buy.user_quote_token_account.as_str())
                    .map(|b| {
                        crate::types::TokenInfo {
                            mint: b.mint.clone(),
                            amount: b.ui_token_amount.ui_amount.unwrap_or(0.0),
                            amount_raw: b.ui_token_amount.amount.clone(),
                            decimals: b.ui_token_amount.decimals,
                            ..Default::default()
                        }
                    })
            })
            .or_else(|| {
                pre_balance_map.get(buy.user_quote_token_account.as_str())
                    .map(|b| {
                        crate::types::TokenInfo {
                            mint: b.mint.clone(),
                            amount: b.ui_token_amount.ui_amount.unwrap_or(0.0),
                            amount_raw: b.ui_token_amount.amount.clone(),
                            decimals: b.ui_token_amount.decimals,
                            ..Default::default()
                        }
                    })
            })
            .or_else(|| {
                transfer_map.get(buy.user_quote_token_account.as_str())
                    .map(|t| {
                        crate::types::TokenInfo {
                            mint: t.info.mint.clone(),
                            amount: t.info.token_amount.ui_amount.unwrap_or(0.0),
                            amount_raw: t.info.token_amount.amount.clone(),
                            decimals: t.info.token_amount.decimals,
                            ..Default::default()
                        }
                    })
            });
        
        let input_info = match input_info {
            Some(info) => info,
            None => {
                // Оптимизация: минимум логирования, быстрый fallback
                // Try to find mint from transfers using cached map
                let inferred_mint = transfer_map.get(buy.user_quote_token_account.as_str())
                    .map(|t| t.info.mint.clone())
                    .or_else(|| {
                        // Try to infer from other quote token accounts (SOL, USDC, USDT are common quote tokens)
                        post_balance_map.values()
                            .find(|b| {
                                // Common quote tokens - используем статические строки для сравнения
                                b.mint == "So11111111111111111111111111111111111111112" || // SOL
                                b.mint == "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v" || // USDC
                                b.mint == "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB" // USDT
                            })
                            .map(|b| b.mint.clone())
                    });
                
                if let Some(mint) = inferred_mint {
                    let decimals = self.adapter.get_token_decimals(&mint);
                    crate::types::TokenInfo {
                        mint,
                        amount: 0.0,
                        amount_raw: "0".to_string(),
                        decimals: if decimals > 0 { decimals } else { 6 }, // Default to 6 for quote tokens
                        ..Default::default()
                    }
                } else {
                    return None;
                }
            }
        };
        
        // Оптимизация: минимум логирования для скорости
        let output_info = self.adapter
            .token_account_info(&buy.user_base_token_account)
            .cloned()
            .or_else(|| {
                post_balance_map.get(buy.user_base_token_account.as_str())
                    .map(|b| {
                        crate::types::TokenInfo {
                            mint: b.mint.clone(),
                            amount: b.ui_token_amount.ui_amount.unwrap_or(0.0),
                            amount_raw: b.ui_token_amount.amount.clone(),
                            decimals: b.ui_token_amount.decimals,
                            ..Default::default()
                        }
                    })
            })
            .or_else(|| {
                pre_balance_map.get(buy.user_base_token_account.as_str())
                    .map(|b| {
                        crate::types::TokenInfo {
                            mint: b.mint.clone(),
                            amount: b.ui_token_amount.ui_amount.unwrap_or(0.0),
                            amount_raw: b.ui_token_amount.amount.clone(),
                            decimals: b.ui_token_amount.decimals,
                            ..Default::default()
                        }
                    })
            })
            .or_else(|| {
                transfer_map.get(buy.user_base_token_account.as_str())
                    .map(|t| {
                        crate::types::TokenInfo {
                            mint: t.info.mint.clone(),
                            amount: t.info.token_amount.ui_amount.unwrap_or(0.0),
                            amount_raw: t.info.token_amount.amount.clone(),
                            decimals: t.info.token_amount.decimals,
                            ..Default::default()
                        }
                    })
            })
            .or_else(|| {
                // Last resort: try to find mint from other token accounts in the transaction
                // For BUY, output is base token. Try to find base mint from pool or other accounts
                post_balance_map.values()
                    .find(|b| {
                        // Try to find pool base token account or other base token accounts
                        b.account != buy.user_quote_token_account && 
                        b.account != buy.protocol_fee_recipient_token_account &&
                        b.mint != input_info.mint // Not the quote mint
                    })
                    .map(|b| {
                        crate::types::TokenInfo {
                            mint: b.mint.clone(),
                            amount: 0.0,
                            amount_raw: "0".to_string(),
                            decimals: b.ui_token_amount.decimals,
                            ..Default::default()
                        }
                    })
            });
        
        let output_info = match output_info {
            Some(info) => info,
            None => {
                // Оптимизация: быстрый fallback без избыточного логирования
                let inferred_mint = transfer_map.get(buy.user_base_token_account.as_str())
                    .map(|t| t.info.mint.clone());
                
                if let Some(mint) = inferred_mint {
                    let decimals = self.adapter.get_token_decimals(&mint);
                    crate::types::TokenInfo {
                        mint,
                        amount: 0.0,
                        amount_raw: "0".to_string(),
                        decimals: if decimals > 0 { decimals } else { 6 }, // Default to 6 for base tokens
                        ..Default::default()
                    }
                } else {
                    return None;
                }
            }
        };
        
        // Оптимизация: fee lookup без избыточного логирования
        let fee_info = self.adapter
            .token_account_info(&buy.protocol_fee_recipient_token_account)
            .cloned()
            .or_else(|| {
                post_balance_map.get(buy.protocol_fee_recipient_token_account.as_str())
                    .map(|b| {
                        crate::types::TokenInfo {
                            mint: b.mint.clone(),
                            amount: b.ui_token_amount.ui_amount.unwrap_or(0.0),
                            amount_raw: b.ui_token_amount.amount.clone(),
                            decimals: b.ui_token_amount.decimals,
                            ..Default::default()
                        }
                    })
            })
            .unwrap_or_else(|| {
                // Fee token might not be in balances, use input token decimals as fallback
                crate::types::TokenInfo {
                    mint: input_info.mint.clone(),
                    amount: 0.0,
                    amount_raw: "0".to_string(),
                    decimals: input_info.decimals,
                    ..Default::default()
                }
            });

        let input_decimals = self.decimals_or(&input_info.mint, input_info.decimals);
        let output_decimals = self.decimals_or(&output_info.mint, output_info.decimals);
        let fee_decimals = self.decimals_or(&fee_info.mint, fee_info.decimals);

        let trade = build_pumpswap_buy_trade(
            event,
            buy,
            (&input_info.mint, input_decimals),
            (&output_info.mint, output_decimals),
            (&fee_info.mint, fee_decimals),
            &self.dex_info,
        );

        Some(attach_token_transfers(
            &self.adapter,
            trade,
            &self.transfer_actions,
        ))
    }

    /// Создает трейд для SELL события (оптимизированная версия с кэшированными картами)
    fn create_sell_trade_cached(
        &self,
        event: &PumpswapEvent,
        sell: &super::pumpswap_event_parser::PumpswapSellEvent,
        post_balance_map: &HashMap<&str, &crate::types::TokenBalance>,
        pre_balance_map: &HashMap<&str, &crate::types::TokenBalance>,
        transfer_map: &HashMap<&str, &crate::types::TransferData>,
    ) -> Option<TradeInfo> {
        // Get token info - try spl_token_map first, then fallback to cached maps
        let input_info = self.adapter
            .token_account_info(&sell.user_base_token_account)
            .cloned()
            .or_else(|| {
                post_balance_map.get(sell.user_base_token_account.as_str())
                    .map(|b| {
                        crate::types::TokenInfo {
                            mint: b.mint.clone(),
                            amount: b.ui_token_amount.ui_amount.unwrap_or(0.0),
                            amount_raw: b.ui_token_amount.amount.clone(),
                            decimals: b.ui_token_amount.decimals,
                            ..Default::default()
                        }
                    })
            })
            .or_else(|| {
                pre_balance_map.get(sell.user_base_token_account.as_str())
                    .map(|b| {
                        crate::types::TokenInfo {
                            mint: b.mint.clone(),
                            amount: b.ui_token_amount.ui_amount.unwrap_or(0.0),
                            amount_raw: b.ui_token_amount.amount.clone(),
                            decimals: b.ui_token_amount.decimals,
                            ..Default::default()
                        }
                    })
            })
            .or_else(|| {
                transfer_map.get(sell.user_base_token_account.as_str())
                    .map(|t| {
                        crate::types::TokenInfo {
                            mint: t.info.mint.clone(),
                            amount: t.info.token_amount.ui_amount.unwrap_or(0.0),
                            amount_raw: t.info.token_amount.amount.clone(),
                            decimals: t.info.token_amount.decimals,
                            ..Default::default()
                        }
                    })
            });
        
        let input_info = match input_info {
            Some(info) => info,
            None => {
                // Try to infer from other base token accounts using cached maps
                // For SELL, input is base token. Look for base mint from other accounts
                let inferred_mint = transfer_map.get(sell.user_base_token_account.as_str())
                    .map(|t| t.info.mint.clone())
                    .or_else(|| {
                        // Try to find from post_token_balances using cached map
                        post_balance_map.values()
                            .find(|b| {
                                // Try to find a base token account (not SOL, USDC, USDT)
                                !b.account.is_empty() && 
                                !b.mint.is_empty() &&
                                b.mint != "So11111111111111111111111111111111111111112" && // Not SOL
                                b.mint != "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v" && // Not USDC
                                b.mint != "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB" && // Not USDT
                                b.account != sell.user_quote_token_account &&
                                b.account != sell.protocol_fee_recipient_token_account
                            })
                            .map(|b| b.mint.clone())
                            .or_else(|| {
                                // Try to find from pre_token_balances using cached map
                                pre_balance_map.values()
                                    .find(|b| {
                                        !b.account.is_empty() && 
                                        !b.mint.is_empty() &&
                                        b.mint != "So11111111111111111111111111111111111111112" &&
                                        b.mint != "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v" &&
                                        b.mint != "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB"
                                    })
                                    .map(|b| b.mint.clone())
                            })
                    });
                
                if let Some(mint) = inferred_mint {
                    let decimals = self.adapter.get_token_decimals(&mint);
                    crate::types::TokenInfo {
                        mint,
                        amount: 0.0,
                        amount_raw: "0".to_string(),
                        decimals: if decimals > 0 { decimals } else { 6 }, // Default to 6 for base tokens
                        ..Default::default()
                    }
                } else {
                    return None;
                }
            }
        };
        
        let output_info = self.adapter
            .token_account_info(&sell.user_quote_token_account)
            .cloned()
            .or_else(|| {
                post_balance_map.get(sell.user_quote_token_account.as_str())
                    .map(|b| {
                        crate::types::TokenInfo {
                            mint: b.mint.clone(),
                            amount: b.ui_token_amount.ui_amount.unwrap_or(0.0),
                            amount_raw: b.ui_token_amount.amount.clone(),
                            decimals: b.ui_token_amount.decimals,
                            ..Default::default()
                        }
                    })
            })
            .or_else(|| {
                pre_balance_map.get(sell.user_quote_token_account.as_str())
                    .map(|b| {
                        crate::types::TokenInfo {
                            mint: b.mint.clone(),
                            amount: b.ui_token_amount.ui_amount.unwrap_or(0.0),
                            amount_raw: b.ui_token_amount.amount.clone(),
                            decimals: b.ui_token_amount.decimals,
                            ..Default::default()
                        }
                    })
            })
            .or_else(|| {
                transfer_map.get(sell.user_quote_token_account.as_str())
                    .map(|t| {
                        crate::types::TokenInfo {
                            mint: t.info.mint.clone(),
                            amount: t.info.token_amount.ui_amount.unwrap_or(0.0),
                            amount_raw: t.info.token_amount.amount.clone(),
                            decimals: t.info.token_amount.decimals,
                            ..Default::default()
                        }
                    })
            });
        
        let output_info = match output_info {
            Some(info) => info,
            None => {
                return None;
            }
        };
        
        let fee_info = self.adapter
            .token_account_info(&sell.protocol_fee_recipient_token_account)
            .cloned()
            .or_else(|| {
                post_balance_map.get(sell.protocol_fee_recipient_token_account.as_str())
                    .map(|b| {
                        crate::types::TokenInfo {
                            mint: b.mint.clone(),
                            amount: b.ui_token_amount.ui_amount.unwrap_or(0.0),
                            amount_raw: b.ui_token_amount.amount.clone(),
                            decimals: b.ui_token_amount.decimals,
                            ..Default::default()
                        }
                    })
            })
            .or_else(|| {
                transfer_map.get(sell.protocol_fee_recipient_token_account.as_str())
                    .map(|t| {
                        crate::types::TokenInfo {
                            mint: t.info.mint.clone(),
                            amount: 0.0,
                            amount_raw: "0".to_string(),
                            decimals: t.info.token_amount.decimals,
                            ..Default::default()
                        }
                    })
            })
            .unwrap_or_else(|| {
                // Use input token as fallback for fee
                crate::types::TokenInfo {
                    mint: input_info.mint.clone(),
                    amount: 0.0,
                    amount_raw: "0".to_string(),
                    decimals: input_info.decimals,
                    ..Default::default()
                }
            });

        let input_decimals = self.decimals_or(&input_info.mint, input_info.decimals);
        let output_decimals = self.decimals_or(&output_info.mint, output_info.decimals);
        let fee_decimals = self.decimals_or(&fee_info.mint, fee_info.decimals);

        let trade = build_pumpswap_sell_trade(
            event,
            sell,
            (&input_info.mint, input_decimals),
            (&output_info.mint, output_decimals),
            (&fee_info.mint, fee_decimals),
            &self.dex_info,
        );

        Some(attach_token_transfers(
            &self.adapter,
            trade,
            &self.transfer_actions,
        ))
    }
}

impl TradeParser for PumpswapParser {
    fn process_trades(&mut self) -> Vec<TradeInfo> {
        // ОПТИМИЗАЦИЯ: кэшируем балансы ОДИН РАЗ в начале
        let (post_balance_map, pre_balance_map, transfer_map) = self.adapter.cached_balance_maps();
        
        let events = self.parse_events();
        let mut trades = Vec::with_capacity(events.len());
        
        // ОПТИМИЗАЦИЯ: используем кэшированные карты для всех событий
        for event in events {
            match &event.data {
                PumpswapEventData::Buy(buy) => {
                    if let Some(trade) = self.create_buy_trade_cached(&event, buy, &post_balance_map, &pre_balance_map, &transfer_map) {
                        trades.push(trade);
                    }
                }
                PumpswapEventData::Sell(sell) => {
                    if let Some(trade) = self.create_sell_trade_cached(&event, sell, &post_balance_map, &pre_balance_map, &transfer_map) {
                        trades.push(trade);
                    }
                }
                _ => {}
            }
        }
        
        trades
    }
}

