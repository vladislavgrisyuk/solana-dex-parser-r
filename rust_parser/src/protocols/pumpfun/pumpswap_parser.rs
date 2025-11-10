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
        let event_parser = PumpswapEventParser::new(adapter.clone());
        Self {
            adapter,
            dex_info,
            transfer_actions,
            classified_instructions,
            event_parser,
        }
    }

    fn parse_events(&self) -> Vec<PumpswapEvent> {
        let t0 = std::time::Instant::now();
        tracing::info!(
            "PumpswapParser: parsing {} classified instructions",
            self.classified_instructions.len()
        );
        
        if self.classified_instructions.is_empty() {
            tracing::warn!("⚠️ PumpswapParser: no classified instructions provided!");
            return Vec::new();
        }
        
        let t1 = std::time::Instant::now();
        let parse_result = self.event_parser.parse_instructions(&self.classified_instructions);
        let t2 = std::time::Instant::now();
        let parse_time = (t2 - t1).as_secs_f64() * 1000.0;
        
        match parse_result {
            Ok(events) => {
                let events_count = events.len();
                tracing::info!("⏱️  parse_events: parse_instructions={:.3}ms, found {} total events", parse_time, events_count);
                
                if events.is_empty() {
                    tracing::warn!("⚠️ PumpswapParser: no events found in {} instructions", self.classified_instructions.len());
                    // Log first instruction data for debugging
                    if let Some(first) = self.classified_instructions.first() {
                        tracing::debug!("First instruction data (first 32 bytes): {:?}", 
                            first.data.data.chars().take(32).collect::<String>());
                    }
                }
                
                let t3 = std::time::Instant::now();
                // Filter only BUY and SELL events, like in TypeScript
                let filtered: Vec<_> = events
                    .into_iter()
                    .filter(|event| {
                        matches!(
                            event.event_type,
                            PumpswapEventType::Buy | PumpswapEventType::Sell
                        )
                    })
                    .collect();
                let t4 = std::time::Instant::now();
                let filter_time = (t4 - t3).as_secs_f64() * 1000.0;
                let total_time = (t4 - t0).as_secs_f64() * 1000.0;
                
                tracing::info!(
                    "⏱️  parse_events: parse_instructions={:.3}ms, filtering={:.3}ms, total={:.3}ms, input_events={}, filtered_events={}",
                    parse_time, filter_time, total_time, events_count, filtered.len()
                );
                filtered
            }
            Err(err) => {
                let total_time = (std::time::Instant::now() - t0).as_secs_f64() * 1000.0;
                tracing::error!("❌ failed to parse pumpswap events: {err} (total={:.3}ms)", total_time);
                tracing::error!("   Instructions count: {}", self.classified_instructions.len());
                Vec::new()
            }
        }
    }

    #[inline]
    fn decimals_or(&self, mint: &str, default_decimals: u8) -> u8 {
        let d = self.adapter.get_token_decimals(mint);
        if d == 0 { default_decimals } else { d }
    }

    fn create_buy_trade(
        &self,
        event: &PumpswapEvent,
        buy: &super::pumpswap_event_parser::PumpswapBuyEvent,
    ) -> Option<TradeInfo> {
        let method_start = std::time::Instant::now();
        tracing::debug!(
            "create_buy_trade: looking for token accounts - quote: {}, base: {}, fee: {}",
            buy.user_quote_token_account,
            buy.user_base_token_account,
            buy.protocol_fee_recipient_token_account
        );
        
        // Get token info - try spl_token_map first, then fallback to post_token_balances, then pre_token_balances, then transfers
        let t0 = std::time::Instant::now();
        let input_info = self.adapter
            .token_account_info(&buy.user_quote_token_account)
            .cloned()
            .or_else(|| {
                tracing::debug!("Input token not in spl_token_map, checking post_token_balances...");
                self.adapter.post_token_balances()
                    .iter()
                    .find(|b| b.account == buy.user_quote_token_account)
                    .map(|b| {
                        tracing::info!("✅ Using post_token_balance for input: account={}, mint={}", b.account, b.mint);
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
                tracing::debug!("Input token not in post_token_balances, checking pre_token_balances...");
                self.adapter.pre_token_balances()
                    .iter()
                    .find(|b| b.account == buy.user_quote_token_account)
                    .map(|b| {
                        tracing::info!("✅ Using pre_token_balance for input: account={}, mint={}", b.account, b.mint);
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
                tracing::debug!("Input token not in balances, checking transfers...");
                // Try to find from transfers - look for transfers involving this account
                self.adapter.transfers()
                    .iter()
                    .find(|t| {
                        t.info.destination == buy.user_quote_token_account || 
                        t.info.source == buy.user_quote_token_account
                    })
                    .map(|t| {
                        tracing::info!("✅ Using transfer info for input: account={}, mint={}, decimals={}", 
                            buy.user_quote_token_account, t.info.mint, t.info.token_amount.decimals);
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
            Some(info) => {
                tracing::debug!("Found input token info: mint={}, decimals={}", info.mint, info.decimals);
                info
            }
            None => {
                tracing::warn!("⚠️ Input token account not found: {}, trying to infer from other sources", buy.user_quote_token_account);
                // Try to find mint from transfers or other token accounts
                let inferred_mint = self.adapter.transfers()
                    .iter()
                    .find(|t| t.info.destination == buy.user_quote_token_account || t.info.source == buy.user_quote_token_account)
                    .map(|t| t.info.mint.clone());
                
                if let Some(mint) = inferred_mint {
                    let decimals = self.adapter.get_token_decimals(&mint);
                    tracing::info!("✅ Inferred input mint from transfers: {} (decimals={})", mint, decimals);
                    crate::types::TokenInfo {
                        mint,
                        amount: 0.0,
                        amount_raw: "0".to_string(),
                        decimals: if decimals > 0 { decimals } else { 6 }, // Default to 6 for quote tokens
                        ..Default::default()
                    }
                } else {
                    // Try to infer from other quote token accounts (SOL, USDC, USDT are common quote tokens)
                    let inferred_mint = self.adapter.post_token_balances()
                        .iter()
                        .find(|b| {
                            // Common quote tokens
                            b.mint == "So11111111111111111111111111111111111111112" || // SOL
                            b.mint == "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v" || // USDC
                            b.mint == "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB" // USDT
                        })
                        .map(|b| b.mint.clone());
                    
                    if let Some(mint) = inferred_mint {
                        let decimals = self.adapter.get_token_decimals(&mint);
                        tracing::warn!("⚠️ Inferred input mint from other quote accounts: {} (decimals={})", mint, decimals);
                        crate::types::TokenInfo {
                            mint,
                            amount: 0.0,
                            amount_raw: "0".to_string(),
                            decimals: if decimals > 0 { decimals } else { 9 }, // Default to 9 for SOL
                            ..Default::default()
                        }
                    } else {
                        tracing::error!("❌ Input token account not found and cannot infer mint: {}", buy.user_quote_token_account);
                        tracing::error!("Available token accounts in spl_token_map (first 20): {:?}", 
                            self.adapter.spl_token_map().keys().take(20).collect::<Vec<_>>());
                        tracing::error!("Available post_token_balances accounts (first 20): {:?}", 
                            self.adapter.post_token_balances().iter().take(20).map(|b| &b.account).collect::<Vec<_>>());
                        tracing::error!("Available pre_token_balances accounts (first 20): {:?}", 
                            self.adapter.pre_token_balances().iter().take(20).map(|b| &b.account).collect::<Vec<_>>());
                        tracing::error!("Available transfers (first 10): {:?}", 
                            self.adapter.transfers().iter().take(10).map(|t| format!("{}->{}:{}", t.info.source, t.info.destination, t.info.mint)).collect::<Vec<_>>());
                return None;
                    }
                }
            }
        };
        
        let output_info = self.adapter
            .token_account_info(&buy.user_base_token_account)
            .cloned()
            .or_else(|| {
                tracing::debug!("Output token not in spl_token_map, checking post_token_balances...");
                let balances = self.adapter.post_token_balances();
                tracing::debug!("Available post_token_balances accounts: {:?}", 
                    balances.iter().map(|b| &b.account).collect::<Vec<_>>());
                
                balances
                    .iter()
                    .find(|b| b.account == buy.user_base_token_account)
                    .map(|b| {
                        tracing::info!("✅ Using post_token_balance for output: account={}, mint={}, decimals={}", 
                            b.account, b.mint, b.ui_token_amount.decimals);
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
                // Account might have been closed, check pre_token_balances
                tracing::debug!("Output token not in post_token_balances, checking pre_token_balances...");
                let balances = self.adapter.pre_token_balances();
                tracing::debug!("Available pre_token_balances accounts: {:?}", 
                    balances.iter().map(|b| &b.account).collect::<Vec<_>>());
                
                balances
                    .iter()
                    .find(|b| b.account == buy.user_base_token_account)
                    .map(|b| {
                        tracing::info!("✅ Using pre_token_balance for output (closed account): account={}, mint={}, decimals={}", 
                            b.account, b.mint, b.ui_token_amount.decimals);
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
                // Account might be new, check transfers for this account
                tracing::debug!("Output token not in balances, checking transfers...");
                let transfers = self.adapter.transfers();
                let transfer = transfers.iter().find(|t| {
                    t.info.destination == buy.user_base_token_account || 
                    t.info.source == buy.user_base_token_account
                });
                
                transfer.map(|t| {
                    tracing::info!("✅ Using transfer info for output: account={}, mint={}, decimals={}", 
                        buy.user_base_token_account, t.info.mint, t.info.token_amount.decimals);
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
                tracing::debug!("Output token not found anywhere, trying to infer from pool or other accounts...");
                
                // Check if we can find base mint from other token accounts that might be related
                // Look for token accounts that might have the same mint (base token)
                let balances = self.adapter.post_token_balances();
                let pool_base_account = balances.iter().find(|b| {
                    // Try to find pool base token account or other base token accounts
                    b.account != buy.user_quote_token_account && 
                    b.account != buy.protocol_fee_recipient_token_account &&
                    b.mint != input_info.mint // Not the quote mint
                });
                
                pool_base_account.map(|b| {
                    tracing::info!("✅ Using inferred base mint from other account: account={}, mint={}, decimals={}", 
                        b.account, b.mint, b.ui_token_amount.decimals);
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
            Some(info) => {
                tracing::debug!("Found output token info: mint={}, decimals={}", info.mint, info.decimals);
                info
            }
            None => {
                tracing::warn!("⚠️ Output token account not found: {}, trying to infer from other sources", buy.user_base_token_account);
                // Try to find mint from transfers or other token accounts
                let inferred_mint = self.adapter.transfers()
                    .iter()
                    .find(|t| t.info.destination == buy.user_base_token_account || t.info.source == buy.user_base_token_account)
                    .map(|t| t.info.mint.clone());
                
                if let Some(mint) = inferred_mint {
                    let decimals = self.adapter.get_token_decimals(&mint);
                    tracing::info!("✅ Inferred output mint from transfers: {} (decimals={})", mint, decimals);
                    crate::types::TokenInfo {
                        mint,
                        amount: 0.0,
                        amount_raw: "0".to_string(),
                        decimals: if decimals > 0 { decimals } else { 6 }, // Default to 6 for base tokens
                        ..Default::default()
                    }
                } else {
                    tracing::error!("❌ Output token account not found and cannot infer mint: {}", buy.user_base_token_account);
                tracing::error!("Available token accounts in spl_token_map: {:?}", 
                    self.adapter.spl_token_map().keys().take(10).collect::<Vec<_>>());
                tracing::error!("Available post_token_balances accounts: {:?}", 
                    self.adapter.post_token_balances().iter().map(|b| &b.account).collect::<Vec<_>>());
                return None;
                }
            }
        };
        
        let fee_info = self.adapter
            .token_account_info(&buy.protocol_fee_recipient_token_account)
            .cloned()
            .or_else(|| {
                tracing::debug!("Fee token not in spl_token_map, checking post_token_balances...");
                self.adapter.post_token_balances()
                    .iter()
                    .find(|b| b.account == buy.protocol_fee_recipient_token_account)
                    .map(|b| {
                        tracing::info!("✅ Using post_token_balance for fee: account={}, mint={}, decimals={}", 
                            b.account, b.mint, b.ui_token_amount.decimals);
                        crate::types::TokenInfo {
                            mint: b.mint.clone(),
                            amount: b.ui_token_amount.ui_amount.unwrap_or(0.0),
                            amount_raw: b.ui_token_amount.amount.clone(),
                            decimals: b.ui_token_amount.decimals,
                            ..Default::default()
                        }
                    })
            });
        
        let fee_info = match fee_info {
            Some(info) => {
                tracing::debug!("Found fee token info: mint={}, decimals={}", info.mint, info.decimals);
                info
            }
            None => {
                tracing::warn!("⚠️ Fee token account not found: {}, continuing anyway", buy.protocol_fee_recipient_token_account);
                // Fee token might not be in balances, use input token decimals as fallback
                crate::types::TokenInfo {
                    mint: input_info.mint.clone(),
                    amount: 0.0,
                    amount_raw: "0".to_string(),
                    decimals: input_info.decimals,
                    ..Default::default()
                }
            }
        };

        let t1 = std::time::Instant::now();
        let input_lookup_time = (t1 - t0).as_secs_f64() * 1000.0;
        tracing::info!("⏱️  create_buy_trade: [1/5] input_lookup={:.3}ms", input_lookup_time);

        let t2 = std::time::Instant::now();
        let output_lookup_time = (t2 - t1).as_secs_f64() * 1000.0;
        tracing::info!("⏱️  create_buy_trade: [2/5] output_lookup={:.3}ms", output_lookup_time);
        
        let t3 = std::time::Instant::now();
        let fee_lookup_time = (t3 - t2).as_secs_f64() * 1000.0;
        tracing::info!("⏱️  create_buy_trade: [3/5] fee_lookup={:.3}ms", fee_lookup_time);
        
        let t4 = std::time::Instant::now();
        let input_decimals = self.decimals_or(&input_info.mint, input_info.decimals);
        let output_decimals = self.decimals_or(&output_info.mint, output_info.decimals);
        let fee_decimals = self.decimals_or(&fee_info.mint, fee_info.decimals);
        let t5 = std::time::Instant::now();
        let decimals_time = (t5 - t4).as_secs_f64() * 1000.0;
        tracing::info!("⏱️  create_buy_trade: [4/5] get_decimals={:.3}ms", decimals_time);

        let t6 = std::time::Instant::now();
        let trade = build_pumpswap_buy_trade(
            event,
            buy,
            (&input_info.mint, input_decimals),
            (&output_info.mint, output_decimals),
            (&fee_info.mint, fee_decimals),
            &self.dex_info,
        );
        let t7 = std::time::Instant::now();
        let build_trade_time = (t7 - t6).as_secs_f64() * 1000.0;
        tracing::info!("⏱️  create_buy_trade: [5/5] build_trade={:.3}ms", build_trade_time);

        let t8 = std::time::Instant::now();
        let result = attach_token_transfers(
            &self.adapter,
            trade,
            &self.transfer_actions,
        );
        let t9 = std::time::Instant::now();
        let attach_transfers_time = (t9 - t8).as_secs_f64() * 1000.0;
        tracing::info!("⏱️  create_buy_trade: attach_transfers={:.3}ms", attach_transfers_time);
        
        let method_duration = method_start.elapsed().as_secs_f64() * 1000.0;
        tracing::info!(
            "✅ create_buy_trade END: total={:.3}ms (input={:.3}ms, output={:.3}ms, fee={:.3}ms, decimals={:.3}ms, build={:.3}ms, attach={:.3}ms)",
            method_duration, input_lookup_time, output_lookup_time, fee_lookup_time, decimals_time, build_trade_time, attach_transfers_time
        );
        
        Some(result)
    }

    fn create_sell_trade(
        &self,
        event: &PumpswapEvent,
        sell: &super::pumpswap_event_parser::PumpswapSellEvent,
    ) -> Option<TradeInfo> {
        let method_start = std::time::Instant::now();
        tracing::debug!(
            "create_sell_trade: looking for token accounts - base: {}, quote: {}, fee: {}",
            sell.user_base_token_account,
            sell.user_quote_token_account,
            sell.protocol_fee_recipient_token_account
        );
        
        // Get token info - try spl_token_map first, then fallback to post_token_balances, then pre_token_balances, then transfers
        let t0 = std::time::Instant::now();
        let input_info = self.adapter
            .token_account_info(&sell.user_base_token_account)
            .cloned()
            .or_else(|| {
                tracing::debug!("Input token not in spl_token_map, checking post_token_balances...");
                self.adapter.post_token_balances()
                    .iter()
                    .find(|b| b.account == sell.user_base_token_account)
                    .map(|b| {
                        tracing::info!("✅ Using post_token_balance for input: account={}, mint={}", b.account, b.mint);
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
                tracing::debug!("Input token not in post_token_balances, checking pre_token_balances...");
                self.adapter.pre_token_balances()
                    .iter()
                    .find(|b| b.account == sell.user_base_token_account)
                    .map(|b| {
                        tracing::info!("✅ Using pre_token_balance for input: account={}, mint={}", b.account, b.mint);
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
                tracing::debug!("Input token not in balances, checking transfers...");
                // Try to find from transfers
                self.adapter.transfers()
                    .iter()
                    .find(|t| t.info.destination == sell.user_base_token_account || t.info.source == sell.user_base_token_account)
                    .map(|t| {
                        tracing::info!("✅ Using transfer info for input: account={}, mint={}, decimals={}", 
                            sell.user_base_token_account, t.info.mint, t.info.token_amount.decimals);
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
            Some(info) => {
                tracing::debug!("✅ Found input token info: mint={}, decimals={}", info.mint, info.decimals);
                info
            }
            None => {
                // Try to infer from other base token accounts or from BUY event in the same transaction
                // For SELL, input is base token. Look for base mint from other accounts or from BUY event
                let inferred_mint = self.adapter.post_token_balances()
                    .iter()
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
                        // Try to find from pre_token_balances
                        self.adapter.pre_token_balances()
                            .iter()
                            .find(|b| {
                                !b.account.is_empty() && 
                                !b.mint.is_empty() &&
                                b.mint != "So11111111111111111111111111111111111111112" &&
                                b.mint != "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v" &&
                                b.mint != "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB"
                            })
                            .map(|b| b.mint.clone())
                    });
                
                if let Some(mint) = inferred_mint {
                    let decimals = self.adapter.get_token_decimals(&mint);
                    tracing::warn!("⚠️ Inferred input mint from other base accounts: {} (decimals={})", mint, decimals);
                    crate::types::TokenInfo {
                        mint,
                        amount: 0.0,
                        amount_raw: "0".to_string(),
                        decimals: if decimals > 0 { decimals } else { 6 }, // Default to 6 for base tokens
                        ..Default::default()
                    }
                } else {
                    tracing::error!("❌ Input token account not found: {}", sell.user_base_token_account);
                    tracing::error!("Available token accounts in spl_token_map (first 20): {:?}", 
                        self.adapter.spl_token_map().keys().take(20).collect::<Vec<_>>());
                    tracing::error!("Available post_token_balances accounts (first 20): {:?}", 
                        self.adapter.post_token_balances().iter().take(20).map(|b| format!("account={}, mint={}", b.account, b.mint)).collect::<Vec<_>>());
                    tracing::error!("Available pre_token_balances accounts (first 20): {:?}", 
                        self.adapter.pre_token_balances().iter().take(20).map(|b| format!("account={}, mint={}", b.account, b.mint)).collect::<Vec<_>>());
                    tracing::error!("Available transfers (first 10): {:?}", 
                        self.adapter.transfers().iter().take(10).map(|t| format!("{}->{}:{}", t.info.source, t.info.destination, t.info.mint)).collect::<Vec<_>>());
                    return None;
                }
            }
        };
        
        let output_info = self.adapter
            .token_account_info(&sell.user_quote_token_account)
            .cloned()
            .or_else(|| {
                self.adapter.post_token_balances()
                    .iter()
                    .find(|b| b.account == sell.user_quote_token_account)
                    .map(|b| {
                        tracing::info!("Using post_token_balance for output: account={}, mint={}", b.account, b.mint);
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
                tracing::debug!("Output token not in post_token_balances, checking pre_token_balances...");
                self.adapter.pre_token_balances()
                    .iter()
                    .find(|b| b.account == sell.user_quote_token_account)
                    .map(|b| {
                        tracing::info!("✅ Using pre_token_balance for output: account={}, mint={}", b.account, b.mint);
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
                tracing::debug!("Output token not in balances, checking transfers...");
                self.adapter.transfers()
                    .iter()
                    .find(|t| t.info.destination == sell.user_quote_token_account || t.info.source == sell.user_quote_token_account)
                    .map(|t| {
                        tracing::info!("✅ Using transfer info for output: account={}, mint={}, decimals={}", 
                            sell.user_quote_token_account, t.info.mint, t.info.token_amount.decimals);
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
            Some(info) => {
                tracing::debug!("✅ Found output token info: mint={}, decimals={}", info.mint, info.decimals);
                info
            }
            None => {
                tracing::error!("❌ Output token account not found: {}", sell.user_quote_token_account);
                return None;
            }
        };
        
        let fee_info = self.adapter
            .token_account_info(&sell.protocol_fee_recipient_token_account)
            .cloned()
            .or_else(|| {
                self.adapter.post_token_balances()
                    .iter()
                    .find(|b| b.account == sell.protocol_fee_recipient_token_account)
                    .map(|b| {
                        tracing::info!("Using post_token_balance for fee: account={}, mint={}", b.account, b.mint);
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
                // Try to find from transfers, or use input token as fallback
                self.adapter.transfers()
                    .iter()
                    .find(|t| t.info.destination == sell.protocol_fee_recipient_token_account || t.info.source == sell.protocol_fee_recipient_token_account)
                    .map(|t| {
                        tracing::info!("Using transfer info for fee: account={}, mint={}", sell.protocol_fee_recipient_token_account, t.info.mint);
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
                tracing::warn!("⚠️ Fee token account not found: {}, using input token mint as fallback", sell.protocol_fee_recipient_token_account);
                crate::types::TokenInfo {
                    mint: input_info.mint.clone(),
                    amount: 0.0,
                    amount_raw: "0".to_string(),
                    decimals: input_info.decimals,
                    ..Default::default()
                }
            });

        let t1 = std::time::Instant::now();
        let input_lookup_time = (t1 - t0).as_secs_f64() * 1000.0;
        tracing::info!("⏱️  create_sell_trade: [1/5] input_lookup={:.3}ms", input_lookup_time);
        
        let t2 = std::time::Instant::now();
        let output_lookup_time = (t2 - t1).as_secs_f64() * 1000.0;
        tracing::info!("⏱️  create_sell_trade: [2/5] output_lookup={:.3}ms", output_lookup_time);
        
        let t3 = std::time::Instant::now();
        let fee_lookup_time = (t3 - t2).as_secs_f64() * 1000.0;
        tracing::info!("⏱️  create_sell_trade: [3/5] fee_lookup={:.3}ms", fee_lookup_time);
        
        let t4 = std::time::Instant::now();
        let input_decimals = self.decimals_or(&input_info.mint, input_info.decimals);
        let output_decimals = self.decimals_or(&output_info.mint, output_info.decimals);
        let fee_decimals = self.decimals_or(&fee_info.mint, fee_info.decimals);
        let t5 = std::time::Instant::now();
        let decimals_time = (t5 - t4).as_secs_f64() * 1000.0;
        tracing::info!("⏱️  create_sell_trade: [4/5] get_decimals={:.3}ms", decimals_time);

        let t6 = std::time::Instant::now();
        let trade = build_pumpswap_sell_trade(
            event,
            sell,
            (&input_info.mint, input_decimals),
            (&output_info.mint, output_decimals),
            (&fee_info.mint, fee_decimals),
            &self.dex_info,
        );
        let t7 = std::time::Instant::now();
        let build_trade_time = (t7 - t6).as_secs_f64() * 1000.0;
        tracing::info!("⏱️  create_sell_trade: [5/5] build_trade={:.3}ms", build_trade_time);

        let t8 = std::time::Instant::now();
        let result = attach_token_transfers(
            &self.adapter,
            trade,
            &self.transfer_actions,
        );
        let t9 = std::time::Instant::now();
        let attach_transfers_time = (t9 - t8).as_secs_f64() * 1000.0;
        tracing::info!("⏱️  create_sell_trade: attach_transfers={:.3}ms", attach_transfers_time);
        
        let method_duration = method_start.elapsed().as_secs_f64() * 1000.0;
        tracing::info!(
            "✅ create_sell_trade END: total={:.3}ms (input={:.3}ms, output={:.3}ms, fee={:.3}ms, decimals={:.3}ms, build={:.3}ms, attach={:.3}ms)",
            method_duration, input_lookup_time, output_lookup_time, fee_lookup_time, decimals_time, build_trade_time, attach_transfers_time
        );
        
        Some(result)
    }
}

impl TradeParser for PumpswapParser {
    fn process_trades(&mut self) -> Vec<TradeInfo> {
        let method_start = std::time::Instant::now();
        tracing::info!("🔹 PumpswapParser::process_trades START");
        
        let t0 = std::time::Instant::now();
        let events = self.parse_events();
        let t1 = std::time::Instant::now();
        let parse_events_time = (t1 - t0).as_secs_f64() * 1000.0;
        tracing::info!("⏱️  [1/3] parse_events={:.3}ms, found {} events", parse_events_time, events.len());
        
        let t2 = std::time::Instant::now();
        let mut trades = Vec::with_capacity(events.len());
        let mut buy_count = 0;
        let mut sell_count = 0;
        let mut buy_success = 0;
        let mut sell_success = 0;
        let mut buy_time = 0.0;
        let mut sell_time = 0.0;
        
        for (idx, event) in events.into_iter().enumerate() {
            let event_start = std::time::Instant::now();
            match &event.data {
                PumpswapEventData::Buy(buy) => {
                    buy_count += 1;
                    let t_buy = std::time::Instant::now();
                    if let Some(trade) = self.create_buy_trade(&event, buy) {
                        buy_success += 1;
                        trades.push(trade);
                    }
                    let buy_duration = (std::time::Instant::now() - t_buy).as_secs_f64() * 1000.0;
                    buy_time += buy_duration;
                    tracing::info!("⏱️  [{}/{}] create_buy_trade={:.3}ms", idx + 1, buy_count + sell_count, buy_duration);
                }
                PumpswapEventData::Sell(sell) => {
                    sell_count += 1;
                    let t_sell = std::time::Instant::now();
                    if let Some(trade) = self.create_sell_trade(&event, sell) {
                        sell_success += 1;
                        trades.push(trade);
                    }
                    let sell_duration = (std::time::Instant::now() - t_sell).as_secs_f64() * 1000.0;
                    sell_time += sell_duration;
                    tracing::info!("⏱️  [{}/{}] create_sell_trade={:.3}ms", idx + 1, buy_count + sell_count, sell_duration);
                }
                _ => {
                    tracing::debug!("Skipping non-trade event: {:?}", event.event_type);
                }
            }
            let event_duration = event_start.elapsed().as_secs_f64() * 1000.0;
            tracing::debug!("⏱️  [{}/{}] process_event_total={:.3}ms", idx + 1, buy_count + sell_count, event_duration);
        }
        let t3 = std::time::Instant::now();
        let process_events_time = (t3 - t2).as_secs_f64() * 1000.0;
        tracing::info!("⏱️  [2/3] process_events={:.3}ms (buy: {} processed, {} success, {:.3}ms total; sell: {} processed, {} success, {:.3}ms total)", 
            process_events_time, buy_count, buy_success, buy_time, sell_count, sell_success, sell_time);
        
        let method_duration = method_start.elapsed().as_secs_f64() * 1000.0;
        tracing::info!("⏱️  [3/3] process_trades_total={:.3}ms", method_duration);
        tracing::info!("✅ PumpswapParser::process_trades END: total={:.3}ms (parse_events={:.3}ms, process_events={:.3}ms), returning {} trades", 
            method_duration, parse_events_time, process_events_time, trades.len());
        trades
    }
}

