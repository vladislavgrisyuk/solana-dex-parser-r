use base64_simd::STANDARD;
use serde::de::DeserializeOwned;

use crate::core::transaction_adapter::TransactionAdapter;
use crate::types::{DexInfo, FeeInfo, MemeEvent, TokenInfo, TradeInfo, TradeType, TransferMap};

use super::constants::{
    PUMP_FUN_PROGRAM_ID, PUMP_FUN_PROGRAM_NAME, PUMP_SWAP_PROGRAM_ID, PUMP_SWAP_PROGRAM_NAME,
    SOL_MINT,
};
use super::error::PumpfunError;
use super::pumpswap_event_parser::{
    PumpswapBuyEvent, PumpswapEvent, PumpswapEventData, PumpswapSellEvent,
};

/// Быстрая конвертация raw amount -> ui_amount через таблицу степеней 10
#[inline]
pub fn convert_to_ui_amount(amount: impl Into<u128>, decimals: u8) -> f64 {
    let value: u128 = amount.into();
    if decimals == 0 {
        return value as f64;
    }

    // Для Solana-токенов обычно достаточно 0..=9, но сделаем запас
    const POW10: [f64; 20] = [
        1.0,
        10.0,
        100.0,
        1_000.0,
        10_000.0,
        100_000.0,
        1_000_000.0,
        10_000_000.0,
        100_000_000.0,
        1_000_000_000.0,
        10_000_000_000.0,
        100_000_000_000.0,
        1_000_000_000_000.0,
        10_000_000_000_000.0,
        100_000_000_000_000.0,
        1_000_000_000_000_000.0,
        10_000_000_000_000_000.0,
        100_000_000_000_000_000.0,
        1_000_000_000_000_000_000.0,
        10_000_000_000_000_000_000.0,
    ];

    let d = decimals as usize;
    let scale = if d < POW10.len() {
        POW10[d]
    } else {
        10f64.powi(decimals as i32)
    };

    (value as f64) / scale
}

#[inline]
pub fn get_trade_type(input_mint: &str, output_mint: &str) -> TradeType {
    if input_mint == SOL_MINT {
        TradeType::Buy
    } else if output_mint == SOL_MINT {
        TradeType::Sell
    } else {
        TradeType::Swap
    }
}

/// Типы, которые можно отсортировать по индексу.
/// Уже существующие impl'ы (типа `impl HasIdx for MemeEvent`) можно **не трогать**.
/// Они продолжают реализовывать только `idx(&self) -> &str`, а `idx_key` по умолчанию
/// парсит строку `"outer-inner"` в `(u32, u32)` без аллокаций.
pub trait HasIdx {
    /// Строковый индекс, как раньше, например `"2-4"`.
    fn idx(&self) -> &str;

    /// Числовой ключ сортировки (outer_index, inner_index).
    /// По умолчанию парсит `idx()` как строку.
    /// Если захочешь максимальной скорости — можешь **в любом типе**
    /// переопределить этот метод и возвращать заранее хранимые числа.
    #[inline]
    fn idx_key(&self) -> (u32, u32) {
        parse_idx_str(self.idx())
    }
}

/// Сортировка по индексу через числовой ключ.
/// При существующем `impl HasIdx for MemeEvent` всё продолжит работать как раньше,
/// просто парсинг строки будет более дешёвым.
pub fn sort_by_idx<T>(mut items: Vec<T>) -> Vec<T>
where
    T: HasIdx,
{
    items.sort_by(|a, b| {
        let (a_main, a_sub) = a.idx_key();
        let (b_main, b_sub) = b.idx_key();
        a_main.cmp(&b_main).then_with(|| a_sub.cmp(&b_sub))
    });

    items
}

/// Быстрый парсинг индекса вида "12-3" в (12, 3) без аллокаций и split()
#[inline]
pub fn parse_idx_str(value: &str) -> (u32, u32) {
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i] != b'-' {
        i += 1;
    }

    let (a, b) = if i <= bytes.len() {
        bytes.split_at(i.min(bytes.len()))
    } else {
        (bytes, &[][..])
    };

    let main = std::str::from_utf8(a)
        .ok()
        .and_then(|p| p.parse::<u32>().ok())
        .unwrap_or(0);

    let sub = if i < bytes.len() {
        std::str::from_utf8(&b[1..])
            .ok()
            .and_then(|p| p.parse::<u32>().ok())
            .unwrap_or(0)
    } else {
        0
    };

    (main, sub)
}

/// Fast base64 decoder for instruction data. No caching, no logging, no fallbacks.
#[inline(always)]
pub fn decode_instruction_data(data: &str) -> Result<Vec<u8>, PumpfunError> {
    if data.is_empty() {
        return Ok(Vec::new());
    }
    STANDARD.decode_to_vec(data).map_err(|e| PumpfunError::instruction_data(format!("base64 decode failed: {}", e)))
}

/// Get instruction data bytes. Fast path: no logging.
#[inline(always)]
pub fn get_instruction_data(
    instruction: &crate::types::SolanaInstruction,
) -> Result<Vec<u8>, PumpfunError> {
    decode_instruction_data(&instruction.data)
}

/// Более дешевая версия: возвращаем ссылку на предыдущую инструкцию без clone.
/// Линейный поиск, но без аллокаций.
pub fn get_prev_instruction_by_index<'a>(
    instructions: &'a [crate::types::ClassifiedInstruction],
    outer_index: usize,
    inner_index: Option<usize>,
) -> Option<&'a crate::types::ClassifiedInstruction> {
    let mut prev: Option<&crate::types::ClassifiedInstruction> = None;
    for instr in instructions {
        if instr.outer_index == outer_index && instr.inner_index == inner_index {
            return prev;
        }
        prev = Some(instr);
    }
    None
}

pub fn attach_token_transfers(
    adapter: &TransactionAdapter,
    mut trade: TradeInfo,
    transfers: &TransferMap,
) -> TradeInfo {
    if let Some(ref program_id) = trade.program_id {
        if let Some(entries) = transfers.get(program_id) {
            if let Some(transfer) = entries.iter().find(|entry| {
                entry.info.mint == trade.input_token.mint
                    && entry.info.token_amount.amount == trade.input_token.amount_raw
            }) {
                trade
                    .user
                    .get_or_insert_with(|| transfer.info.source.clone());
            }
        }
    }

    if trade.signer.is_none() {
        // ZERO-COPY: клонируем signers только один раз
        trade.signer = Some(adapter.signers().to_vec());
    }

    trade
}

#[inline]
pub fn build_fee_info(mint: &str, amount: u128, decimals: u8, dex: Option<String>) -> FeeInfo {
    FeeInfo {
        mint: mint.to_string(),
        amount: convert_to_ui_amount(amount, decimals),
        amount_raw: amount.to_string(),
        decimals,
        dex,
        fee_type: None,
        recipient: None,
    }
}

#[inline]
pub fn build_token_info(
    mint: &str,
    amount: u128,
    decimals: u8,
    _owner: Option<String>,
) -> TokenInfo {
    TokenInfo {
        mint: mint.to_string(),
        amount: convert_to_ui_amount(amount, decimals),
        amount_raw: amount.to_string(),
        decimals,
        authority: None,
        destination: None,
        destination_owner: None,
        destination_balance: None,
        destination_pre_balance: None,
        source: None,
        source_balance: None,
        source_pre_balance: None,
        destination_balance_change: None,
        source_balance_change: None,
        balance_change: None,
    }
}

pub fn get_pumpfun_trade_info(
    event: &MemeEvent,
    adapter: &TransactionAdapter,
    dex_info: &DexInfo,
) -> TradeInfo {
    // Здесь логирование обычно не критично, это конструктор структуры.
    TradeInfo {
        trade_type: event.event_type.clone(),
        pool: event
            .pool
            .as_ref()
            .map(|pool| vec![pool.clone()])
            .unwrap_or_default(),
        input_token: event
            .input_token
            .clone()
            .unwrap_or_else(|| build_token_info(&event.base_mint, 0, 6, None)),
        output_token: event
            .output_token
            .clone()
            .unwrap_or_else(|| build_token_info(&event.quote_mint, 0, 9, None)),
        slippage_bps: None,
        fee: None,
        fees: Vec::new(),
        user: Some(event.user.clone()),
        program_id: Some(
            dex_info
                .program_id
                .clone()
                .unwrap_or_else(|| PUMP_FUN_PROGRAM_ID.to_string()),
        ),
        amm: Some(
            dex_info
                .amm
                .clone()
                .unwrap_or_else(|| PUMP_FUN_PROGRAM_NAME.to_string()),
        ),
        amms: None,
        route: Some(dex_info.route.clone().unwrap_or_default()),
        slot: adapter.slot(),
        timestamp: event.timestamp,
        // ZERO-COPY: используем Arc::clone для signature (дешевая операция)
        signature: event.signature.clone(),
        idx: event.idx.clone(),
        // ZERO-COPY: клонируем signers только один раз
        signer: Some(adapter.signers().to_vec()),
    }
}

pub fn get_pumpswap_trade_info(
    event: &PumpswapEvent,
    dex_info: &DexInfo,
    input: (&str, u8, u128),
    output: (&str, u8, u128),
    fee: FeeInfo,
    fees: Vec<FeeInfo>,
    user: String,
) -> TradeInfo {
    let (input_mint, input_decimals, input_amount) = input;
    let (output_mint, output_decimals, output_amount) = output;

    let trade_type = get_trade_type(input_mint, output_mint);
    TradeInfo {
        trade_type,
        pool: match &event.data {
            PumpswapEventData::Buy(data) => vec![data.pool.clone()],
            PumpswapEventData::Sell(data) => vec![data.pool.clone()],
            _ => Vec::new(),
        },
        input_token: build_token_info(input_mint, input_amount, input_decimals, None),
        output_token: build_token_info(output_mint, output_amount, output_decimals, None),
        slippage_bps: None,
        fee: Some(fee),
        fees,
        user: Some(user),
        program_id: Some(
            dex_info
                .program_id
                .clone()
                .unwrap_or_else(|| PUMP_SWAP_PROGRAM_ID.to_string()),
        ),
        amm: Some(
            dex_info
                .amm
                .clone()
                .filter(|a| a != "Unknown DEX")
                .unwrap_or_else(|| PUMP_SWAP_PROGRAM_NAME.to_string()),
        ),
        amms: None,
        route: Some(dex_info.route.clone().unwrap_or_default()),
        slot: event.slot,
        timestamp: event.timestamp,
        signature: event.signature.as_ref().clone(),
        idx: event.idx.clone(),
        signer: event.signer.as_ref().map(|s| s.as_ref().clone()),
    }
}

pub fn build_pumpswap_buy_trade(
    event: &PumpswapEvent,
    buy: &PumpswapBuyEvent,
    input: (&str, u8),
    output: (&str, u8),
    fee: (&str, u8),
    dex_info: &DexInfo,
) -> TradeInfo {
    let (input_mint, input_decimals) = input;
    let (output_mint, output_decimals) = output;
    let (fee_mint, fee_decimals) = fee;

    let total_fee = (buy.protocol_fee + buy.coin_creator_fee) as u128;

    let mut fees = Vec::with_capacity(2);
    fees.push(FeeInfo {
        mint: fee_mint.to_string(),
        amount: convert_to_ui_amount(buy.protocol_fee as u128, fee_decimals),
        amount_raw: buy.protocol_fee.to_string(),
        decimals: fee_decimals,
        dex: Some(PUMP_SWAP_PROGRAM_NAME.to_string()),
        fee_type: Some("protocol".to_string()),
        recipient: Some(buy.protocol_fee_recipient.clone()),
    });
    if buy.coin_creator_fee > 0 {
        fees.push(FeeInfo {
            mint: fee_mint.to_string(),
            amount: convert_to_ui_amount(buy.coin_creator_fee as u128, fee_decimals),
            amount_raw: buy.coin_creator_fee.to_string(),
            decimals: fee_decimals,
            dex: Some(PUMP_SWAP_PROGRAM_NAME.to_string()),
            fee_type: Some("coinCreator".to_string()),
            recipient: Some(buy.coin_creator.clone()),
        });
    }

    let fee_info = FeeInfo {
        mint: fee_mint.to_string(),
        amount: convert_to_ui_amount(total_fee, fee_decimals),
        amount_raw: total_fee.to_string(),
        decimals: fee_decimals,
        dex: None,
        fee_type: None,
        recipient: None,
    };

    get_pumpswap_trade_info(
        event,
        dex_info,
        (
            input_mint,
            input_decimals,
            buy.quote_amount_in_with_lp_fee as u128,
        ),
        (output_mint, output_decimals, buy.base_amount_out as u128),
        fee_info,
        fees,
        buy.user.clone(),
    )
}

pub fn build_pumpswap_sell_trade(
    event: &PumpswapEvent,
    sell: &PumpswapSellEvent,
    input: (&str, u8),
    output: (&str, u8),
    fee: (&str, u8),
    dex_info: &DexInfo,
) -> TradeInfo {
    let (input_mint, input_decimals) = input;
    let (output_mint, output_decimals) = output;
    let (fee_mint, fee_decimals) = fee;

    let total_fee = (sell.protocol_fee + sell.coin_creator_fee) as u128;

    let mut fees = Vec::with_capacity(2);
    fees.push(FeeInfo {
        mint: fee_mint.to_string(),
        amount: convert_to_ui_amount(sell.protocol_fee as u128, fee_decimals),
        amount_raw: sell.protocol_fee.to_string(),
        decimals: fee_decimals,
        dex: Some(PUMP_SWAP_PROGRAM_NAME.to_string()),
        fee_type: Some("protocol".to_string()),
        recipient: Some(sell.protocol_fee_recipient.clone()),
    });
    if sell.coin_creator_fee > 0 {
        fees.push(FeeInfo {
            mint: fee_mint.to_string(),
            amount: convert_to_ui_amount(sell.coin_creator_fee as u128, fee_decimals),
            amount_raw: sell.coin_creator_fee.to_string(),
            decimals: fee_decimals,
            dex: Some(PUMP_SWAP_PROGRAM_NAME.to_string()),
            fee_type: Some("coinCreator".to_string()),
            recipient: Some(sell.coin_creator.clone()),
        });
    }

    let fee_info = FeeInfo {
        mint: fee_mint.to_string(),
        amount: convert_to_ui_amount(total_fee, fee_decimals),
        amount_raw: total_fee.to_string(),
        decimals: fee_decimals,
        dex: None,
        fee_type: None,
        recipient: None,
    };

    get_pumpswap_trade_info(
        event,
        dex_info,
        (input_mint, input_decimals, sell.base_amount_in as u128),
        (
            output_mint,
            output_decimals,
            sell.user_quote_amount_out as u128,
        ),
        fee_info,
        fees,
        sell.user.clone(),
    )
}

#[inline]
pub fn parse_json_value<T: DeserializeOwned>(
    value: serde_json::Value,
) -> Result<T, PumpfunError> {
    // Direct deserialization from Value (no clone needed, Value is moved)
    serde_json::from_value(value).map_err(PumpfunError::from)
}
