// cargo run --release --bin analog_rpc
//
// Rust analog of analog.rs but fetches transaction via RPC by signature hash
// Parses transaction using DexParser and outputs results in the same format
// Все данные (signature и rpc_url) прописаны в коде

use anyhow::{anyhow, bail, Context, Result};
use base64_simd::STANDARD as B64;
use bincode::deserialize;
use bs58;
use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use solana_dex_parser::config::ParseConfig;
use solana_dex_parser::core::dex_parser::DexParser;
use solana_dex_parser::types::{
    BalanceChange, InnerInstruction, SolanaInstruction, SolanaTransaction, TokenAmount,
    TokenBalance, TransactionMeta, TransactionStatus,
};
use solana_sdk::transaction::VersionedTransaction;
use std::collections::HashMap;
use std::time::{Duration, Instant};

const WSOL: &str = "So11111111111111111111111111111111111111112";
const SIGNATURE: &str = "4fesiuBKwrBkE9Aaqv1D8ZTeQPL8Tyd7vQfzfiCJKefTbkrsXqkuEnngwAd2q2uaF5579DFtsSGUTrtuyVYMqUh6"; // Замените на нужный хеш транзакции
const RPC_URL: &str = "https://api.mainnet-beta.solana.com"; // Замените на нужный RPC URL

fn main() -> Result<()> {
    // Initialize tracing subscriber for logging
    tracing_subscriber::fmt()
        .with_target(false)
        .with_thread_ids(false)
        .with_level(true)
        .compact()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("🔍 Получаю транзакцию {} через RPC {}...", SIGNATURE, RPC_URL);

    let t0 = Instant::now();

    // Создаем RPC клиент и делаем запрос с явным указанием base64
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .context("Не удалось создать HTTP клиент")?;

    // RPC запрос с явным указанием base64 encoding
    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getTransaction",
        "params": [
            SIGNATURE,
            {
                "encoding": "base64",
                "maxSupportedTransactionVersion": 0
            }
        ]
    });

    let resp = client
        .post(RPC_URL)
        .json(&body)
        .send()
        .context("RPC запрос не удался")?;

    if !resp.status().is_success() {
        bail!("RPC вернул статус: {}", resp.status());
    }

    let text = resp.text().context("Не удалось прочитать ответ RPC")?;
    let bytes = text.as_bytes();
    let rpc_resp: JsonRpcResponseGetTx =
        serde_json::from_slice(bytes).context("Не удалось распарсить JSON RPC-ответ")?;

    if let Some(err) = rpc_resp.error {
        bail!("RPC ошибка {}: {}", err.code, err.message);
    }

    let result = rpc_resp
        .result
        .ok_or_else(|| anyhow!("RPC вернул пустой result (null)"))?;

    // Извлекаем base64 транзакцию
    let (tx_base64, encoding) = match result.transaction {
        TxField::Encoded(v) => {
            if v.len() != 2 {
                bail!("Неожиданный формат transaction: ожидаю [<base64>,\"base64\"]");
            }
            (v[0].clone(), v[1].clone())
        }
        TxField::Json(_) => bail!("Ожидался base64, а пришел JSON-объект"),
    };

    if encoding.as_str() != "base64" {
        bail!("Ожидалось \"base64\", а пришло \"{}\"", encoding);
    }

    // Декодируем base64 в бинарные данные
    let raw_bytes = B64
        .decode_to_vec(&tx_base64)
        .context("Не удалось декодировать base64 транзакции")?;

    let t_fetched = Instant::now();

    // Конвертируем бинарные данные в SolanaTransaction
    let meta = result.meta.as_ref();
    let slot = result.slot;
    let block_time = result.block_time.unwrap_or(0) as u64;
    let tx = convert_binary_to_solana_tx(&raw_bytes, slot, SIGNATURE, block_time, meta)
        .context("Не удалось конвертировать транзакцию")?;

    println!("✅ Транзакция получена!");

    // Initialize parser
    let parser = DexParser::new();
    let config = ParseConfig {
        try_unknown_dex: true,
        aggregate_trades: false,
        ..Default::default()
    };

    let t_parse0 = Instant::now();
    let res = parser.parse_all(tx, Some(config));
    let t_parsed = Instant::now();

    // Пропускаем failed транзакции
    

    // === Build and print summary ===
    hr();
    // Format ISO timestamp manually
    let (year, month, day, hour, min, sec) = seconds_to_datetime(res.timestamp);
    let datetime = format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.000Z", year, month, day, hour, min, sec);
    println!(
        "🔗 {}  @ slot {}  ({})",
        res.signature, res.slot, datetime
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

    // === Timing breakdown ===
    let fetch_ms = ms(t_fetched.duration_since(t0));
    let parse_ms = ms(t_parsed.duration_since(t_parse0));
    let print_ms = ms(t_printed.duration_since(t_parsed));
    let total_ms = ms(t_printed.duration_since(t0));

    println!(
        "⏱️ Timing: Fetch={:.3}ms  Parse={:.3}ms  Print={:.3}ms  TOTAL={:.3}ms",
        fetch_ms, parse_ms, print_ms, total_ms
    );

    hr();
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

// === RPC Response Structures ===

#[derive(Debug, Deserialize)]
struct JsonRpcResponseGetTx {
    result: Option<GetTxResult>,
    error: Option<RpcError>,
}

#[derive(Debug, Deserialize)]
struct RpcError {
    code: i64,
    message: String,
}

#[derive(Debug, Deserialize)]
struct GetTxResult {
    slot: u64,
    #[serde(rename = "blockTime")]
    block_time: Option<i64>,
    #[serde(rename = "transaction")]
    transaction: TxField,
    meta: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum TxField {
    Encoded(Vec<String>),    // ["<base64>", "base64"]
    Json(Value),              // если вдруг encoding != "base64"
}

/// Convert binary transaction bytes to SolanaTransaction
fn convert_binary_to_solana_tx(
    bytes: &[u8],
    slot: u64,
    signature: &str,
    block_time: u64,
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
        extract_inner_instructions(meta_val, &all_account_keys)
    } else {
        Vec::new()
    };

    // Extract token balances from meta if present
    let (pre_token_balances, post_token_balances) = if let Some(meta_val) = meta {
        let pre = extract_token_balances(meta_val.pointer("/preTokenBalances"), &all_account_keys);
        let post = extract_token_balances(meta_val.pointer("/postTokenBalances"), &all_account_keys);
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
            let account = bal_val
                .get("accountIndex")
                .and_then(|v| v.as_u64())
                .and_then(|idx| account_keys.get(idx as usize))
                .cloned()
                .or_else(|| {
                    bal_val
                        .get("account")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                })
                .or_else(|| {
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

    // Проверяем статус: если err существует и не null, то Failed
    // В TypeScript: if (this.tx.meta.err == null) return 'success'
    let status = if let Some(err_val) = meta.get("err") {
        // Если err не null и не пустой объект, то Failed
        if err_val.is_null() {
            TransactionStatus::Success
        } else {
            TransactionStatus::Failed
        }
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

