// cargo run --release --bin analog
//
// Rust analog of test.ts - WebSocket DEX parser with full timing breakdown
// Subscribes to Helius WebSocket and parses transactions using DexParser

use anyhow::{Context, Result};
use base64_simd::STANDARD as B64;
use bincode::deserialize;
use bs58;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use solana_dex_parser::config::ParseConfig;
use solana_dex_parser::core::dex_parser::DexParser;
use solana_dex_parser::types::{
    BalanceChange, InnerInstruction, SolanaInstruction, SolanaTransaction, TokenAmount,
    TokenBalance, TransactionMeta, TransactionStatus,
};
use solana_sdk::transaction::VersionedTransaction;
use std::collections::HashMap;
use std::time::Instant;
use tokio::time::{interval, Duration, MissedTickBehavior};
use tokio_tungstenite::tungstenite::Message;


const API_KEY: &str = "767f42d9-06c2-46f8-8031-9869035d6ce4";
// Pumpfun и Meteor program IDs для парсинга
const ACCOUNT_INCLUDE: &[&str] = &[
    // Pumpfun
    "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P",
    // Pumpswap
    "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA",
    // Meteor DLMM
    "LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo",
    // Meteor DAMM
    "Eo7WjKq67rjJQSZxS6z3YkapzY3eMj6Xy8X5EQVn5UaB",
    // Meteor DAMM V2
    "cpamdpZCGKUy5JxQXB4dcpGPiikHawvSWAd6mEn1sGG",
    // Meteor DBC
    "dbcij3LWUppWqq96dh6gJWwBifmcGfLSB5D4DuSMaqN",
];
const MAX_EVENTS: usize = 50;
const VERBOSE_JSON: bool = false;
const WSOL: &str = "So11111111111111111111111111111111111111112";

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    // Initialize tracing subscriber for logging
    tracing_subscriber::fmt()
        .with_target(false)
        .with_thread_ids(false)
        .with_level(true)
        .compact()
        .with_max_level(tracing::Level::INFO)
        .init();
    
    let ws_url = format!("wss://atlas-mainnet.helius-rpc.com/?api-key={}", API_KEY);
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
            {
                "accountInclude": ACCOUNT_INCLUDE,
                "vote": false,
                "failed": false
            },
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
    println!("✅ Connected. Subscribing (base64)...");

    // Keepalive pings
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(60));
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

    while let Some(msg) = stream.next().await {
        let t0 = Instant::now(); // старт

        let raw = match msg {
            Ok(Message::Text(t)) => t,
            Ok(Message::Binary(b)) => String::from_utf8_lossy(&b).into_owned(),
            Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_)) => continue,
            Ok(Message::Close(_)) => break,
            Err(e) => {
                eprintln!("WS error: {}", e);
                break;
            }
        };

        // === 1️⃣ JSON parse ===
        let raw_bytes = raw.as_bytes();
        let msg: Value = match serde_json::from_slice(raw_bytes) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let t_json_parsed = Instant::now();

        if msg.get("method").and_then(|m| m.as_str()) != Some("transactionNotification") {
            continue;
        }

        let r = match msg.pointer("/params/result") {
            Some(r) => r,
            None => continue,
        };

        // === 2️⃣ Decode base64 transaction ===
        let tx_raw = r
            .pointer("/transaction/transaction")
            .or_else(|| r.get("transaction"));
        let mut t_decoded = t_json_parsed;

        let tx = match extract_and_decode_tx(tx_raw, r, t_json_parsed, &mut t_decoded) {
            Ok(Some(tx)) => tx,
            Ok(None) => {
                eprintln!("⚠️ decode failed: transaction is not in base64 format");
                continue;
            }
            Err(e) => {
                eprintln!("⚠️ decode failed: {}", e);
                continue;
            }
        };

        // === 3️⃣ Prepare txLike and call parser ===
        let slot = r.get("slot").and_then(|s| s.as_u64()).unwrap_or(0);
        let block_time = r
            .get("blockTime")
            .and_then(|b| b.as_u64())
            .unwrap_or_else(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
            });

        // Сохраняем информацию о программах из инструкций для вывода ошибок
        let tx_programs: Vec<String> = tx.instructions.iter()
            .map(|ix| ix.program_id.clone())
            .collect();

        let t_parse0 = Instant::now();
        let res = parser.parse_all(tx, Some(config.clone()));
        let t_parsed = Instant::now();

        // === 4️⃣ Build and print summary ===
        let signature = r
            .get("signature")
            .and_then(|s| s.as_str())
            .or_else(|| {
                r.pointer("/transaction/signatures")
                    .and_then(|sigs| sigs.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|s| s.as_str())
            })
            .unwrap_or("unknown");

        hr();
        // Format ISO timestamp manually
        let (year, month, day, hour, min, sec) = seconds_to_datetime(block_time);
        let datetime = format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.000Z", year, month, day, hour, min, sec);
        println!(
            "🔗 {}  @ slot {}  ({})",
            signature, slot, datetime
        );

        let status_str = match res.tx_status {
            TransactionStatus::Success => "Success",
            TransactionStatus::Failed => "Failed",
            TransactionStatus::Unknown => "n/a",
        };
        let cu_str = if res.compute_units > 0 {
            res.compute_units.to_string()
        } else {
            "?".to_string()
        };
        let fee_amount = res
            .fee
            .ui_amount
            .unwrap_or_else(|| {
                res.fee
                    .amount
                    .parse::<f64>()
                    .unwrap_or(0.0)
                    / 1_000_000_000.0
            });
        println!(
            "⚙️ status={}  CU={}  fee={:.9} SOL",
            status_str, cu_str, fee_amount
        );

        // Вывод деталей ошибки, если транзакция провалилась
        if res.tx_status == TransactionStatus::Failed {
            if let Some(meta) = r.pointer("/transaction/meta").or_else(|| r.get("meta")) {
                if let Some(err) = meta.get("err") {
                    let err_str = format_error(err);
                    println!("❌ Error: {}", err_str);
                }
                
                // Выводим логи транзакции, если они есть (последние несколько строк)
                if let Some(logs) = meta.get("logMessages").and_then(|v| v.as_array()) {
                    if !logs.is_empty() {
                        let show_count = logs.len().min(5);
                        let start_idx = logs.len().saturating_sub(show_count);
                        println!("📋 Logs (showing last {} of {}):", show_count, logs.len());
                        for (i, log) in logs.iter().skip(start_idx).enumerate() {
                            if let Some(log_str) = log.as_str() {
                                // Обрезаем длинные логи
                                let display_log = if log_str.len() > 120 {
                                    format!("{}...", &log_str[..120])
                                } else {
                                    log_str.to_string()
                                };
                                println!("   [{}] {}", start_idx + i, display_log);
                            }
                        }
                    }
                }
            }
            // Также выводим сообщение из ParseResult, если оно есть
            if let Some(ref msg) = res.msg {
                println!("⚠️ Parser message: {}", msg);
            }
            
            // Выводим информацию о программах
            let mut programs: Vec<String> = res.trades.iter()
                .filter_map(|t| t.program_id.as_ref().or_else(|| t.amm.as_ref()))
                .cloned()
                .collect();
            
            // Если не нашли программы в trades, используем программы из инструкций
            if programs.is_empty() {
                programs = tx_programs.clone();
            }
            
            programs.sort();
            programs.dedup();
            if !programs.is_empty() {
                println!("🔧 Programs involved: {}", programs.iter().map(|p| sh(p)).collect::<Vec<_>>().join(", "));
            }
        }

        if let Some(ref t) = res.aggregate_trade {
            let input_mint_display = if t.input_token.mint == WSOL {
                "SOL"
            } else {
                &sh(&t.input_token.mint)
            };
            let output_mint_display = if t.output_token.mint == WSOL {
                "SOL"
            } else {
                &sh(&t.output_token.mint)
            };
            let amm_str = t
                .amm
                .as_ref()
                .map(|a| format!("| amm={}", a))
                .unwrap_or_default();
            println!(
                "💱 {} {} → {} {} {}",
                fmt_amt(t.input_token.amount, t.input_token.decimals),
                input_mint_display,
                fmt_amt(t.output_token.amount, t.output_token.decimals),
                output_mint_display,
                amm_str
            );
        }

        if !res.trades.is_empty() {
            println!("🛣️ trades ({}):", res.trades.len());
            for (i, t) in res.trades.iter().enumerate() {
                let amm_or_program = t
                    .amm
                    .as_ref()
                    .or_else(|| t.program_id.as_ref())
                    .map(|s| s.as_str())
                    .unwrap_or("DEX");
                println!(
                    "   #{} {}: {} → {}",
                    i + 1,
                    amm_or_program,
                    fmt_amt(t.input_token.amount, t.input_token.decimals),
                    fmt_amt(t.output_token.amount, t.output_token.decimals)
                );
            }
        }

        let t_printed = Instant::now();

        // === 5️⃣ Full timing breakdown ===
        let json_ms = ms(t_json_parsed.duration_since(t0));
        let decode_ms = ms(t_decoded.duration_since(t_json_parsed));
        let parse_ms = ms(t_parsed.duration_since(t_parse0));
        let print_ms = ms(t_printed.duration_since(t_parsed));
        let total_ms = ms(t_printed.duration_since(t0));

        println!(
            "⏱️ Timing: JSON={:.3}ms  Decode={:.3}ms  Parse={:.3}ms  Print={:.3}ms  TOTAL={:.3}ms",
            json_ms, decode_ms, parse_ms, print_ms, total_ms
        );

        if VERBOSE_JSON {
            println!("— raw ParseResult —");
            println!("{:#}", serde_json::to_string_pretty(&res).unwrap_or_default());
        }

        shown += 1;
        if shown >= MAX_EVENTS {
            hr();
            println!("✅ shown {} events — closing", shown);
            break;
        }
    }

    println!("WS closed");
    Ok(())
}

// === Helpers ===

fn ms(d: std::time::Duration) -> f64 {
    d.as_secs_f64() * 1_000.0
}

fn hr() {
    println!("{}", "—".repeat(90));
}

fn sh(x: &str) -> String {
    if x.len() > 12 {
        format!("{}…{}", &x[..4], &x[x.len() - 4..])
    } else {
        x.to_string()
    }
}

fn fmt_amt(amt: f64, dec: u8) -> String {
    let decimals = dec.min(9) as usize;
    format!("{:.decimals$}", amt, decimals = decimals)
}

/// Format transaction error for display
fn format_error(err: &Value) -> String {
    match err {
        Value::Object(obj) => {
            // Try to extract error code and message
            let mut parts = Vec::new();
            
            // Extract InstructionError - most common error type
            if let Some(code) = obj.get("InstructionError") {
                if let Some(arr) = code.as_array() {
                    if arr.len() >= 2 {
                        let idx = arr[0].as_u64().map(|n| n.to_string()).unwrap_or_else(|| "?".to_string());
                        let err_val = &arr[1];
                        
                        // Try to parse error name and details
                        if let Some(err_obj) = err_val.as_object() {
                            if let Some(err_name) = err_obj.keys().next() {
                                let mut error_line = format!("Instruction[{}]: {}", idx, err_name);
                                
                                // Add custom program error code if present
                                if err_name == "Custom" {
                                    if let Some(custom_code) = err_obj.get("Custom") {
                                        if let Some(code_num) = custom_code.as_u64() {
                                            error_line.push_str(&format!(" (code: {})", code_num));
                                        }
                                    }
                                }
                                
                                parts.push(error_line);
                                
                                // Add additional details if present
                                if let Some(details) = err_obj.get(err_name) {
                                    if !details.is_null() && !details.is_object() {
                                        let details_str = serde_json::to_string(details).unwrap_or_default();
                                        if !details_str.is_empty() && details_str != "null" {
                                            parts.push(format!("  → {}", details_str));
                                        }
                                    }
                                }
                            } else {
                                parts.push(format!("Instruction[{}]: {}", idx, serde_json::to_string(err_val).unwrap_or_default()));
                            }
                        } else if let Some(err_str) = err_val.as_str() {
                            parts.push(format!("Instruction[{}]: {}", idx, err_str));
                        } else {
                            parts.push(format!("Instruction[{}]: {}", idx, serde_json::to_string(err_val).unwrap_or_default()));
                        }
                    } else {
                        parts.push(format!("InstructionError: {}", serde_json::to_string(code).unwrap_or_default()));
                    }
                } else {
                    parts.push(format!("InstructionError: {}", serde_json::to_string(code).unwrap_or_default()));
                }
            } 
            // Extract other common error types
            else if let Some(err_str) = obj.keys().next() {
                // Generic error object (e.g., "InsufficientFundsForFee", "AccountNotFound", etc.)
                let error_name = err_str.clone();
                parts.push(error_name.clone());
                
                if let Some(details) = obj.get(&error_name) {
                    if !details.is_null() {
                        let details_str = if details.is_object() || details.is_array() {
                            serde_json::to_string_pretty(details).unwrap_or_default()
                        } else {
                            serde_json::to_string(details).unwrap_or_default()
                        };
                        if !details_str.is_empty() && details_str != "null" {
                            parts.push(format!("  → {}", details_str));
                        }
                    }
                }
            } else {
                // Fallback: pretty print the whole object
                parts.push(serde_json::to_string_pretty(err).unwrap_or_default());
            }
            
            parts.join("\n")
        }
        Value::String(s) => s.clone(),
        Value::Number(n) => format!("Error code: {}", n),
        Value::Array(arr) => {
            arr.iter()
                .enumerate()
                .map(|(i, v)| format!("[{}]: {}", i, format_error(v)))
                .collect::<Vec<_>>()
                .join("\n")
        }
        _ => serde_json::to_string(err).unwrap_or_else(|_| "Unknown error".to_string()),
    }
}

/// Extract and decode transaction from WebSocket result
/// Returns (transaction, decode_time)
fn extract_and_decode_tx(
    tx_raw: Option<&Value>,
    result: &Value,
    t_json_parsed: Instant,
    t_decoded: &mut Instant,
) -> Result<Option<SolanaTransaction>> {
    if let Some(arr) = tx_raw.and_then(|v| v.as_array()) {
        if arr.len() == 2 {
            if let (Some(b64), Some(enc)) = (arr[0].as_str(), arr[1].as_str()) {
                if enc == "base64" {
                    let raw_bytes = B64.decode_to_vec(b64).context("base64 decode failed")?;
                    *t_decoded = Instant::now();
                    let meta = result
                        .pointer("/transaction/meta")
                        .or_else(|| result.get("meta"));
                    let signature = result
                        .get("signature")
                        .and_then(|s| s.as_str())
                        .unwrap_or("unknown");
                    let slot = result.get("slot").and_then(|s| s.as_u64()).unwrap_or(0);
                    let tx = convert_binary_to_solana_tx(&raw_bytes, slot, signature, meta)?;
                    return Ok(Some(tx));
                }
            }
        }
    }
    // No decoding needed, time is same as JSON parse
    *t_decoded = t_json_parsed;
    Ok(None)
}

/// Convert binary transaction bytes to SolanaTransaction
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

            // Encode instruction data as base64
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
        let inner_ixs = extract_inner_instructions(meta_val, &all_account_keys);
        tracing::debug!(
            "Extracted {} inner instruction groups, total instructions: {}",
            inner_ixs.len(),
            inner_ixs.iter().map(|g| g.instructions.len()).sum::<usize>()
        );
        // Log all program IDs found in inner instructions
        let mut inner_programs: std::collections::HashSet<String> = std::collections::HashSet::new();
        for group in &inner_ixs {
            for ix in &group.instructions {
                inner_programs.insert(ix.program_id.clone());
            }
        }
        if !inner_programs.is_empty() {
            tracing::info!(
                "Found {} unique program IDs in inner instructions: {:?}",
                inner_programs.len(),
                inner_programs.iter().collect::<Vec<_>>()
            );
        }
        inner_ixs
    } else {
        Vec::new()
    };

    // Extract token balances from meta if present
    let (pre_token_balances, post_token_balances) = if let Some(meta_val) = meta {
        let pre = extract_token_balances(meta_val.pointer("/preTokenBalances"), &all_account_keys);
        let post = extract_token_balances(meta_val.pointer("/postTokenBalances"), &all_account_keys);
        tracing::debug!(
            "Extracted {} pre_token_balances, {} post_token_balances",
            pre.len(),
            post.len()
        );
        // Log first few balances for debugging
        if !post.is_empty() {
            tracing::debug!(
                "First 3 post_token_balances: {:?}",
                post.iter().take(3).map(|b| format!("account={}, mint={}", b.account, b.mint)).collect::<Vec<_>>()
            );
        }
        (pre, post)
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
                            ix_val
                                .get("programIdIndex")
                                .and_then(|idx| idx.as_u64())
                                .and_then(|idx| account_keys.get(idx as usize))
                                .map(|s| s.as_str())
                        })
                        .unwrap_or("")
                        .to_string();

                    let accounts: Vec<String> = if let Some(acc_arr) =
                        ix_val.get("accounts").and_then(|v| v.as_array())
                    {
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

fn extract_token_balances(
    meta_opt: Option<&Value>,
    account_keys: &[String],
) -> Vec<TokenBalance> {
    let mut result = Vec::new();

    if let Some(balances) = meta_opt.and_then(|v| v.as_array()) {
        for bal_val in balances {
            // In TypeScript: const accountKey = this.accountKeys[balance.accountIndex];
            // So we need to use accountIndex first, then fallback to account as string
            let account = bal_val
                .get("accountIndex")
                .and_then(|v| v.as_u64())
                .and_then(|idx| account_keys.get(idx as usize))
                .cloned()
                .or_else(|| {
                    // Fallback to account as string
                    bal_val
                        .get("account")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                })
                .or_else(|| {
                    // Last fallback: account as number (index)
                    bal_val
                        .get("account")
                        .and_then(|v| v.as_u64())
                        .and_then(|idx| account_keys.get(idx as usize))
                        .cloned()
                })
                .unwrap_or_else(|| "".to_string());

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

fn extract_sol_balance_changes(
    meta: &Value,
    account_keys: &[String],
) -> HashMap<String, BalanceChange> {
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

                result.insert(
                    account,
                    BalanceChange {
                        pre,
                        post,
                        change: post - pre,
                    },
                );
            }
        }
    }

    result
}

/// Convert Unix timestamp to (year, month, day, hour, minute, second)
fn seconds_to_datetime(secs: u64) -> (u32, u32, u32, u32, u32, u32) {
    const SECS_PER_DAY: u64 = 86400;
    const DAYS_PER_YEAR: u64 = 365;
    const DAYS_PER_4_YEARS: u64 = DAYS_PER_YEAR * 4 + 1;
    const DAYS_PER_100_YEARS: u64 = DAYS_PER_4_YEARS * 25 - 1;
    const DAYS_PER_400_YEARS: u64 = DAYS_PER_100_YEARS * 4 + 1;

    let days = secs / SECS_PER_DAY;
    let secs_in_day = secs % SECS_PER_DAY;

    let mut year = 1970u32;
    let mut day = days;

    // Approximate years
    year += (day / DAYS_PER_400_YEARS) as u32 * 400;
    day %= DAYS_PER_400_YEARS;

    year += (day / DAYS_PER_100_YEARS) as u32 * 100;
    day %= DAYS_PER_100_YEARS;

    year += (day / DAYS_PER_4_YEARS) as u32 * 4;
    day %= DAYS_PER_4_YEARS;

    year += (day / DAYS_PER_YEAR) as u32;
    day %= DAYS_PER_YEAR;

    // Simple month calculation (approximate)
    let month_days = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let is_leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
    let mut month = 1u32;
    let mut day_num = day as u32 + 1;

    for (i, &md) in month_days.iter().enumerate() {
        let days_in_month = if i == 1 && is_leap { md + 1 } else { md };
        if day_num > days_in_month {
            day_num -= days_in_month;
            month += 1;
        } else {
            break;
        }
    }

    let hour = (secs_in_day / 3600) as u32;
    let minute = ((secs_in_day % 3600) / 60) as u32;
    let second = (secs_in_day % 60) as u32;

    (year, month, day_num, hour, minute, second)
}

