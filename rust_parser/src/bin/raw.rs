// src/bin/raw.rs
use anyhow::{anyhow, bail, Context, Result};
use base64_simd::STANDARD;
use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::json;
use std::fmt::Write as _;
use std::time::{Duration, Instant};

// === CLI ===

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Использование: cargo run --bin raw <signature> [rpc_url]");
        std::process::exit(1);
    }

    let signature = &args[1];
    let rpc_url = args
        .get(2)
        .cloned()
        .unwrap_or_else(|| "https://api.mainnet-beta.solana.com".to_string());

    let rpc = Rpc::new(&rpc_url)?;
    println!("🔍 Получаю транзакцию {} через RPC {}...", signature, rpc_url);

    let tx = rpc
        .get_transaction_base64(signature)
        .with_context(|| format!("Не удалось получить транзакцию {}", signature))?;

    println!("✅ Транзакция получена!");
    println!("   Slot: {}", tx.slot);
    println!("   Signature: {}", tx.signature);
    println!("   Instructions: {}", tx.instructions.len());
    println!();

    // — Выводим человеческий разбор —
    print_tx_view(&tx);

    // — Бенч base64-simd на данных инструкций —
    let mut total_base64_len = 0usize;
    let mut total_decoded_len = 0usize;
    let mut decode_times: Vec<Duration> = Vec::new();

    println!("\n📊 Тест декодирования base64 (base64-simd) для всех инструкций...");
    for (i, instruction) in tx.instructions.iter().enumerate() {
        if instruction.data_base64.is_empty() {
            continue;
        }
        total_base64_len += instruction.data_base64.len();

        let start = Instant::now();
        let decoded = STANDARD
            .decode_to_vec(&instruction.data_base64)
            .context("Ошибка декодирования base64")?;
        let elapsed = start.elapsed();

        total_decoded_len += decoded.len();
        decode_times.push(elapsed);

        if i < 3 {
            println!(
                "   Инструкция {}: base64_len={}, decoded_len={}, time={:.3}μs",
                i,
                instruction.data_base64.len(),
                decoded.len(),
                elapsed.as_secs_f64() * 1_000_000.0
            );
        }
    }

    if !decode_times.is_empty() {
        let total_time = decode_times.iter().copied().fold(Duration::ZERO, |acc, d| acc + d);
        let avg_time = total_time / (decode_times.len() as u32);

        println!("\n📈 Статистика:");
        println!("   Всего инструкций с данными: {}", decode_times.len());
        println!("   Общий размер base64: {} байт", total_base64_len);
        println!("   Общий размер декодированных данных: {} байт", total_decoded_len);
        println!("   Общее время декодирования: {:.6}s", total_time.as_secs_f64());
        if total_time.as_secs_f64() > 0.0 {
            println!(
                "⚡ Скорость: {:.0} MB/s",
                total_decoded_len as f64 / 1_000_000.0 / total_time.as_secs_f64()
            );
        }
        let _avg_time = avg_time; // подавить warning, если не используешь avg отдельно
    }

    Ok(())
}

// === RPC слой ===

struct Rpc {
    url: String,
    client: Client,
}

impl Rpc {
    fn new(url: &str) -> Result<Self> {
        let client = Client::builder()
            .user_agent("dex-parser/raw-b64/1.0")
            .timeout(Duration::from_secs(30))
            .tcp_keepalive(Duration::from_secs(30))
            .build()?;
        Ok(Self {
            url: url.to_string(),
            client,
        })
    }

    fn get_transaction_base64(&self, signature: &str) -> Result<TxView> {
        // getTransaction c encoding:"base64" => transaction == ["<base64>", "base64"]
        let body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getTransaction",
            "params": [
                signature,
                { "encoding": "base64", "maxSupportedTransactionVersion": 0 }
            ]
        });

        let resp = self
            .client
            .post(&self.url)
            .json(&body)
            .send()
            .context("RPC запрос не удался")?;

        if !resp.status().is_success() {
            bail!("RPC вернул статус: {}", resp.status());
        }

        let text = resp.text().context("Не удалось прочитать ответ RPC")?;
        // Optimized: parse from bytes instead of string
        let bytes = text.as_bytes();
        let rpc_resp: JsonRpcResponseGetTx =
            serde_json::from_slice(bytes).context("Не удалось распарсить JSON RPC-ответ")?;

        if let Some(err) = rpc_resp.error {
            bail!("RPC ошибка {}: {}", err.code, err.message);
        }

        let result = rpc_resp
            .result
            .ok_or_else(|| anyhow!("RPC вернул пустой result (null)."))?;

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

        let tx_bytes = STANDARD
            .decode_to_vec(&tx_base64)
            .context("Не удалось декодировать base64 транзакции")?;

        parse_transaction_view(&tx_bytes, result.slot, signature)
    }
}

// === Парсинг в удобное представление ===

#[derive(Debug)]
struct TxView {
    slot: u64,
    signature: String,
    version: TxVersion,
    header: Header,
    recent_blockhash: [u8; 32],
    account_keys: Vec<[u8; 32]>,
    instructions: Vec<IxView>,
}

#[derive(Debug, Clone, Copy)]
enum TxVersion {
    Legacy,
    V0,
}

#[derive(Debug, Clone, Copy)]
struct Header {
    num_required_signatures: u8,
    num_readonly_signed_accounts: u8,
    num_readonly_unsigned_accounts: u8,
}

#[derive(Debug)]
struct IxView {
    program_id_index: u8,
    program_id: [u8; 32],
    account_indices: Vec<u8>,
    accounts: Vec<[u8; 32]>,
    data_base64: String,
    data_hex: String,
}

fn parse_transaction_view(bytes: &[u8], slot: u64, sig: &str) -> Result<TxView> {
    let mut p = 0usize;

    // signatures: shortvec + N * 64
    let (num_sigs, n_sig_len) = read_compact_u16(slice_from(bytes, p)?)?;
    p += n_sig_len;
    p = p
        .checked_add(num_sigs as usize * 64)
        .ok_or_else(|| anyhow!("Переполнение при пропуске сигнатур"))?;
    ensure_len(bytes, p)?;

    // message: legacy/v0
    if p >= bytes.len() {
        bail!("Пустое сообщение после сигнатур");
    }
    let versioned = (bytes[p] & 0x80) != 0;
    let version = if versioned { TxVersion::V0 } else { TxVersion::Legacy };
    if versioned {
        p += 1; // съедаем байт версии (старший бит = 1)
    }

    // header (3 байта)
    ensure_len(bytes, p + 3)?;
    let header = Header {
        num_required_signatures: bytes[p],
        num_readonly_signed_accounts: bytes[p + 1],
        num_readonly_unsigned_accounts: bytes[p + 2],
    };
    p += 3;

    // account_keys: shortvec + 32*len
    let (num_accounts, acc_len_size) = read_compact_u16(slice_from(bytes, p)?)?;
    p += acc_len_size;
    let keys_bytes = num_accounts as usize * 32;
    ensure_len(bytes, p + keys_bytes)?;
    let mut account_keys = Vec::with_capacity(num_accounts as usize);
    for i in 0..(num_accounts as usize) {
        let mut k = [0u8; 32];
        k.copy_from_slice(&bytes[p + i * 32..p + (i + 1) * 32]);
        account_keys.push(k);
    }
    p += keys_bytes;

    // recent blockhash (32)
    ensure_len(bytes, p + 32)?;
    let mut rb = [0u8; 32];
    rb.copy_from_slice(&bytes[p..p + 32]);
    p += 32;

    // instructions
    let (num_ix, ix_len_size) = read_compact_u16(slice_from(bytes, p)?)?;
    p += ix_len_size;

    let mut ixs = Vec::with_capacity(num_ix as usize);
    for _ in 0..num_ix {
        ensure_len(bytes, p + 1)?;
        let program_id_index = bytes[p];
        p += 1;

        let (acc_count, acc_len_size) = read_compact_u16(slice_from(bytes, p)?)?;
        p += acc_len_size;
        ensure_len(bytes, p + acc_count as usize)?;
        let account_indices = bytes[p..p + acc_count as usize].to_vec();
        p += acc_count as usize;

        let (data_len, data_len_size) = read_compact_u16(slice_from(bytes, p)?)?;
        p += data_len_size;
        ensure_len(bytes, p + data_len as usize)?;
        let data_bytes = &bytes[p..p + data_len as usize];
        p += data_len as usize;

        // program_id + accounts как байтовые ключи
        let program_id = account_keys
            .get(program_id_index as usize)
            .ok_or_else(|| anyhow!("Некорректный program_id_index"))?;
        let mut pid = [0u8; 32];
        pid.copy_from_slice(program_id);

        let mut accs = Vec::with_capacity(account_indices.len());
        for idx in &account_indices {
            let k = account_keys
                .get(*idx as usize)
                .ok_or_else(|| anyhow!("Некорректный account index"))?;
            let mut kk = [0u8; 32];
            kk.copy_from_slice(k);
            accs.push(kk);
        }

        let data_base64 = STANDARD.encode_to_string(data_bytes);
        let mut data_hex = String::with_capacity(data_bytes.len() * 2);
        for b in data_bytes {
            write!(&mut data_hex, "{:02x}", b).unwrap();
        }

        ixs.push(IxView {
            program_id_index,
            program_id: pid,
            account_indices,
            accounts: accs,
            data_base64,
            data_hex,
        });
    }

    // v0: после инструкций идут address_table_lookups (shortvec<lookup>), можно пропустить
    // если хочешь — добавь тут парсинг LUT для полноты.

    Ok(TxView {
        slot,
        signature: sig.to_string(),
        version,
        header,
        recent_blockhash: rb,
        account_keys,
        instructions: ixs,
    })
}

// === Утилиты ===

fn read_compact_u16(data: &[u8]) -> Result<(u16, usize)> {
    if data.is_empty() {
        bail!("Недостаточно данных для compact-u16");
    }
    let b0 = data[0];
    if b0 <= 0x7f {
        Ok((b0 as u16, 1))
    } else if b0 <= 0xbf {
        if data.len() < 2 {
            bail!("Недостаточно данных для 2-байтового compact-u16");
        }
        let v = (((b0 & 0x3f) as u16) << 8) | data[1] as u16;
        Ok((v, 2))
    } else {
        if data.len() < 3 {
            bail!("Недостаточно данных для 3-байтового compact-u16");
        }
        let v = ((((b0 & 0x1f) as u32) << 16) | ((data[1] as u32) << 8) | data[2] as u32) as u16;
        Ok((v, 3))
    }
}

fn slice_from<'a>(bytes: &'a [u8], pos: usize) -> Result<&'a [u8]> {
    if pos > bytes.len() {
        bail!("Позиция за пределами буфера");
    }
    Ok(&bytes[pos..])
}
fn ensure_len(bytes: &[u8], need: usize) -> Result<()> {
    if need > bytes.len() {
        bail!("Недостаточно данных");
    }
    Ok(())
}

// — печать в человекочитаемом виде —
// Для печати base58 используем bs58 (только для вывода, вне «горячего пути»)
fn b58(pk: &[u8; 32]) -> String {
    bs58::encode(pk).into_string()
}
fn b58_list(v: &[[u8; 32]]) -> Vec<String> {
    v.iter().map(b58).collect()
}
fn hex32(x: &[u8; 32]) -> String {
    let mut s = String::with_capacity(64);
    for b in x {
        write!(&mut s, "{:02x}", b).unwrap();
    }
    s
}

fn print_tx_view(tx: &TxView) {
    println!("— Разбор транзакции —");
    println!("Версия: {:?}", tx.version);
    println!(
        "Header: num_required_signatures={}, ro_signed={}, ro_unsigned={}",
        tx.header.num_required_signatures,
        tx.header.num_readonly_signed_accounts,
        tx.header.num_readonly_unsigned_accounts
    );
    println!("RecentBlockhash: {}", hex32(&tx.recent_blockhash));

    // Аккаунты
    println!("\nАккаунты ({}):", tx.account_keys.len());
    for (i, k) in tx.account_keys.iter().enumerate() {
        println!("  [{}] {}", i, b58(k));
    }

    // Инструкции
    println!("\nИнструкции ({}):", tx.instructions.len());
    for (i, ix) in tx.instructions.iter().enumerate() {
        let acc_b58 = b58_list(&ix.accounts);
        println!("  — Инструкция #{}", i);
        println!("    program_id_index: {} ({})", ix.program_id_index, b58(&ix.program_id));
        if !ix.account_indices.is_empty() {
            println!(
                "    accounts[{}]: {:?}",
                ix.account_indices.len(),
                acc_b58
            );
        } else {
            println!("    accounts: []");
        }
        if !ix.data_base64.is_empty() {
            println!("    data.base64: {}", ix.data_base64);
            println!("    data.hex   : {}", ix.data_hex);
        } else {
            println!("    data: <empty>");
        }
    }
}

// === JSON-модели под getTransaction(base64) ===

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
    #[serde(rename = "transaction")]
    transaction: TxField,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum TxField {
    Encoded(Vec<String>),      // ["<base64>", "base64"]
    Json(serde_json::Value),   // если вдруг encoding != "base64"
}

// === зависимости в Cargo.toml ===
// [dependencies]
// anyhow = "1"
// base64-simd = "0.8"
// reqwest = { version = "0.12", features = ["blocking", "json", "rustls-tls"] }
// serde = { version = "1", features = ["derive"] }
// serde_json = "1"
// bs58 = "0.5"
