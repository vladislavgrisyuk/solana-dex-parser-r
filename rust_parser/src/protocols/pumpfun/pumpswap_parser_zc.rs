//! Zero-copy Pumpswap parser for ZcAdapter
//! 
//! This module provides zero-copy parsing of Pumpswap trades using ZcAdapter
//! and ZcCachedBalanceMaps, avoiding conversion to SolanaTransaction.

use std::collections::HashMap;

use crate::core::zc_adapter::ZcAdapter;
use crate::core::zc_adapter_helpers::ZcCachedBalanceMaps;
use crate::core::zc_instruction_classifier::ZcClassifiedInstruction;
use crate::types::{DexInfo, TradeInfo, TransferMap};

use super::pumpswap_event_parser::{
    PumpswapEvent, PumpswapEventData, PumpswapEventParser, PumpswapEventType,
};
use super::util::{build_pumpswap_buy_trade, build_pumpswap_sell_trade};

/// Process Pumpswap trades using zero-copy structures
/// 
/// This function works directly with ZcAdapter and ZcCachedBalanceMaps,
/// avoiding conversion to SolanaTransaction.
/// 
/// # Arguments
/// * `zc_adapter` - Zero-copy adapter
/// * `classified_instructions` - Zero-copy classified instructions
/// * `cached_maps` - Cached balance maps (parsed from meta JSON)
/// * `transfer_actions` - Transfer map (parsed from instructions)
/// * `dex_info` - DEX info
/// 
/// # Returns
/// Vector of trade info
pub fn process_pumpswap_trades_zc<'a>(
    zc_adapter: &'a ZcAdapter<'a>,
    classified_instructions: &[ZcClassifiedInstruction<'a>],
    cached_maps: &ZcCachedBalanceMaps,
    transfer_actions: &TransferMap,
    dex_info: &DexInfo,
) -> Vec<TradeInfo> {
    // Parse events using zero-copy event parser
    let event_parser = PumpswapEventParser::new();
    let events_result = event_parser.parse_instructions_zc(zc_adapter, classified_instructions);
    
    let events = match events_result {
        Ok(events) => {
            // Filter only Buy/Sell events
            events
                .into_iter()
                .filter(|e| matches!(e.event_type, PumpswapEventType::Buy | PumpswapEventType::Sell))
                .collect()
        }
        Err(_) => Vec::new(),
    };
    
    if events.is_empty() {
        return Vec::new();
    }
    
    // Get cached balance maps (with references for zero-copy)
    let post_balance_map = cached_maps.post_balance_map_ref();
    let pre_balance_map = cached_maps.pre_balance_map_ref();
    let transfer_map = cached_maps.transfer_map_ref();
    
    // Process events and create trades
    let mut trades = Vec::with_capacity(events.len());
    
    for event in events {
            match &event.data {
            PumpswapEventData::Buy(buy) => {
                if let Some(trade) = create_buy_trade_zc(
                    &event,
                    buy,
                    zc_adapter,
                    cached_maps,
                    &post_balance_map,
                    &pre_balance_map,
                    &transfer_map,
                    transfer_actions,
                    dex_info,
                ) {
                    trades.push(trade);
                }
            }
            PumpswapEventData::Sell(sell) => {
                if let Some(trade) = create_sell_trade_zc(
                    &event,
                    sell,
                    zc_adapter,
                    cached_maps,
                    &post_balance_map,
                    &pre_balance_map,
                    &transfer_map,
                    transfer_actions,
                    dex_info,
                ) {
                    trades.push(trade);
                }
            }
            _ => {}
        }
    }
    
    trades
}

/// Create buy trade using zero-copy structures
fn create_buy_trade_zc(
    event: &PumpswapEvent,
    buy: &super::pumpswap_event_parser::PumpswapBuyEvent,
    zc_adapter: &ZcAdapter,
    cached_maps: &ZcCachedBalanceMaps,
    post_balance_map: &HashMap<&str, &crate::types::TokenBalance>,
    pre_balance_map: &HashMap<&str, &crate::types::TokenBalance>,
    transfer_map: &HashMap<&str, &crate::types::TransferData>,
    transfer_actions: &TransferMap,
    dex_info: &DexInfo,
) -> Option<TradeInfo> {
    // Get token info - try cached maps first, then fallback
    let input_info = cached_maps
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
            // Try to infer from transfers or common quote tokens
            let inferred_mint = transfer_map.get(buy.user_quote_token_account.as_str())
                .map(|t| t.info.mint.clone())
                .or_else(|| {
                    post_balance_map.values()
                        .find(|b| {
                            b.mint == "So11111111111111111111111111111111111111112" || // SOL
                            b.mint == "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v" || // USDC
                            b.mint == "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB" // USDT
                        })
                        .map(|b| b.mint.clone())
                });
            
            if let Some(mint) = inferred_mint {
                let decimals = cached_maps.get_token_decimals(&mint);
                crate::types::TokenInfo {
                    mint,
                    amount: 0.0,
                    amount_raw: "0".to_string(),
                    decimals: if decimals > 0 { decimals } else { 6 },
                    ..Default::default()
                }
            } else {
                return None;
            }
        }
    };
    
    // Get output info (base token)
    let output_info = cached_maps
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
            // Last resort: try to find mint from other token accounts
            post_balance_map.values()
                .find(|b| {
                    b.account != buy.user_quote_token_account && 
                    b.account != buy.protocol_fee_recipient_token_account &&
                    b.mint != input_info.mint
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
            let inferred_mint = transfer_map.get(buy.user_base_token_account.as_str())
                .map(|t| t.info.mint.clone());
            
            if let Some(mint) = inferred_mint {
                let decimals = cached_maps.get_token_decimals(&mint);
                crate::types::TokenInfo {
                    mint,
                    amount: 0.0,
                    amount_raw: "0".to_string(),
                    decimals: if decimals > 0 { decimals } else { 6 },
                    ..Default::default()
                }
            } else {
                return None;
            }
        }
    };
    
    // Get fee info
    let fee_info = cached_maps
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
    
    let input_decimals = if cached_maps.get_token_decimals(&input_info.mint) > 0 {
        cached_maps.get_token_decimals(&input_info.mint)
    } else {
        input_info.decimals
    };
    let output_decimals = if cached_maps.get_token_decimals(&output_info.mint) > 0 {
        cached_maps.get_token_decimals(&output_info.mint)
    } else {
        output_info.decimals
    };
    let fee_decimals = if cached_maps.get_token_decimals(&fee_info.mint) > 0 {
        cached_maps.get_token_decimals(&fee_info.mint)
    } else {
        fee_info.decimals
    };
    
    let mut trade = build_pumpswap_buy_trade(
        event,
        buy,
        (&input_info.mint, input_decimals),
        (&output_info.mint, output_decimals),
        (&fee_info.mint, fee_decimals),
        dex_info,
    );
    
    // Attach token transfers (zero-copy: work with transfer_actions directly)
    attach_token_transfers_zc(&mut trade, transfer_actions);
    
    Some(trade)
}

/// Create sell trade using zero-copy structures
fn create_sell_trade_zc(
    event: &PumpswapEvent,
    sell: &super::pumpswap_event_parser::PumpswapSellEvent,
    zc_adapter: &ZcAdapter,
    cached_maps: &ZcCachedBalanceMaps,
    post_balance_map: &HashMap<&str, &crate::types::TokenBalance>,
    pre_balance_map: &HashMap<&str, &crate::types::TokenBalance>,
    transfer_map: &HashMap<&str, &crate::types::TransferData>,
    transfer_actions: &TransferMap,
    dex_info: &DexInfo,
) -> Option<TradeInfo> {
    // Get token info - try cached maps first, then fallback
    let input_info = cached_maps
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
            // Try to infer from transfers or other base token accounts
            let inferred_mint = transfer_map.get(sell.user_base_token_account.as_str())
                .map(|t| t.info.mint.clone())
                .or_else(|| {
                    post_balance_map.values()
                        .find(|b| {
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
                let decimals = cached_maps.get_token_decimals(&mint);
                crate::types::TokenInfo {
                    mint,
                    amount: 0.0,
                    amount_raw: "0".to_string(),
                    decimals: if decimals > 0 { decimals } else { 6 },
                    ..Default::default()
                }
            } else {
                return None;
            }
        }
    };
    
    // Get output info (quote token)
    let output_info = cached_maps
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
    
    // Get fee info
    let fee_info = cached_maps
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
    
    let input_decimals = if cached_maps.get_token_decimals(&input_info.mint) > 0 {
        cached_maps.get_token_decimals(&input_info.mint)
    } else {
        input_info.decimals
    };
    let output_decimals = if cached_maps.get_token_decimals(&output_info.mint) > 0 {
        cached_maps.get_token_decimals(&output_info.mint)
    } else {
        output_info.decimals
    };
    let fee_decimals = if cached_maps.get_token_decimals(&fee_info.mint) > 0 {
        cached_maps.get_token_decimals(&fee_info.mint)
    } else {
        fee_info.decimals
    };
    
    let mut trade = build_pumpswap_sell_trade(
        event,
        sell,
        (&input_info.mint, input_decimals),
        (&output_info.mint, output_decimals),
        (&fee_info.mint, fee_decimals),
        dex_info,
    );
    
    // Attach token transfers (zero-copy: work with transfer_actions directly)
    attach_token_transfers_zc(&mut trade, transfer_actions);
    
    Some(trade)
}

/// Attach token transfers to trade (zero-copy version)
/// 
/// This function works directly with TransferMap, avoiding TransactionAdapter.
fn attach_token_transfers_zc(
    trade: &mut TradeInfo,
    transfer_actions: &TransferMap,
) {
    if let Some(ref program_id) = trade.program_id {
        if let Some(entries) = transfer_actions.get(program_id) {
            if let Some(transfer) = entries.iter().find(|entry| {
                entry.info.mint == trade.input_token.mint
                    && entry.info.token_amount.amount == trade.input_token.amount_raw
            }) {
                trade.user.get_or_insert_with(|| transfer.info.source.clone());
            }
        }
    }
    
    // Signer is already set from event, no need to update
}

