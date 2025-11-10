use crate::core::transaction_adapter::TransactionAdapter;
use crate::types::ClassifiedInstruction;

use super::binary_reader::BinaryReader;
use super::constants::discriminators::pumpswap_events;
use super::error::PumpfunError;
use super::util::{get_instruction_data, sort_by_idx, HasIdx};

use std::time::Instant;
use tracing::{debug, info};

#[derive(Clone, Debug, PartialEq)]
pub enum PumpswapEventType {
    Create,
    Add,
    Remove,
    Buy,
    Sell,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PumpswapEvent {
    pub event_type: PumpswapEventType,
    pub data: PumpswapEventData,
    pub slot: u64,
    pub timestamp: u64,
    pub signature: String,
    pub idx: String,
    pub signer: Option<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PumpswapEventData {
    Buy(PumpswapBuyEvent),
    Sell(PumpswapSellEvent),
    Create(PumpswapCreatePoolEvent),
    Deposit(PumpswapDepositEvent),
    Withdraw(PumpswapWithdrawEvent),
}

#[derive(Clone, Debug, PartialEq)]
pub struct PumpswapBuyEvent {
    pub timestamp: u64,
    pub base_amount_out: u64,
    pub max_quote_amount_in: u64,
    pub user_base_token_reserves: u64,
    pub user_quote_token_reserves: u64,
    pub pool_base_token_reserves: u64,
    pub pool_quote_token_reserves: u64,
    pub quote_amount_in: u64,
    pub lp_fee_basis_points: u64,
    pub lp_fee: u64,
    pub protocol_fee_basis_points: u64,
    pub protocol_fee: u64,
    pub quote_amount_in_with_lp_fee: u64,
    pub user_quote_amount_in: u64,
    pub pool: String,
    pub user: String,
    pub user_base_token_account: String,
    pub user_quote_token_account: String,
    pub protocol_fee_recipient: String,
    pub protocol_fee_recipient_token_account: String,
    pub coin_creator: String,
    pub coin_creator_fee_basis_points: u64,
    pub coin_creator_fee: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PumpswapSellEvent {
    pub timestamp: u64,
    pub base_amount_in: u64,
    pub min_quote_amount_out: u64,
    pub user_base_token_reserves: u64,
    pub user_quote_token_reserves: u64,
    pub pool_base_token_reserves: u64,
    pub pool_quote_token_reserves: u64,
    pub quote_amount_out: u64,
    pub lp_fee_basis_points: u64,
    pub lp_fee: u64,
    pub protocol_fee_basis_points: u64,
    pub protocol_fee: u64,
    pub quote_amount_out_without_lp_fee: u64,
    pub user_quote_amount_out: u64,
    pub pool: String,
    pub user: String,
    pub user_base_token_account: String,
    pub user_quote_token_account: String,
    pub protocol_fee_recipient: String,
    pub protocol_fee_recipient_token_account: String,
    pub coin_creator: String,
    pub coin_creator_fee_basis_points: u64,
    pub coin_creator_fee: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PumpswapCreatePoolEvent {
    pub timestamp: u64,
    pub index: u16,
    pub creator: String,
    pub base_mint: String,
    pub quote_mint: String,
    pub base_mint_decimals: u8,
    pub quote_mint_decimals: u8,
    pub base_amount_in: u64,
    pub quote_amount_in: u64,
    pub pool_base_amount: u64,
    pub pool_quote_amount: u64,
    pub minimum_liquidity: u64,
    pub initial_liquidity: u64,
    pub lp_token_amount_out: u64,
    pub pool_bump: u8,
    pub pool: String,
    pub lp_mint: String,
    pub user_base_token_account: String,
    pub user_quote_token_account: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PumpswapDepositEvent {
    pub timestamp: u64,
    pub lp_token_amount_out: u64,
    pub max_base_amount_in: u64,
    pub max_quote_amount_in: u64,
    pub user_base_token_reserves: u64,
    pub user_quote_token_reserves: u64,
    pub pool_base_token_reserves: u64,
    pub pool_quote_token_reserves: u64,
    pub base_amount_in: u64,
    pub quote_amount_in: u64,
    pub lp_mint_supply: u64,
    pub pool: String,
    pub user: String,
    pub user_base_token_account: String,
    pub user_quote_token_account: String,
    pub user_pool_token_account: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PumpswapWithdrawEvent {
    pub timestamp: u64,
    pub lp_token_amount_in: u64,
    pub min_base_amount_out: u64,
    pub min_quote_amount_out: u64,
    pub user_base_token_reserves: u64,
    pub user_quote_token_reserves: u64,
    pub pool_base_token_reserves: u64,
    pub pool_quote_token_reserves: u64,
    pub base_amount_out: u64,
    pub quote_amount_out: u64,
    pub lp_mint_supply: u64,
    pub pool: String,
    pub user: String,
    pub user_base_token_account: String,
    pub user_quote_token_account: String,
    pub user_pool_token_account: String,
}

#[inline]
fn since_us(t: Instant) -> u128 { t.elapsed().as_micros() }

pub struct PumpswapEventParser {
    adapter: TransactionAdapter,
}

impl PumpswapEventParser {
    pub fn new(adapter: TransactionAdapter) -> Self {
        Self { adapter }
    }

    pub fn parse_instructions(
        &self,
        instructions: &[ClassifiedInstruction],
    ) -> Result<Vec<PumpswapEvent>, PumpfunError> {
        let t_all = Instant::now();

        // Профилирование (суммы по циклу):
        let mut us_alloc_vec = 0;
        let mut us_adapter_reads = 0;
        let mut us_loop_total = 0;
        let mut us_get_instr = 0;
        let mut us_disc_slice = 0;
        let mut us_match_disc = 0;
        let mut us_decode_buy = 0;
        let mut us_decode_sell = 0;
        let mut us_decode_create = 0;
        let mut us_decode_add = 0;
        let mut us_decode_remove = 0;
        let mut us_idx_format = 0;
        let mut us_push = 0;

        let mut cnt_buy = 0u32;
        let mut cnt_sell = 0u32;
        let mut cnt_create = 0u32;
        let mut cnt_add = 0u32;
        let mut cnt_remove = 0u32;

        // Аллокация вектора под ожидаемое кол-во событий
        let t = Instant::now();
        let mut events: Vec<PumpswapEvent> = Vec::with_capacity(instructions.len());
        us_alloc_vec += since_us(t);

        // Один раз читаем из адаптера
        let t = Instant::now();
        let slot = self.adapter.slot();
        let timestamp = self.adapter.block_time();
        let signature = self.adapter.signature().to_string();
        let signers = self.adapter.signers().to_vec();
        us_adapter_reads += since_us(t);

        debug!("PumpswapEventParser: parsing {} instructions", instructions.len());

        for classified in instructions {
            let t_loop = Instant::now();

            // get_instruction_data
            let t = Instant::now();
            let data = match get_instruction_data(&classified.data) {
                Ok(d) => d,
                Err(e) => {
                    debug!(
                        "decode error at {}-{}: {}",
                        classified.outer_index,
                        classified.inner_index.unwrap_or(0),
                        e
                    );
                    continue;
                }
            };
            us_get_instr += since_us(t);

            if data.len() < 16 {
                continue;
            }

            // slice дискриминатора
            let t = Instant::now();
            let disc = &data[..16];
            us_disc_slice += since_us(t);

            // идентификация типа (без копий payload)
            let t = Instant::now();
            let et = if disc == pumpswap_events::CREATE_POOL.as_slice() {
                Some(PumpswapEventType::Create)
            } else if disc == pumpswap_events::ADD_LIQUIDITY.as_slice() {
                Some(PumpswapEventType::Add)
            } else if disc == pumpswap_events::REMOVE_LIQUIDITY.as_slice() {
                Some(PumpswapEventType::Remove)
            } else if disc == pumpswap_events::BUY.as_slice() {
                info!("✅ Found PUMPSWAP BUY event at {}-{}", classified.outer_index, classified.inner_index.unwrap_or(0));
                Some(PumpswapEventType::Buy)
            } else if disc == pumpswap_events::SELL.as_slice() {
                info!("✅ Found PUMPSWAP SELL event at {}-{}", classified.outer_index, classified.inner_index.unwrap_or(0));
                Some(PumpswapEventType::Sell)
            } else {
                if tracing::enabled!(tracing::Level::DEBUG) {
                    let disc_hex: String = disc.iter().map(|b| format!("{:02x}", b)).collect();
                    let buy_hex: String = pumpswap_events::BUY.iter().map(|b| format!("{:02x}", b)).collect();
                    let sell_hex: String = pumpswap_events::SELL.iter().map(|b| format!("{:02x}", b)).collect();
                    debug!("No match. Expected BUY: {}, SELL: {}, got: {}", buy_hex, sell_hex, disc_hex);
                }
                None
            };
            us_match_disc += since_us(t);

            if let Some(event_type) = et {
                // передаём срез без предварительного to_vec
                let payload = &data[16..];

                // decode по типу с таймингом
                let t = Instant::now();
                let data_enum = match event_type {
                    PumpswapEventType::Buy => {
                        let ev = self.decode_buy_event(payload)?;
                        us_decode_buy += since_us(t);
                        cnt_buy += 1;
                        PumpswapEventData::Buy(ev)
                    }
                    PumpswapEventType::Sell => {
                        let ev = self.decode_sell_event(payload)?;
                        us_decode_sell += since_us(t);
                        cnt_sell += 1;
                        PumpswapEventData::Sell(ev)
                    }
                    PumpswapEventType::Create => {
                        let ev = self.decode_create_event(payload)?;
                        us_decode_create += since_us(t);
                        cnt_create += 1;
                        PumpswapEventData::Create(ev)
                    }
                    PumpswapEventType::Add => {
                        let ev = self.decode_add_liquidity(payload)?;
                        us_decode_add += since_us(t);
                        cnt_add += 1;
                        PumpswapEventData::Deposit(ev)
                    }
                    PumpswapEventType::Remove => {
                        let ev = self.decode_remove_liquidity(payload)?;
                        us_decode_remove += since_us(t);
                        cnt_remove += 1;
                        PumpswapEventData::Withdraw(ev)
                    }
                };

                // форматируем idx
                let t = Instant::now();
                let idx = format!(
                    "{}-{}",
                    classified.outer_index,
                    classified.inner_index.unwrap_or(0)
                );
                us_idx_format += since_us(t);

                // пушим событие
                let t = Instant::now();
                events.push(PumpswapEvent {
                    event_type,
                    data: data_enum,
                    slot,
                    timestamp,
                    signature: signature.clone(),
                    idx,
                    signer: Some(signers.clone()),
                });
                us_push += since_us(t);
            }

            us_loop_total += since_us(t_loop);
        }

        // сортировка
        let t_sort = Instant::now();
        let sorted = sort_by_idx(events);
        let us_sort = since_us(t_sort);

        let us_total = since_us(t_all);
        debug!(
            "⏱️  PumpswapEventParser::parse_instructions TOTAL={}μs ({:.3}ms) | alloc_vec={}μs, adapter_reads={}μs, loop={}μs, get_instr={}μs, disc_slice={}μs, match_disc={}μs, idx_fmt={}μs, push={}μs, sort={}μs | decode: buy={}μs({}), sell={}μs({}), create={}μs({}), add={}μs({}), remove={}μs({})",
            us_total, us_total as f64 / 1000.0,
            us_alloc_vec,
            us_adapter_reads,
            us_loop_total,
            us_get_instr,
            us_disc_slice,
            us_match_disc,
            us_idx_format,
            us_push,
            us_sort,
            us_decode_buy,   cnt_buy,
            us_decode_sell,  cnt_sell,
            us_decode_create,cnt_create,
            us_decode_add,   cnt_add,
            us_decode_remove,cnt_remove
        );

        Ok(sorted)
    }

    #[inline]
    fn decode_buy_event(&self, data: &[u8]) -> Result<PumpswapBuyEvent, PumpfunError> {
        let t_all = Instant::now();
        // BinaryReader требует владение — создаём один to_vec здесь (единственная копия)
        let mut reader = BinaryReader::new(data.to_vec());

        let ts = reader.read_i64()?;
        let ev = PumpswapBuyEvent {
            timestamp: normalize_timestamp(ts),
            base_amount_out: reader.read_u64()?,
            max_quote_amount_in: reader.read_u64()?,
            user_base_token_reserves: reader.read_u64()?,
            user_quote_token_reserves: reader.read_u64()?,
            pool_base_token_reserves: reader.read_u64()?,
            pool_quote_token_reserves: reader.read_u64()?,
            quote_amount_in: reader.read_u64()?,
            lp_fee_basis_points: reader.read_u64()?,
            lp_fee: reader.read_u64()?,
            protocol_fee_basis_points: reader.read_u64()?,
            protocol_fee: reader.read_u64()?,
            quote_amount_in_with_lp_fee: reader.read_u64()?,
            user_quote_amount_in: reader.read_u64()?,
            pool: reader.read_pubkey()?,
            user: reader.read_pubkey()?,
            user_base_token_account: reader.read_pubkey()?,
            user_quote_token_account: reader.read_pubkey()?,
            protocol_fee_recipient: reader.read_pubkey()?,
            protocol_fee_recipient_token_account: reader.read_pubkey()?,
            // опциональные поля
            coin_creator: {
                if reader.remaining() >= 32 + 8 + 8 {
                    reader.read_pubkey()?
                } else {
                    "11111111111111111111111111111111".to_string()
                }
            },
            coin_creator_fee_basis_points: if reader.remaining() >= 8 + 8 {
                reader.read_u64()?
            } else { 0 },
            coin_creator_fee: if reader.remaining() >= 8 {
                reader.read_u64()?
            } else { 0 },
        };

        let us_total = since_us(t_all);
        debug!("   ↳ decode_buy_event: {}μs ({:.3}ms)", us_total, us_total as f64 / 1000.0);
        Ok(ev)
    }

    #[inline]
    fn decode_sell_event(&self, data: &[u8]) -> Result<PumpswapSellEvent, PumpfunError> {
        let t_all = Instant::now();
        let mut reader = BinaryReader::new(data.to_vec());

        let ts = reader.read_i64()?;
        let ev = PumpswapSellEvent {
            timestamp: normalize_timestamp(ts),
            base_amount_in: reader.read_u64()?,
            min_quote_amount_out: reader.read_u64()?,
            user_base_token_reserves: reader.read_u64()?,
            user_quote_token_reserves: reader.read_u64()?,
            pool_base_token_reserves: reader.read_u64()?,
            pool_quote_token_reserves: reader.read_u64()?,
            quote_amount_out: reader.read_u64()?,
            lp_fee_basis_points: reader.read_u64()?,
            lp_fee: reader.read_u64()?,
            protocol_fee_basis_points: reader.read_u64()?,
            protocol_fee: reader.read_u64()?,
            quote_amount_out_without_lp_fee: reader.read_u64()?,
            user_quote_amount_out: reader.read_u64()?,
            pool: reader.read_pubkey()?,
            user: reader.read_pubkey()?,
            user_base_token_account: reader.read_pubkey()?,
            user_quote_token_account: reader.read_pubkey()?,
            protocol_fee_recipient: reader.read_pubkey()?,
            protocol_fee_recipient_token_account: reader.read_pubkey()?,
            coin_creator: {
                if reader.remaining() >= 32 + 8 + 8 {
                    reader.read_pubkey()?
                } else {
                    "11111111111111111111111111111111".to_string()
                }
            },
            coin_creator_fee_basis_points: if reader.remaining() >= 8 + 8 {
                reader.read_u64()?
            } else { 0 },
            coin_creator_fee: if reader.remaining() >= 8 {
                reader.read_u64()?
            } else { 0 },
        };

        let us_total = since_us(t_all);
        debug!("   ↳ decode_sell_event: {}μs ({:.3}ms)", us_total, us_total as f64 / 1000.0);
        Ok(ev)
    }

    #[inline]
    fn decode_add_liquidity(&self, data: &[u8]) -> Result<PumpswapDepositEvent, PumpfunError> {
        let t_all = Instant::now();
        let mut reader = BinaryReader::new(data.to_vec());

        let ts = reader.read_i64()?;
        let ev = PumpswapDepositEvent {
            timestamp: normalize_timestamp(ts),
            lp_token_amount_out: reader.read_u64()?,
            max_base_amount_in: reader.read_u64()?,
            max_quote_amount_in: reader.read_u64()?,
            user_base_token_reserves: reader.read_u64()?,
            user_quote_token_reserves: reader.read_u64()?,
            pool_base_token_reserves: reader.read_u64()?,
            pool_quote_token_reserves: reader.read_u64()?,
            base_amount_in: reader.read_u64()?,
            quote_amount_in: reader.read_u64()?,
            lp_mint_supply: reader.read_u64()?,
            pool: reader.read_pubkey()?,
            user: reader.read_pubkey()?,
            user_base_token_account: reader.read_pubkey()?,
            user_quote_token_account: reader.read_pubkey()?,
            user_pool_token_account: reader.read_pubkey()?,
        };

        let us_total = since_us(t_all);
        debug!("   ↳ decode_add_liquidity: {}μs ({:.3}ms)", us_total, us_total as f64 / 1000.0);
        Ok(ev)
    }

    #[inline]
    fn decode_create_event(&self, data: &[u8]) -> Result<PumpswapCreatePoolEvent, PumpfunError> {
        let t_all = Instant::now();
        let mut reader = BinaryReader::new(data.to_vec());

        let ts = reader.read_i64()?;
        let ev = PumpswapCreatePoolEvent {
            timestamp: normalize_timestamp(ts),
            index: reader.read_u16()?,
            creator: reader.read_pubkey()?,
            base_mint: reader.read_pubkey()?,
            quote_mint: reader.read_pubkey()?,
            base_mint_decimals: reader.read_u8()?,
            quote_mint_decimals: reader.read_u8()?,
            base_amount_in: reader.read_u64()?,
            quote_amount_in: reader.read_u64()?,
            pool_base_amount: reader.read_u64()?,
            pool_quote_amount: reader.read_u64()?,
            minimum_liquidity: reader.read_u64()?,
            initial_liquidity: reader.read_u64()?,
            lp_token_amount_out: reader.read_u64()?,
            pool_bump: reader.read_u8()?,
            pool: reader.read_pubkey()?,
            lp_mint: reader.read_pubkey()?,
            user_base_token_account: reader.read_pubkey()?,
            user_quote_token_account: reader.read_pubkey()?,
        };

        let us_total = since_us(t_all);
        debug!("   ↳ decode_create_event: {}μs ({:.3}ms)", us_total, us_total as f64 / 1000.0);
        Ok(ev)
    }

    #[inline]
    fn decode_remove_liquidity(&self, data: &[u8]) -> Result<PumpswapWithdrawEvent, PumpfunError> {
        let t_all = Instant::now();
        let mut reader = BinaryReader::new(data.to_vec());

        let ts = reader.read_i64()?;
        let ev = PumpswapWithdrawEvent {
            timestamp: normalize_timestamp(ts),
            lp_token_amount_in: reader.read_u64()?,
            min_base_amount_out: reader.read_u64()?,
            min_quote_amount_out: reader.read_u64()?,
            user_base_token_reserves: reader.read_u64()?,
            user_quote_token_reserves: reader.read_u64()?,
            pool_base_token_reserves: reader.read_u64()?,
            pool_quote_token_reserves: reader.read_u64()?,
            base_amount_out: reader.read_u64()?,
            quote_amount_out: reader.read_u64()?,
            lp_mint_supply: reader.read_u64()?,
            pool: reader.read_pubkey()?,
            user: reader.read_pubkey()?,
            user_base_token_account: reader.read_pubkey()?,
            user_quote_token_account: reader.read_pubkey()?,
            user_pool_token_account: reader.read_pubkey()?,
        };

        let us_total = since_us(t_all);
        debug!("   ↳ decode_remove_liquidity: {}μs ({:.3}ms)", us_total, us_total as f64 / 1000.0);
        Ok(ev)
    }
}

impl HasIdx for PumpswapEvent {
    #[inline]
    fn idx(&self) -> &str { &self.idx }
}

#[inline]
fn normalize_timestamp(value: i64) -> u64 {
    if value >= 0 { value as u64 } else { 0 }
}
