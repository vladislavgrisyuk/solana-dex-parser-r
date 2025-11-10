// cargo run --release --bin ws_raw -- <API_KEY> [MINT1,MINT2,...]

use anyhow::{anyhow, bail, Context, Result};
use base64_simd::STANDARD as B64;
use bs58;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::fmt::Write as _;
use std::time::Instant;
use tokio::time::{interval, Duration};
use tokio_tungstenite::tungstenite::Message;

// === Entry ===

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let api_key = "767f42d9-06c2-46f8-8031-9869035d6ce4".to_string();
    let include_mints: Vec<String> = args
        .next()
        .unwrap_or_else(|| "EKpQGSJtjMFqKZ9KQanSqYXRcF8fBopzLHYxdM65zcjm".to_string())
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let ws_url = format!("wss://atlas-mainnet.helius-rpc.com/?api-key={}", api_key);
    println!("🔌 connecting {}", ws_url);

    let (ws_stream, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .context("ws connect failed")?;
    let (mut sink, mut stream) = ws_stream.split();

    // subscribe: base64 + full + v0 support
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
        .context("send subscribe")?;
    println!("✅ subscribed (encoding=base64, details=full, mints={:?})", include_mints);

    // keepalive pings (Atlas любит пинг)
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(30));
        loop {
            ticker.tick().await;
            // we can't send from here (we moved sink), but tokio-tungstenite keeps TCP alive fine.
            // If нужно — оформляй через mpsc канал и прокидывай ping -> sink.send(Message::Ping(vec![]))
        }
    });

    let mut shown = 0usize;
    const MAX_EVENTS: usize = 50;

    while let Some(msg) = stream.next().await {
        let raw = match msg {
            Ok(Message::Text(t)) => t,
            Ok(Message::Binary(b)) => String::from_utf8_lossy(&b).into_owned(),
            Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_)) => continue,
            Ok(Message::Close(_)) => break,
            Err(e) => {
                eprintln!("ws error: {e}");
                break;
            }
        };

        let t0 = Instant::now();

        // 1) JSON parse with serde_json::from_slice (faster than from_str, no string copy)
        let raw_bytes = raw.as_bytes();
        let v: Value = match serde_json::from_slice(raw_bytes) {
            Ok(v) => v,
            Err(_) => continue,
        };
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

        // 2) Try extract base64 bytes first (fast path)
        let t_json = Instant::now();
        match extract_base64_tx(result) {
            Ok(Some(bytes)) => {
                let (lw, lr) = extract_loaded_addresses(result);
                let t_b64 = Instant::now();
                match parse_transaction_view(&bytes, slot, signature, &lw, &lr) {
                    Ok(txv) => {
                        let t_parsed = Instant::now();
                        print_pretty(&txv);
                        let t_printed = Instant::now();
                        timing("B64",
                               t0, t_json, t_b64, t_parsed, t_printed);
                    }
                    Err(e) => eprintln!("⚠️ parse(bytes) error: {e}"),
                }
            }
            Ok(None) => {
                // 3) Fallback: provider прислал json/jsonParsed
                eprintln!("ℹ️  transaction is JSON (not base64) — using json fallback");
                match parse_json_transaction_view(result, slot, signature) {
                    Ok(txv) => {
                        let t_parsed = Instant::now();
                        print_pretty(&txv);
                        let t_printed = Instant::now();
                        // тайминги без отдельной decode стадии
                        println!(
                            "⏱️ Timing: JSON={:.3}ms  Decode=—  Parse={:.3}ms  Print={:.3}ms  TOTAL={:.3}ms",
                            ms(t_json.duration_since(t0)),
                            ms(t_parsed.duration_since(t_json)),
                            ms(t_printed.duration_since(t_parsed)),
                            ms(t_printed.duration_since(t0)),
                        );
                    }
                    Err(e) => eprintln!("⚠️ parse(json) error: {e}"),
                }
            }
            Err(e) => eprintln!("⚠️ extract_base64_tx error: {e}"),
        }

        shown += 1;
        if shown >= MAX_EVENTS {
            println!("✅ shown {} events — exit", shown);
            break;
        }
    }

    Ok(())
}

// === Helpers ===

fn ms(d: std::time::Duration) -> f64 {
    (d.as_secs_f64() * 1_000.0)
}

fn timing(kind: &str, t0: Instant, t_json: Instant, t_b64: Instant, t_parsed: Instant, t_printed: Instant) {
    let json_ms = ms(t_json.duration_since(t0));
    let dec_ms  = ms(t_b64.duration_since(t_json));
    let par_ms  = ms(t_parsed.duration_since(t_b64));
    let prn_ms  = ms(t_printed.duration_since(t_parsed));
    let tot_ms  = ms(t_printed.duration_since(t0));
    println!("⏱️ Timing[{kind}]: JSON={json_ms:.3}ms  Decode={dec_ms:.3}ms  Parse={par_ms:.3}ms  Print={prn_ms:.3}ms  TOTAL={tot_ms:.3}ms");
}

/// Пытаемся вытащить base64-массив из result.transaction.*
/// Форматы из доков Helius:
/// - result.transaction.transaction = ["<base64>", "base64"]
/// - либо напрямую result.transaction = ["<base64>", "base64"]
fn extract_base64_tx(result: &Value) -> Result<Option<Vec<u8>>> {
    // 1) result.transaction.transaction = ["..","base64"]
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
    // 2) result.transaction == ["..","base64"]
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

/// Извлекает загруженные адреса из ALT из meta.loadedAddresses
fn extract_loaded_addresses(result: &Value) -> (Vec<[u8;32]>, Vec<[u8;32]>) {
    fn to32(s: &str) -> Result<[u8;32]> {
        let v = bs58::decode(s).into_vec().context("b58 decode")?;
        anyhow::ensure!(v.len() == 32, "pubkey not 32 bytes");
        let mut out = [0u8; 32];
        out.copy_from_slice(&v);
        Ok(out)
    }

    let mut w = Vec::new();
    let mut r = Vec::new();

    if let Some(arr) = result.pointer("/transaction/meta/loadedAddresses/writable").and_then(|v| v.as_array()) {
        for v in arr {
            if let Some(s) = v.as_str() {
                if let Ok(pk) = to32(s) {
                    w.push(pk);
                }
            }
        }
    }

    if let Some(arr) = result.pointer("/transaction/meta/loadedAddresses/readonly").and_then(|v| v.as_array()) {
        for v in arr {
            if let Some(s) = v.as_str() {
                if let Ok(pk) = to32(s) {
                    r.push(pk);
                }
            }
        }
    }

    (w, r)
}

// === Byte-level parser (как в твоём HTTP-бине) ===

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

fn parse_transaction_view(
    bytes: &[u8],
    slot: u64,
    sig: &str,
    loaded_writable: &[[u8;32]],
    loaded_readonly: &[[u8;32]],
) -> Result<TxView> {
    use anyhow::ensure;

    let mut p = 0usize;

    // signatures
    let (num_sigs, n_sig_len) = read_compact_u16(&bytes[p..])?;
    p += n_sig_len;
    ensure!(p + num_sigs as usize * 64 <= bytes.len(), "sigs oob");
    p += num_sigs as usize * 64;

    // version / legacy
    ensure!(p < bytes.len(), "empty message");
    let versioned = (bytes[p] & 0x80) != 0;
    let version = if versioned { TxVersion::V0 } else { TxVersion::Legacy };
    if versioned { p += 1; }

    // header
    ensure!(p + 3 <= bytes.len(), "no header");
    let header = Header {
        num_required_signatures: bytes[p],
        num_readonly_signed_accounts: bytes[p+1],
        num_readonly_unsigned_accounts: bytes[p+2],
    };
    p += 3;

    // static account keys
    let (n_keys, n_len) = read_compact_u16(&bytes[p..])?;
    p += n_len;
    let keys_bytes = n_keys as usize * 32;
    ensure!(p + keys_bytes <= bytes.len(), "keys oob");
    let mut static_keys = Vec::with_capacity(n_keys as usize);
    for i in 0..(n_keys as usize) {
        let mut k = [0u8; 32];
        k.copy_from_slice(&bytes[p + i*32 .. p + (i+1)*32]);
        static_keys.push(k);
    }
    p += keys_bytes;

    // recent blockhash
    ensure!(p + 32 <= bytes.len(), "rb oob");
    let mut rb = [0u8; 32];
    rb.copy_from_slice(&bytes[p..p+32]);
    p += 32;

    // compiled instructions
    let (n_ix, n_ix_len) = read_compact_u16(&bytes[p..])?;
    p += n_ix_len;

    // Сперва собираем «сырой» вид без резолва ключей:
    struct RawIx {
        pid_idx: u8,
        acc_idx: Vec<u8>,
        data: Vec<u8>,
    }

    let mut raw_ixs: Vec<RawIx> = Vec::with_capacity(n_ix as usize);
    for _ in 0..n_ix {
        ensure!(p < bytes.len(), "ix header oob");
        let pid_idx = bytes[p];
        p += 1;

        let (acc_cnt, acc_len) = read_compact_u16(&bytes[p..])?;
        p += acc_len;
        ensure!(p + acc_cnt as usize <= bytes.len(), "ix accounts oob");
        let acc_idx = bytes[p..p + acc_cnt as usize].to_vec();
        p += acc_cnt as usize;

        let (dl, dl_len) = read_compact_u16(&bytes[p..])?;
        p += dl_len;
        ensure!(p + dl as usize <= bytes.len(), "ix data oob");
        let data = bytes[p..p + dl as usize].to_vec();
        p += dl as usize;

        raw_ixs.push(RawIx { pid_idx, acc_idx, data });
    }

    // v0: за инструкциями идут address table lookups → просто пропустим байты,
    // чтобы корректно прочитать весь message (резолв делаем через meta.loadedAddresses):
    if versioned {
        let (n_luts, n_luts_len) = read_compact_u16(&bytes[p..])?;
        p += n_luts_len;
        for _ in 0..n_luts {
            // table account pubkey
            ensure!(p + 32 <= bytes.len(), "lut pubkey oob");
            p += 32;

            // writable indices
            let (nw, nlw) = read_compact_u16(&bytes[p..])?;
            p += nlw;
            ensure!(p + nw as usize <= bytes.len(), "lut writable idx oob");
            p += nw as usize;

            // readonly indices
            let (nr, nlr) = read_compact_u16(&bytes[p..])?;
            p += nlr;
            ensure!(p + nr as usize <= bytes.len(), "lut readonly idx oob");
            p += nr as usize;
        }
    }

    // Собираем общий пул ключей: static + loaded(writable, readonly)
    let mut all_keys = static_keys.clone();
    all_keys.extend_from_slice(loaded_writable);
    all_keys.extend_from_slice(loaded_readonly);

    // Теперь создаём IxView с безопасным резолвом
    let mut ixs: Vec<IxView> = Vec::with_capacity(raw_ixs.len());
    for raw in raw_ixs {
        let program_id = all_keys.get(raw.pid_idx as usize).copied().unwrap_or([0u8; 32]);

        let mut accounts = Vec::with_capacity(raw.acc_idx.len());
        for idx in &raw.acc_idx {
            if let Some(pk) = all_keys.get(*idx as usize) {
                accounts.push(*pk);
            }
        }

        let data_base64 = B64.encode_to_string(&raw.data);
        let mut data_hex = String::with_capacity(raw.data.len() * 2);
        for b in &raw.data {
            write!(&mut data_hex, "{:02x}", b).unwrap();
        }

        ixs.push(IxView {
            program_id_index: raw.pid_idx,
            program_id,
            account_indices: raw.acc_idx,
            accounts,
            data_base64,
            data_hex,
        });
    }

    Ok(TxView {
        slot,
        signature: sig.to_string(),
        version,
        header,
        recent_blockhash: rb,
        account_keys: all_keys, // <- тут уже общий список
        instructions: ixs,
    })
}

// === JSON fallback parser (для json/jsonParsed) ===
// На практике в jsonParsed: message.accountKeys = [{pubkey, signer, writable}]
// instructions: у многих программ поле `data` — base58 строка; декодируем её.

fn parse_json_transaction_view(result: &Value, slot: u64, sig: &str) -> Result<TxView> {
    let tx_obj = result
        .get("transaction")
        .ok_or_else(|| anyhow!("no transaction"))?;

    // где-то transaction={ transaction:{...}, meta:{...} }, а где-то сразу { message:{...}, ... }
    let inner = tx_obj.get("transaction").unwrap_or(tx_obj);

    let msg = inner
        .get("message")
        .ok_or_else(|| anyhow!("no message in json tx"))?;

    // header
    let header = msg
        .get("header")
        .ok_or_else(|| anyhow!("no header"))?;
    let hdr = Header {
        num_required_signatures: header.get("numRequiredSignatures").and_then(|v| v.as_u64()).unwrap_or(0) as u8,
        num_readonly_signed_accounts: header.get("numReadonlySignedAccounts").and_then(|v| v.as_u64()).unwrap_or(0) as u8,
        num_readonly_unsigned_accounts: header.get("numReadonlyUnsignedAccounts").and_then(|v| v.as_u64()).unwrap_or(0) as u8,
    };

    // account keys — либо список строк, либо объектов с pubkey
    let mut account_keys: Vec<[u8;32]> = Vec::new();
    if let Some(arr) = msg.get("accountKeys").and_then(|v| v.as_array()) {
        for item in arr {
            let s = if let Some(pk) = item.get("pubkey").and_then(|v| v.as_str()) {
                pk
            } else if let Some(pk) = item.as_str() {
                pk
            } else {
            continue;
            };
            account_keys.push(pk_to32(s)?);
        }
    }

    // recentBlockhash
    let mut rb = [0u8; 32];
    if let Some(rb58) = msg.get("recentBlockhash").and_then(|v| v.as_str()) {
        rb.copy_from_slice(&pk_to32(rb58)?);
    }

    // instructions:
    let mut ixs: Vec<IxView> = Vec::new();
    let ix_arr = msg.get("instructions").and_then(|v| v.as_array()).ok_or_else(|| anyhow!("no instructions"))?;
    for ixv in ix_arr {
        // programId может быть строкой (jsonParsed) или индексом (json)
        let (program_id_index, program_id) = if let Some(pid_str) = ixv.get("programId").and_then(|v| v.as_str()) {
            // map pid_str into index if present; иначе просто ставим 0xFF
            let pid_bytes = pk_to32(pid_str)?;
            let idx = account_keys.iter().position(|k| k == &pid_bytes).map(|i| i as u8).unwrap_or(0xFF);
            (idx, pid_bytes)
        } else if let Some(idx) = ixv.get("programIdIndex").and_then(|v| v.as_u64()) {
            let idx_u = idx as u8;
            let pid = account_keys.get(idx as usize).ok_or_else(|| anyhow!("bad programIdIndex"))?;
            (idx_u, *pid)
        } else {
            (0xFF, [0u8;32])
        };

        // accounts: либо массив строк pubkey, либо массив индексов
        let mut account_indices: Vec<u8> = Vec::new();
        let mut accounts: Vec<[u8;32]> = Vec::new();

        if let Some(accs) = ixv.get("accounts").and_then(|v| v.as_array()) {
            if accs.first().and_then(|x| x.as_str()).is_some() {
                for s in accs {
                    let pk = pk_to32(s.as_str().unwrap())?;
                    accounts.push(pk);
                    // попробуем найти индекс
                    if let Some(idx) = account_keys.iter().position(|k| k == &pk) {
                        account_indices.push(idx as u8);
                    }
                }
            } else if accs.first().and_then(|x| x.as_u64()).is_some() {
                for idxv in accs {
                    let idx = idxv.as_u64().unwrap() as usize;
                    account_indices.push(idx as u8);
                    accounts.push(*account_keys.get(idx).ok_or_else(|| anyhow!("bad account index"))?);
                }
            }
        }

        // data: строка — чаще base58; если не декодится как base58, пробуем base64; иначе пусто
        let mut data_bytes: Vec<u8> = Vec::new();
        if let Some(dstr) = ixv.get("data").and_then(|v| v.as_str()) {
            if let Ok(b) = bs58::decode(dstr).into_vec() {
                data_bytes = b;
            } else if let Ok(b) = B64.decode_to_vec(dstr) {
                data_bytes = b;
            }
        }

        let data_base64 = B64.encode_to_string(&data_bytes);
        let mut data_hex = String::with_capacity(data_bytes.len()*2);
        for b in &data_bytes { write!(&mut data_hex, "{:02x}", b).unwrap(); }

        ixs.push(IxView {
            program_id_index,
            program_id,
            account_indices,
            accounts,
            data_base64,
            data_hex,
        });
    }

    Ok(TxView {
        slot,
        signature: sig.to_string(),
        version: TxVersion::V0, // нельзя надёжно узнать из jsonParsed → пусть будет V0/Legacy не критично
        header: hdr,
        recent_blockhash: rb,
        account_keys,
        instructions: ixs,
    })
}

// === Misc ===

fn read_compact_u16(data: &[u8]) -> Result<(u16, usize)> {
    if data.is_empty() { bail!("short compact-u16"); }
    let b0 = data[0];
    if b0 <= 0x7f { Ok((b0 as u16, 1)) }
    else if b0 <= 0xbf {
        if data.len() < 2 { bail!("short 2b compact"); }
        Ok((((b0 & 0x3f) as u16) << 8 | data[1] as u16, 2))
    } else {
        if data.len() < 3 { bail!("short 3b compact"); }
        Ok(((((b0 & 0x1f) as u32) << 16 | ((data[1] as u32) << 8) | data[2] as u32) as u16, 3))
    }
}

fn pk_to32(b58: &str) -> Result<[u8;32]> {
    let v = bs58::decode(b58).into_vec().context("b58 decode")?;
    if v.len() != 32 { bail!("pubkey not 32 bytes"); }
    let mut out = [0u8; 32];
    out.copy_from_slice(&v);
    Ok(out)
}

fn b58(pk: &[u8;32]) -> String { bs58::encode(pk).into_string() }
fn hex32(x: &[u8;32]) -> String {
    let mut s = String::with_capacity(64);
    for b in x { write!(&mut s, "{:02x}", b).unwrap(); }
    s
}

fn print_pretty(tx: &TxView) {
    println!("—{}","—".repeat(88));
    println!("🔗 {}  @ slot {}", tx.signature, tx.slot);
    println!("Header: sigs={}, ro_signed={}, ro_unsigned={}",
        tx.header.num_required_signatures,
        tx.header.num_readonly_signed_accounts,
        tx.header.num_readonly_unsigned_accounts
    );
    println!("RecentBlockhash: {}", hex32(&tx.recent_blockhash));

    println!("\nАккаунты ({}):", tx.account_keys.len());
    for (i, k) in tx.account_keys.iter().enumerate() {
        println!("  [{i}] {}", b58(k));
    }

    println!("\nИнструкции ({}):", tx.instructions.len());
    for (i, ix) in tx.instructions.iter().enumerate() {
        println!("  — Инструкция #{i}");
        println!("    program_id_index: {} ({})", ix.program_id_index, b58(&ix.program_id));
        if ix.accounts.is_empty() {
            println!("    accounts: []");
        } else {
            let acc_list: Vec<String> = ix.accounts.iter().map(b58).collect();
            println!("    accounts[{}]: {:?}", ix.accounts.len(), acc_list);
        }
        if !ix.data_base64.is_empty() {
            println!("    data.base64: {}", ix.data_base64);
            println!("    data.hex   : {}", ix.data_hex);
        } else {
            println!("    data: <empty>");
        }
    }
}
