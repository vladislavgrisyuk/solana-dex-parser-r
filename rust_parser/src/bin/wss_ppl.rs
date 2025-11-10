// cargo run --release --bin wss_ppl -- [MINT1,MINT2,...]
//
// WebSocket parser using DexParser for transaction parsing.
// Measures parsing performance at each stage.
// Optimized for base64 encoding from Helius WebSocket.

use anyhow::{anyhow, Context, Result};
use base64_simd::STANDARD as B64;
use bincode::deserialize;
use bs58;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use solana_dex_parser::config::ParseConfig;
use solana_dex_parser::core::dex_parser::DexParser;
use solana_dex_parser::types::{BalanceChange, InnerInstruction, SolanaInstruction, SolanaTransaction, TokenBalance, TokenAmount, TransactionMeta, TransactionStatus};
use std::fmt::Write;
use solana_sdk::transaction::VersionedTransaction;
use std::collections::HashMap;
use std::time::Instant;
use tokio::time::{interval, Duration, MissedTickBehavior};
use tokio_tungstenite::tungstenite::Message;

// === Entry ===

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let api_key = "767f42d9-06c2-46f8-8031-9869035d6ce4".to_string();
    let include_mints: Vec<String> = args
        .next()
        .unwrap_or_else(|| "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA".to_string())
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let ws_url = format!("wss://atlas-mainnet.helius-rpc.com/?api-key={}", api_key);
    println!("🔌 Connecting to {}", ws_url);

    let (ws_stream, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .context("WebSocket connection failed")?;
    let (mut sink, mut stream) = ws_stream.split();

    // Subscribe: base64 + full + v0 support
    let sub = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "transactionSubscribe",
        "params": [
            { "accountInclude": include_mints, "vote": false, "failed": false },
            {
                "commitment": "processed",
                "encoding": "base64",
                "transactionDetails": "full",
                "maxSupportedTransactionVersion": 0
            }
        ]
    });

    sink.send(Message::Text(sub.to_string()))
        .await
        .context("Failed to send subscription")?;
    println!("✅ Subscribed (encoding=base64, details=full, mints={:?})", include_mints);

    // Keepalive pings
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(30));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            // tokio-tungstenite handles TCP keepalive automatically
        }
    });

    // Initialize parser
    let parser = DexParser::new();
    let config = ParseConfig::default();

    let mut shown = 0usize;
    const MAX_EVENTS: usize = 50;

    println!("\n📊 Waiting for transactions...\n");

    while let Some(msg) = stream.next().await {
        let raw = match msg {
            Ok(Message::Text(t)) => t,
            Ok(Message::Binary(b)) => String::from_utf8_lossy(&b).into_owned(),
            Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_)) => continue,
            Ok(Message::Close(_)) => break,
            Err(e) => {
                eprintln!("⚠️ WebSocket error: {e}");
                break;
            }
        };

        let t_total_start = Instant::now();

        // === Stage 1: JSON Parsing ===
        let t_json_start = Instant::now();
        let raw_bytes = raw.as_bytes();
        let v: Value = match serde_json::from_slice(raw_bytes) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("⚠️ JSON parse error: {e}");
                continue;
            }
        };
        let t_json_end = Instant::now();

        // Check if it's a transaction notification
        if v.get("method").and_then(|m| m.as_str()) != Some("transactionNotification") {
            continue;
        }

        let result = match v.pointer("/params/result") {
            Some(r) => r,
            None => continue,
        };

        let signature = result
            .get("signature")
            .and_then(|s| s.as_str())
            .unwrap_or("<unknown>");
        let slot = result.get("slot").and_then(|s| s.as_u64()).unwrap_or(0);

        // === Stage 2: Extract base64 and decode ===
        let t_convert_start = Instant::now();
        
        // Helius sends: "transaction": ["<base64_string>", "base64"]
        // Extract base64 string from array
        let tx_bytes = match extract_base64_tx(result) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => {
                eprintln!("⚠️ Transaction is not in base64 format, skipping");
                continue;
            }
            Err(e) => {
                eprintln!("⚠️ Failed to extract base64 transaction: {e}");
                continue;
            }
        };
        
        // Extract metadata from result (for balance changes, etc.)
        let meta = result.pointer("/transaction/meta").or_else(|| result.get("meta"));
        
        // Convert binary transaction bytes to SolanaTransaction
        let tx = match convert_binary_to_solana_tx(&tx_bytes, slot, signature, meta) {
            Ok(tx) => tx,
            Err(e) => {
                eprintln!("⚠️ Failed to convert binary to SolanaTransaction: {e}");
                continue;
            }
        };
        let t_convert_end = Instant::now();

        // === Stage 3: Parse with DexParser ===
        // This internally uses get_instruction_data() which is optimized:
        // - Uses base64-simd::STANDARD (20-50x faster than base64)
        // - #[inline(always)] for hot-path
        // - No caching overhead
        // - No logging in hot-path
        let t_parse_start = Instant::now();
        let parse_result = parser.parse_all(tx, Some(config.clone()));
        let t_parse_end = Instant::now();

        // === Stage 4: Display Results ===
        let t_display_start = Instant::now();
        print_results(
            signature,
            slot,
            &parse_result,
            t_total_start,
            t_json_start,
            t_json_end,
            t_convert_start,
            t_convert_end,
            t_parse_start,
            t_parse_end,
            t_display_start,
        );
        let t_display_end = Instant::now();

        // === Final Timing Summary ===
        let total_time = t_display_end.duration_since(t_total_start);
        let json_time = t_json_end.duration_since(t_json_start);
        let convert_time = t_convert_end.duration_since(t_convert_start);
        let parse_time = t_parse_end.duration_since(t_parse_start);
        let display_time = t_display_end.duration_since(t_display_start);

        println!(
            "⏱️  TIMING: JSON={:.3}ms  Convert={:.3}ms  Parse={:.3}ms  Display={:.3}ms  TOTAL={:.3}ms",
            ms(json_time),
            ms(convert_time),
            ms(parse_time),
            ms(display_time),
            ms(total_time)
        );
        println!("{}", "─".repeat(100));

        shown += 1;
        if shown >= MAX_EVENTS {
            println!("\n✅ Processed {} events — exiting", shown);
            break;
        }
    }

    Ok(())
}

// === Helpers ===

fn ms(d: std::time::Duration) -> f64 {
    d.as_secs_f64() * 1_000.0
}

fn print_results(
    signature: &str,
    slot: u64,
    result: &solana_dex_parser::types::ParseResult,
    t_total_start: Instant,
    t_json_start: Instant,
    t_json_end: Instant,
    t_convert_start: Instant,
    t_convert_end: Instant,
    t_parse_start: Instant,
    t_parse_end: Instant,
    t_display_start: Instant,
) {
    println!("{}", "═".repeat(100));
    println!("🔗 Transaction: {} @ slot {}", signature, slot);
    println!("   State: {}", if result.state { "✅ Success" } else { "❌ Failed" });
    
    if let Some(ref msg) = result.msg {
        println!("   Message: {}", msg);
    }

    println!("\n📊 Parse Results:");
    println!("   Trades:      {}", result.trades.len());
    println!("   Liquidities: {}", result.liquidities.len());
    println!("   Transfers:   {}", result.transfers.len());
    println!("   Meme Events: {}", result.meme_events.len());

    if !result.trades.is_empty() {
        println!("\n💱 Trades:");
        for (i, trade) in result.trades.iter().take(3).enumerate() {
            println!(
                "   [{i}] {} -> {} (amount: {})",
                trade.input_token.mint,
                trade.output_token.mint,
                trade.input_token.amount_raw
            );
        }
        if result.trades.len() > 3 {
            println!("   ... and {} more", result.trades.len() - 3);
        }
    }

    if !result.liquidities.is_empty() {
        println!("\n💧 Liquidity Events:");
        for (i, liq) in result.liquidities.iter().take(3).enumerate() {
            println!(
                "   [{i}] {:?} (pool: {})",
                liq.event_type,
                liq.pool_id
            );
        }
        if result.liquidities.len() > 3 {
            println!("   ... and {} more", result.liquidities.len() - 3);
        }
    }

    if !result.transfers.is_empty() && result.trades.is_empty() {
        println!("\n📤 Transfers:");
        for (i, transfer) in result.transfers.iter().take(3).enumerate() {
            println!(
                "   [{i}] {} -> {} (mint: {})",
                transfer.info.source,
                transfer.info.destination,
                transfer.info.mint
            );
        }
        if result.transfers.len() > 3 {
            println!("   ... and {} more", result.transfers.len() - 3);
        }
    }

    // Show intermediate timings
    let json_time = t_json_end.duration_since(t_json_start);
    let convert_time = t_convert_end.duration_since(t_convert_start);
    let parse_time = t_parse_end.duration_since(t_parse_start);
    
    let total_before_display = t_display_start.duration_since(t_total_start);
    let total_ms = ms(total_before_display);
    
    println!("\n⚡ Performance Breakdown:");
    if total_ms > 0.0 {
        println!("   JSON Parse:  {:.3}ms ({:.1}%)", ms(json_time), 
            ms(json_time) / total_ms * 100.0);
        println!("   Convert TX:  {:.3}ms ({:.1}%)", ms(convert_time),
            ms(convert_time) / total_ms * 100.0);
        println!("   DexParser:   {:.3}ms ({:.1}%)", ms(parse_time),
            ms(parse_time) / total_ms * 100.0);
        println!("\n💡 Note: get_instruction_data() uses base64-simd (optimized)");
    }
}

// === Helpers ===

/// Extract base64 transaction bytes from WebSocket result
/// Format: result.transaction = ["<base64>", "base64"] or result.transaction.transaction = ["<base64>", "base64"]
fn extract_base64_tx(result: &Value) -> Result<Option<Vec<u8>>> {
    // Try result.transaction.transaction first
    if let Some(arr) = result.pointer("/transaction/transaction").and_then(|v| v.as_array()) {
        if arr.len() == 2 {
            if let (Some(b64), Some(enc)) = (arr[0].as_str(), arr[1].as_str()) {
                if enc == "base64" {
                    let bytes = B64.decode_to_vec(b64).context("base64 decode failed")?;
                    return Ok(Some(bytes));
                }
            }
        }
    }
    // Try result.transaction
    if let Some(arr) = result.get("transaction").and_then(|v| v.as_array()) {
        if arr.len() == 2 {
            if let (Some(b64), Some(enc)) = (arr[0].as_str(), arr[1].as_str()) {
                if enc == "base64" {
                    let bytes = B64.decode_to_vec(b64).context("base64 decode failed")?;
                    return Ok(Some(bytes));
                }
            }
        }
    }
    Ok(None)
}

/// Convert binary transaction bytes to SolanaTransaction
/// Uses bincode to deserialize VersionedTransaction, then extracts data
fn convert_binary_to_solana_tx(
    bytes: &[u8],
    slot: u64,
    signature: &str,
    meta: Option<&Value>,
) -> Result<SolanaTransaction> {
    // Deserialize binary transaction
    let versioned_tx: VersionedTransaction = deserialize(bytes)
        .context("Failed to deserialize VersionedTransaction")?;
    
    let message = &versioned_tx.message;
    let account_keys = message.static_account_keys();
    
    // Extract signers (first N accounts where N = num_required_signatures)
    let num_signatures = message.header().num_required_signatures as usize;
    let signers: Vec<String> = account_keys
        .iter()
        .take(num_signatures)
        .map(|pk| bs58::encode(pk.as_ref()).into_string())
        .collect();
    
    // Extract all account keys (static + loaded from ALT if v0)
    let mut all_account_keys: Vec<String> = account_keys
        .iter()
        .map(|pk| bs58::encode(pk.as_ref()).into_string())
        .collect();
    
    // Add loaded addresses from ALT if present
    if let Some(meta_val) = meta {
        if let Some(loaded) = meta_val.pointer("/loadedAddresses") {
            if let Some(writable) = loaded.get("writable").and_then(|v| v.as_array()) {
                for addr in writable {
                    if let Some(s) = addr.as_str() {
                        all_account_keys.push(s.to_string());
                    }
                }
            }
            if let Some(readonly) = loaded.get("readonly").and_then(|v| v.as_array()) {
                for addr in readonly {
                    if let Some(s) = addr.as_str() {
                        all_account_keys.push(s.to_string());
                    }
                }
            }
        }
    }
    
    // Extract instructions
    let instructions: Vec<SolanaInstruction> = message
        .instructions()
        .iter()
        .map(|ix| {
            let program_id = if (ix.program_id_index as usize) < all_account_keys.len() {
                all_account_keys[ix.program_id_index as usize].clone()
            } else {
                "".to_string()
            };
            
            let accounts: Vec<String> = ix
                .accounts
                .iter()
                .filter_map(|&idx| {
                    if (idx as usize) < all_account_keys.len() {
                        Some(all_account_keys[idx as usize].clone())
                    } else {
                        None
                    }
                })
                .collect();
            
            // Encode instruction data as base64 (for get_instruction_data optimization)
            let data_base64 = B64.encode_to_string(&ix.data);
            
            SolanaInstruction {
                program_id,
                accounts,
                data: data_base64,
            }
        })
        .collect();
    
    // Extract inner instructions from meta if present
    let inner_instructions = if let Some(meta_val) = meta {
        extract_inner_instructions(meta_val, &all_account_keys)
    } else {
        Vec::new()
    };
    
    // Extract token balances from meta if present
    let (pre_token_balances, post_token_balances) = if let Some(meta_val) = meta {
        (
            extract_token_balances(meta_val.pointer("/preTokenBalances"), &all_account_keys),
            extract_token_balances(meta_val.pointer("/postTokenBalances"), &all_account_keys),
        )
    } else {
        (Vec::new(), Vec::new())
    };
    
    // Extract transaction meta
    let tx_meta = if let Some(meta_val) = meta {
        extract_transaction_meta(meta_val, &all_account_keys)
    } else {
        TransactionMeta {
            fee: 0,
            compute_units: 0,
            status: TransactionStatus::Success,
            sol_balance_changes: HashMap::new(),
            token_balance_changes: HashMap::new(),
        }
    };
    
    // Extract block time from meta if present
    let block_time = meta
        .and_then(|m| m.get("blockTime").and_then(|v| v.as_u64()))
        .unwrap_or(0);
    
    Ok(SolanaTransaction {
        slot,
        signature: signature.to_string(),
        block_time,
        signers,
        instructions,
        inner_instructions,
        transfers: Vec::new(), // Will be populated by DexParser
        pre_token_balances,
        post_token_balances,
        meta: tx_meta,
    })
}

fn extract_inner_instructions(meta: &Value, account_keys: &[String]) -> Vec<InnerInstruction> {
    let mut result = Vec::new();
    
    if let Some(inner_arr) = meta.get("innerInstructions").and_then(|v| v.as_array()) {
        for group in inner_arr {
            let index = group.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            
            let mut instructions = Vec::new();
            if let Some(ixs) = group.get("instructions").and_then(|v| v.as_array()) {
                for ix_val in ixs {
                    let program_id = ix_val
                        .get("programId")
                        .and_then(|v| v.as_str())
                        .or_else(|| {
                            ix_val.get("programIdIndex")
                                .and_then(|idx| idx.as_u64())
                                .and_then(|idx| account_keys.get(idx as usize))
                                .map(|s| s.as_str())
                        })
                        .unwrap_or("")
                        .to_string();
                    
                    let accounts: Vec<String> = if let Some(acc_arr) = ix_val.get("accounts").and_then(|v| v.as_array()) {
                        acc_arr
                            .iter()
                            .filter_map(|v| {
                                if let Some(s) = v.as_str() {
                                    Some(s.to_string())
                                } else if let Some(idx) = v.as_u64() {
                                    account_keys.get(idx as usize).cloned()
                                } else {
                                    None
                                }
                            })
                            .collect()
                    } else {
                        Vec::new()
                    };
                    
                    // Data might be base58 or base64 - encode as base64 for consistency
                    let data = ix_val
                        .get("data")
                        .and_then(|v| v.as_str())
                        .map(|s| {
                            // If it's base58, decode and re-encode as base64
                            if let Ok(bytes) = bs58::decode(s).into_vec() {
                                B64.encode_to_string(&bytes)
                            } else {
                                // Assume it's already base64 or empty
                                s.to_string()
                            }
                        })
                        .unwrap_or_default();
                    
                    instructions.push(SolanaInstruction {
                        program_id,
                        accounts,
                        data,
                    });
                }
            }
            
            if !instructions.is_empty() {
                result.push(InnerInstruction {
                    index,
                    instructions,
                });
            }
        }
    }
    
    result
}

fn extract_token_balances(meta_opt: Option<&Value>, account_keys: &[String]) -> Vec<TokenBalance> {
    let mut result = Vec::new();
    
    if let Some(balances) = meta_opt.and_then(|v| v.as_array()) {
        for bal_val in balances {
            let account = bal_val
                .get("account")
                .and_then(|v| v.as_u64())
                .and_then(|idx| account_keys.get(idx as usize))
                .cloned()
                .unwrap_or_else(|| {
                    bal_val.get("account")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string()
                });
            
            let mint = bal_val
                .get("mint")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            
            let owner = bal_val
                .get("owner")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            
            let ui_amount = bal_val
                .get("uiTokenAmount")
                .and_then(|v| {
                    let amount = v.get("amount").and_then(|a| a.as_str()).unwrap_or("0");
                    let decimals = v.get("decimals").and_then(|d| d.as_u64()).unwrap_or(0) as u8;
                    let ui_amount = v.get("uiAmount").and_then(|u| u.as_f64());
                    Some(TokenAmount::new(amount, decimals, ui_amount))
                })
                .unwrap_or_default();
            
            result.push(TokenBalance {
                account,
                mint,
                owner,
                ui_token_amount: ui_amount,
            });
        }
    }
    
    result
}

fn extract_transaction_meta(meta: &Value, account_keys: &[String]) -> TransactionMeta {
    let fee = meta.get("fee").and_then(|v| v.as_u64()).unwrap_or(0);
    
    let compute_units = meta
        .get("computeUnitsConsumed")
        .or_else(|| meta.get("computeUnits"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    
    let status = if meta.get("err").is_some() {
        TransactionStatus::Failed
    } else {
        TransactionStatus::Success
    };
    
    let sol_balance_changes = extract_sol_balance_changes(meta, account_keys);
    
    TransactionMeta {
        fee,
        compute_units,
        status,
        sol_balance_changes,
        token_balance_changes: HashMap::new(), // Will be populated by DexParser
    }
}

fn extract_sol_balance_changes(meta: &Value, account_keys: &[String]) -> HashMap<String, BalanceChange> {
    let mut result = HashMap::new();
    
    let pre_balances = meta.get("preBalances").and_then(|v| v.as_array());
    let post_balances = meta.get("postBalances").and_then(|v| v.as_array());
    
    if let Some(balances) = pre_balances {
        for (idx, pre_val) in balances.iter().enumerate() {
            let pre = pre_val.as_i64().unwrap_or(0) as i128;
            let post = post_balances
                .and_then(|arr| arr.get(idx))
                .and_then(|v| v.as_i64())
                .unwrap_or(0) as i128;
            
            if pre != post {
                let account = account_keys
                    .get(idx)
                    .cloned()
                    .unwrap_or_else(|| format!("unknown_{}", idx));
                
                result.insert(account, BalanceChange {
                    pre,
                    post,
                    change: post - pre,
                });
            }
        }
    }
    
    result
}
