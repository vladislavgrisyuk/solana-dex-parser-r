// cargo run --release --bin analog_rpc
//
// Rust analog of analog.rs but fetches transaction via RPC by signature hash
// Parses transaction using DexParser and outputs results in the same format
// Все данные (signature и rpc_url) прописаны в коде

use anyhow::{anyhow, bail, Context, Result};
use base64_simd::STANDARD as B64;
use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use solana_dex_parser::config::ParseConfig;
use solana_dex_parser::core::dex_parser::DexParser;
use solana_dex_parser::core::zero_copy::ZcTransaction;
use solana_dex_parser::types::{ParseResult, TransactionStatus};
use std::time::{Duration, Instant};

const WSOL: &str = "So11111111111111111111111111111111111111112";
const SIGNATURE: &str = "4fesiuBKwrBkE9Aaqv1D8ZTeQPL8Tyd7vQfzfiCJKefTbkrsXqkuEnngwAd2q2uaF5579DFtsSGUTrtuyVYMqUh6"; // Замените на нужный хеш транзакции
const RPC_URL: &str = "https://mainnet.helius-rpc.com/?api-key=d83c5403-fccf-434c-b337-8d1b5b693f49"; // Замените на нужный RPC URL

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

    // Парсим транзакцию используя zero-copy парсинг
    let meta = result.meta.as_ref();
    let slot = result.slot;
    let block_time = result.block_time.unwrap_or(0) as u64;
    
    // Zero-copy парсинг: парсим напрямую из raw bytes
    let zc_tx = ZcTransaction::parse(&raw_bytes, slot, SIGNATURE, block_time, meta)
        .context("Не удалось распарсить транзакцию (zero-copy)")?;

    println!("✅ Транзакция получена!");

    // Initialize parser (один раз)
    let parser = DexParser::new();
    let config = ParseConfig {
        try_unknown_dex: true,
        aggregate_trades: false,
        ..Default::default()
    };

    // ZERO-COPY: используем parse_zc() для максимальной производительности
    // Для pumpswap это полностью zero-copy (без конвертации в SolanaTransaction)
    // Для других протоколов все еще конвертируется, но это будет оптимизировано позже
    const ITERATIONS: usize = 300;
    let mut parse_times = Vec::with_capacity(ITERATIONS);
    
    println!("🔄 Запускаю {} итераций парсинга (zero-copy для pumpswap)...", ITERATIONS);
    
    for i in 0..ITERATIONS {
        let t_parse_start = Instant::now();
        // ZERO-COPY: используем parse_zc() для pumpswap
        // NOTE: zc_tx является ссылкой на буфер, который живет в области видимости
        // Для других протоколов все еще конвертируется в SolanaTransaction
        let _res = parser.parse_zc(&zc_tx, meta, Some(config.clone()))
            .unwrap_or_else(|e| {
                eprintln!("Ошибка парсинга: {}", e);
                ParseResult::new()
            });
        let t_parse_end = Instant::now();
        parse_times.push(t_parse_end.duration_since(t_parse_start));
        
        // Показываем прогресс каждые 50 итераций
        if (i + 1) % 50 == 0 {
            let current_sum: Duration = parse_times.iter().sum();
            let current_avg = current_sum / (i + 1) as u32;
            println!("   {} итераций: среднее = {:.3}ms", i + 1, ms(current_avg));
        }
    }
    
    // Вычисляем среднее время парсинга
    let total_parse_time: Duration = parse_times.iter().sum();
    let avg_parse_time = total_parse_time / parse_times.len() as u32;
    let avg_parse_ms = ms(avg_parse_time);
    
    // Парсим еще раз для вывода результатов (zero-copy)
    let t_parse0 = Instant::now();
    let res = parser.parse_zc(&zc_tx, meta, Some(config))
        .unwrap_or_else(|e| {
            eprintln!("Ошибка парсинга: {}", e);
            ParseResult::new()
        });
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
    println!(
        "📊 Benchmark ({} iterations): Avg Parse={:.3}ms",
        ITERATIONS, avg_parse_ms
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

// NOTE: Old conversion functions removed - now using zero-copy parsing from core::zero_copy module
// All conversion logic is now in core::zero_copy::convert_zc_to_solana_tx()

