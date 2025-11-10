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
        
        match self
            .event_parser
            .parse_instructions(&self.classified_instructions)
        {
            Ok(events) => {
                let t1 = std::time::Instant::now();
                tracing::info!("PumpswapParser: found {} total events", events.len());
                
                if events.is_empty() {
                    tracing::warn!("⚠️ PumpswapParser: no events found in {} instructions", self.classified_instructions.len());
                    // Log first instruction data for debugging
                    if let Some(first) = self.classified_instructions.first() {
                        tracing::debug!("First instruction data (first 32 bytes): {:?}", 
                            first.data.data.chars().take(32).collect::<String>());
                    }
                }
                
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
                let t2 = std::time::Instant::now();
                tracing::debug!(
                    "⏱️  parse_events: parsing={:.3}μs, filtering={:.3}μs, total={:.3}μs, events={}",
                    (t1 - t0).as_secs_f64() * 1_000_000.0,
                    (t2 - t1).as_secs_f64() * 1_000_000.0,
                    (t2 - t0).as_secs_f64() * 1_000_000.0,
                    filtered.len()
                );
                tracing::info!("PumpswapParser: filtered to {} BUY/SELL events", filtered.len());
                filtered
            }
            Err(err) => {
                tracing::error!("❌ failed to parse pumpswap events: {err}");
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
        let t0 = std::time::Instant::now();
        tracing::debug!(
            "create_buy_trade: looking for token accounts - quote: {}, base: {}, fee: {}",
            buy.user_quote_token_account,
            buy.user_base_token_account,
            buy.protocol_fee_recipient_token_account
        );
        
        // Get token info - try spl_token_map first, then fallback to post_token_balances, then pre_token_balances, then transfers
        let t1 = std::time::Instant::now();
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

        let t2 = std::time::Instant::now();
        let input_decimals = self.decimals_or(&input_info.mint, input_info.decimals);
        let output_decimals = self.decimals_or(&output_info.mint, output_info.decimals);
        let fee_decimals = self.decimals_or(&fee_info.mint, fee_info.decimals);
        let t3 = std::time::Instant::now();

        let trade = build_pumpswap_buy_trade(
            event,
            buy,
            (&input_info.mint, input_decimals),
            (&output_info.mint, output_decimals),
            (&fee_info.mint, fee_decimals),
            &self.dex_info,
        );
        let t4 = std::time::Instant::now();

        let result = attach_token_transfers(
            &self.adapter,
            trade,
            &self.transfer_actions,
        );
        let t5 = std::time::Instant::now();
        
        tracing::debug!(
            "⏱️  create_buy_trade: token_lookup={:.3}μs, decimals={:.3}μs, build_trade={:.3}μs, attach_transfers={:.3}μs, total={:.3}μs",
            (t2 - t1).as_secs_f64() * 1_000_000.0,
            (t3 - t2).as_secs_f64() * 1_000_000.0,
            (t4 - t3).as_secs_f64() * 1_000_000.0,
            (t5 - t4).as_secs_f64() * 1_000_000.0,
            (t5 - t0).as_secs_f64() * 1_000_000.0,
        );
        
        Some(result)
    }

    fn create_sell_trade(
        &self,
        event: &PumpswapEvent,
        sell: &super::pumpswap_event_parser::PumpswapSellEvent,
    ) -> Option<TradeInfo> {
        tracing::debug!(
            "create_sell_trade: looking for token accounts - base: {}, quote: {}, fee: {}",
            sell.user_base_token_account,
            sell.user_quote_token_account,
            sell.protocol_fee_recipient_token_account
        );
        
        // Get token info - try spl_token_map first, then fallback to post_token_balances, then pre_token_balances, then transfers
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
        let mut trades = Vec::new();
        let events = self.parse_events();
        tracing::info!("PumpswapParser::process_trades: found {} events", events.len());
        
        for event in events {
            match &event.data {
                PumpswapEventData::Buy(buy) => {
                    tracing::info!("Processing BUY event: user={}, pool={}, quote_account={}, base_account={}", 
                        buy.user, buy.pool, buy.user_quote_token_account, buy.user_base_token_account);
                    if let Some(trade) = self.create_buy_trade(&event, buy) {
                        tracing::info!("✅ Created BUY trade: {} -> {}", trade.input_token.mint, trade.output_token.mint);
                        trades.push(trade);
                    } else {
                        tracing::warn!("❌ Failed to create BUY trade for event at {}", event.idx);
                    }
                }
                PumpswapEventData::Sell(sell) => {
                    tracing::info!("Processing SELL event: user={}, pool={}, base_account={}, quote_account={}", 
                        sell.user, sell.pool, sell.user_base_token_account, sell.user_quote_token_account);
                    if let Some(trade) = self.create_sell_trade(&event, sell) {
                        tracing::info!("✅ Created SELL trade: {} -> {}", trade.input_token.mint, trade.output_token.mint);
                        trades.push(trade);
                    } else {
                        tracing::warn!("❌ Failed to create SELL trade for event at {}", event.idx);
                    }
                }
                _ => {
                    tracing::debug!("Skipping non-trade event: {:?}", event.event_type);
                }
            }
        }
        tracing::info!("PumpswapParser::process_trades: returning {} trades", trades.len());
        trades
    }
}

