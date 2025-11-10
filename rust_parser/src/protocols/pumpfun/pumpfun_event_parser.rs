use bs58::encode as bs58_encode;

use crate::types::{ClassifiedInstruction, MemeEvent, TradeType};

use super::binary_reader::BinaryReader;
use super::constants::{
    discriminators::pumpfun_events, PUMP_FUN_PROGRAM_NAME, PUMP_SWAP_PROGRAM_NAME, SOL_MINT,
};
use super::error::PumpfunError;
use super::util::{
    build_token_info, get_instruction_data, get_prev_instruction_by_index, get_trade_type,
    sort_by_idx, HasIdx,
};

use crate::core::transaction_adapter::TransactionAdapter;

use std::time::Instant;
use tracing::debug;

#[inline]
fn since_us(start: Instant) -> u128 {
    start.elapsed().as_micros()
}

pub struct PumpfunEventParser {
    adapter: TransactionAdapter,
}

impl PumpfunEventParser {
    pub fn new(adapter: TransactionAdapter) -> Self {
        Self { adapter }
    }

    pub fn parse_instructions(
        &self,
        instructions: &[ClassifiedInstruction],
    ) -> Result<Vec<MemeEvent>, PumpfunError> {
        let t_all = Instant::now();
        let mut t_vec_alloc_us: u128 = 0;
        let mut t_adapter_calls_us: u128 = 0;
        let mut t_loop_us: u128 = 0;
        let mut t_get_instr_data_us: u128 = 0;
        let mut t_discriminator_check_us: u128 = 0;
        let mut t_payload_copy_us: u128 = 0;
        let mut t_decode_trade_us: u128 = 0;
        let mut t_decode_create_us: u128 = 0;
        let mut t_decode_complete_us: u128 = 0;
        let mut t_decode_migrate_us: u128 = 0;
        let mut t_prev_lookup_us: u128 = 0;
        let mut t_event_fill_us: u128 = 0;
        let mut t_format_idx_us: u128 = 0;
        let mut t_push_us: u128 = 0;

        let t = Instant::now();
        let mut events = Vec::with_capacity(instructions.len());
        t_vec_alloc_us = since_us(t);

        let t = Instant::now();
        let signature = self.adapter.signature().to_string();
        let slot = self.adapter.slot();
        let timestamp = self.adapter.block_time();
        t_adapter_calls_us = since_us(t);

        for classified in instructions {
            let t_loop = Instant::now();

            let t = Instant::now();
            let data = get_instruction_data(&classified.data)?;
            t_get_instr_data_us += since_us(t);

            if data.len() < 16 {
                continue;
            }

            let t = Instant::now();
            let discriminator = &data[..16];
            t_discriminator_check_us += since_us(t);

            let event = if discriminator == pumpfun_events::TRADE {
                let t = Instant::now();
                let payload = data[16..].to_vec();
                t_payload_copy_us += since_us(t);

                let t = Instant::now();
                let ev = self.decode_trade_event(payload)?;
                t_decode_trade_us += since_us(t);
                Some(ev)
            } else if discriminator == pumpfun_events::CREATE {
                let t = Instant::now();
                let payload = data[16..].to_vec();
                t_payload_copy_us += since_us(t);

                let t = Instant::now();
                let ev = self.decode_create_event(payload)?;
                t_decode_create_us += since_us(t);
                Some(ev)
            } else if discriminator == pumpfun_events::COMPLETE {
                let t = Instant::now();
                let payload = data[16..].to_vec();
                t_payload_copy_us += since_us(t);

                let t = Instant::now();
                let ev = self.decode_complete_event(payload)?;
                t_decode_complete_us += since_us(t);
                Some(ev)
            } else if discriminator == pumpfun_events::MIGRATE {
                let t = Instant::now();
                let payload = data[16..].to_vec();
                t_payload_copy_us += since_us(t);

                let t = Instant::now();
                let ev = self.decode_migrate_event(payload)?;
                t_decode_migrate_us += since_us(t);
                Some(ev)
            } else {
                None
            };

            if let Some(mut meme_event) = event {
                if matches!(meme_event.event_type, TradeType::Buy | TradeType::Sell) {
                    let t_prev = Instant::now();
                    if let Some(prev) = get_prev_instruction_by_index(
                        instructions,
                        classified.outer_index,
                        classified.inner_index,
                    ) {
                        if prev.data.accounts.len() > 3 {
                            if let Some(account) = prev.data.accounts.get(3) {
                                meme_event.bonding_curve = Some(account.clone());
                            }
                        }
                    }
                    t_prev_lookup_us += since_us(t_prev);
                }

                let t = Instant::now();
                meme_event.signature = signature.clone();
                meme_event.slot = slot;
                meme_event.timestamp = timestamp;
                t_event_fill_us += since_us(t);

                let t = Instant::now();
                meme_event.idx = format!(
                    "{}-{}",
                    classified.outer_index,
                    classified.inner_index.unwrap_or(0)
                );
                t_format_idx_us += since_us(t);

                let t = Instant::now();
                events.push(meme_event);
                t_push_us += since_us(t);
            }

            t_loop_us += since_us(t_loop);
        }

        let total_us = since_us(t_all);

        debug!(
            "⏱️  parse_instructions TOTAL: {}μs ({:.3}ms) | vec_alloc={}μs, adapter_calls={}μs, loop={}μs, get_instr_data={}μs, discriminator={}μs, payload_copy={}μs, prev_lookup={}μs, event_fill={}μs, format_idx={}μs, push={}μs | decode: trade={}μs, create={}μs, complete={}μs, migrate={}μs",
            total_us,
            total_us as f64 / 1000.0,
            t_vec_alloc_us,
            t_adapter_calls_us,
            t_loop_us,
            t_get_instr_data_us,
            t_discriminator_check_us,
            t_payload_copy_us,
            t_prev_lookup_us,
            t_event_fill_us,
            t_format_idx_us,
            t_push_us,
            t_decode_trade_us,
            t_decode_create_us,
            t_decode_complete_us,
            t_decode_migrate_us
        );

        let t_sort = Instant::now();
        let sorted = sort_by_idx(events);
        let sort_us = since_us(t_sort);
        debug!("⏱️  sort_by_idx: {}μs ({:.3}ms)", sort_us, sort_us as f64 / 1000.0);

        Ok(sorted)
    }

    fn decode_trade_event(&self, data: Vec<u8>) -> Result<MemeEvent, PumpfunError> {
        let t_all = Instant::now();
        let mut t_reader_new_us: u128 = 0;
        let mut t_read_pubkey_us: u128 = 0;
        let mut t_read_u64_us: u128 = 0;
        let mut t_read_u8_us: u128 = 0;
        let mut t_read_fixed_array_us: u128 = 0;
        let mut t_bs58_encode_us: u128 = 0;
        let mut t_read_i64_us: u128 = 0;
        let mut t_remaining_check_us: u128 = 0;
        let mut t_optional_reads_us: u128 = 0;
        let mut t_tuple_construct_us: u128 = 0;
        let mut t_build_token_info_us: u128 = 0;
        let mut t_get_trade_type_us: u128 = 0;
        let mut t_event_construct_us: u128 = 0;

        let t = Instant::now();
        let mut reader = BinaryReader::new(data);
        t_reader_new_us = since_us(t);

        let t = Instant::now();
        let mint = reader.read_pubkey()?;
        t_read_pubkey_us += since_us(t);

        let quote_mint = SOL_MINT.to_string();

        let t = Instant::now();
        let sol_amount = reader.read_u64()? as u128;
        t_read_u64_us += since_us(t);

        let t = Instant::now();
        let token_amount = reader.read_u64()? as u128;
        t_read_u64_us += since_us(t);

        let t = Instant::now();
        let is_buy = reader.read_u8()? == 1;
        t_read_u8_us = since_us(t);

        let t = Instant::now();
        let user_bytes = reader.read_fixed_array(32)?;
        t_read_fixed_array_us += since_us(t);

        let t = Instant::now();
        let user = bs58_encode(user_bytes).into_string();
        t_bs58_encode_us += since_us(t);

        let t = Instant::now();
        let _event_timestamp = reader.read_i64()?;
        t_read_i64_us += since_us(t);

        let t = Instant::now();
        let _virtual_sol = reader.read_u64()?;
        t_read_u64_us += since_us(t);

        let t = Instant::now();
        let _virtual_token = reader.read_u64()?;
        t_read_u64_us += since_us(t);

        let mut fee = None;
        let mut creator = None;
        let mut creator_fee = None;

        let t = Instant::now();
        let has_optional = reader.remaining() >= 52;
        t_remaining_check_us = since_us(t);

        if has_optional {
            let t = Instant::now();
            let _real_sol_reserves = reader.read_u64()?;
            let _real_token_reserves = reader.read_u64()?;
            let _fee_recipient = reader.read_pubkey()?;
            let _fee_basis_points = reader.read_u16()?;
            let raw_fee = reader.read_u64()?;
            let creator_key = reader.read_pubkey()?;
            let _creator_fee_basis_points = reader.read_u16()?;
            let raw_creator_fee = reader.read_u64()?;
            t_optional_reads_us = since_us(t);

            fee = Some(raw_fee as f64);
            creator = Some(creator_key);
            creator_fee = Some(raw_creator_fee as f64);
        }

        let t = Instant::now();
        let (input_mint, input_amount, input_decimals, output_mint, output_amount, output_decimals) =
            if is_buy {
                (&quote_mint, sol_amount, 9, &mint, token_amount, 6)
            } else {
                (&mint, token_amount, 6, &quote_mint, sol_amount, 9)
            };
        t_tuple_construct_us = since_us(t);

        let t = Instant::now();
        let input_token = build_token_info(input_mint, input_amount, input_decimals, None);
        t_build_token_info_us += since_us(t);

        let t = Instant::now();
        let output_token = build_token_info(output_mint, output_amount, output_decimals, None);
        t_build_token_info_us += since_us(t);

        let t = Instant::now();
        let trade_type = get_trade_type(input_mint, output_mint);
        t_get_trade_type_us = since_us(t);

        let t = Instant::now();
        let event = MemeEvent {
            event_type: trade_type,
            timestamp: 0,
            idx: String::new(),
            slot: 0,
            signature: String::new(),
            user,
            base_mint: mint,
            quote_mint,
            input_token: Some(input_token),
            output_token: Some(output_token),
            name: None,
            symbol: None,
            uri: None,
            decimals: None,
            total_supply: None,
            fee,
            protocol_fee: None,
            platform_fee: None,
            share_fee: None,
            creator_fee,
            protocol: Some(PUMP_FUN_PROGRAM_NAME.to_string()),
            platform_config: None,
            creator,
            bonding_curve: None,
            pool: None,
            pool_dex: None,
            pool_a_reserve: None,
            pool_b_reserve: None,
            pool_fee_rate: None,
        };
        t_event_construct_us = since_us(t);

        let total_us = since_us(t_all);
        debug!(
            "⏱️  decode_trade TOTAL: {}μs ({:.3}ms) | reader_new={}μs, read_pubkey={}μs, read_u64={}μs, read_u8={}μs, read_fixed_array={}μs, bs58={}μs, read_i64={}μs, remaining_check={}μs, optional_reads={}μs, tuple={}μs, build_token={}μs, get_trade_type={}μs, event_construct={}μs",
            total_us,
            total_us as f64 / 1000.0,
            t_reader_new_us,
            t_read_pubkey_us,
            t_read_u64_us,
            t_read_u8_us,
            t_read_fixed_array_us,
            t_bs58_encode_us,
            t_read_i64_us,
            t_remaining_check_us,
            t_optional_reads_us,
            t_tuple_construct_us,
            t_build_token_info_us,
            t_get_trade_type_us,
            t_event_construct_us
        );

        Ok(event)
    }

    fn decode_create_event(&self, data: Vec<u8>) -> Result<MemeEvent, PumpfunError> {
        let t_all = Instant::now();
        let mut t_reader_new_us: u128 = 0;
        let mut t_read_string_us: u128 = 0;
        let mut t_read_fixed_array_us: u128 = 0;
        let mut t_bs58_encode_us: u128 = 0;
        let mut t_remaining_check_us: u128 = 0;
        let mut t_read_pubkey_us: u128 = 0;
        let mut t_read_i64_us: u128 = 0;
        let mut t_read_optional_reserves_us: u128 = 0;
        let mut t_event_construct_us: u128 = 0;

        let t = Instant::now();
        let mut reader = BinaryReader::new(data);
        t_reader_new_us = since_us(t);

        let t = Instant::now();
        let name = reader.read_string()?;
        t_read_string_us += since_us(t);

        let t = Instant::now();
        let symbol = reader.read_string()?;
        t_read_string_us += since_us(t);

        let t = Instant::now();
        let uri = reader.read_string()?;
        t_read_string_us += since_us(t);

        let t = Instant::now();
        let mint_bytes = reader.read_fixed_array(32)?;
        t_read_fixed_array_us += since_us(t);

        let t = Instant::now();
        let mint = bs58_encode(mint_bytes).into_string();
        t_bs58_encode_us += since_us(t);

        let t = Instant::now();
        let bonding_curve_bytes = reader.read_fixed_array(32)?;
        t_read_fixed_array_us += since_us(t);

        let t = Instant::now();
        let bonding_curve = bs58_encode(bonding_curve_bytes).into_string();
        t_bs58_encode_us += since_us(t);

        let t = Instant::now();
        let user_bytes = reader.read_fixed_array(32)?;
        t_read_fixed_array_us += since_us(t);

        let t = Instant::now();
        let user = bs58_encode(user_bytes).into_string();
        t_bs58_encode_us += since_us(t);

        let mut creator = None;
        let mut timestamp = 0;

        let t = Instant::now();
        let has_creator = reader.remaining() >= 16;
        t_remaining_check_us += since_us(t);

        if has_creator {
            let t = Instant::now();
            creator = Some(reader.read_pubkey()?);
            t_read_pubkey_us = since_us(t);

            let t = Instant::now();
            let ts = reader.read_i64()?;
            t_read_i64_us = since_us(t);

            if ts >= 0 {
                timestamp = ts as u64;
            }
        }

        let t = Instant::now();
        let has_reserves = reader.remaining() >= 32;
        t_remaining_check_us += since_us(t);

        if has_reserves {
            let t = Instant::now();
            let _virtual_token_reserves = reader.read_u64()?;
            let _virtual_sol_reserves = reader.read_u64()?;
            let _real_token_reserves = reader.read_u64()?;
            let _token_total_supply = reader.read_u64()?;
            t_read_optional_reserves_us = since_us(t);
        }

        let t = Instant::now();
        let event = MemeEvent {
            event_type: TradeType::Create,
            timestamp,
            idx: String::new(),
            slot: 0,
            signature: String::new(),
            user,
            base_mint: mint,
            quote_mint: SOL_MINT.to_string(),
            input_token: None,
            output_token: None,
            name: Some(name),
            symbol: Some(symbol),
            uri: Some(uri),
            decimals: None,
            total_supply: None,
            fee: None,
            protocol_fee: None,
            platform_fee: None,
            share_fee: None,
            creator_fee: None,
            protocol: Some(PUMP_FUN_PROGRAM_NAME.to_string()),
            platform_config: None,
            creator,
            bonding_curve: Some(bonding_curve),
            pool: None,
            pool_dex: None,
            pool_a_reserve: None,
            pool_b_reserve: None,
            pool_fee_rate: None,
        };
        t_event_construct_us = since_us(t);

        let total_us = since_us(t_all);
        debug!(
            "⏱️  decode_create TOTAL: {}μs ({:.3}ms) | reader_new={}μs, read_string={}μs, read_fixed_array={}μs, bs58={}μs, remaining_check={}μs, read_pubkey={}μs, read_i64={}μs, read_reserves={}μs, event_construct={}μs",
            total_us,
            total_us as f64 / 1000.0,
            t_reader_new_us,
            t_read_string_us,
            t_read_fixed_array_us,
            t_bs58_encode_us,
            t_remaining_check_us,
            t_read_pubkey_us,
            t_read_i64_us,
            t_read_optional_reserves_us,
            t_event_construct_us
        );

        Ok(event)
    }

    fn decode_complete_event(&self, data: Vec<u8>) -> Result<MemeEvent, PumpfunError> {
        let t_all = Instant::now();
        let mut t_reader_new_us: u128 = 0;
        let mut t_read_fixed_array_us: u128 = 0;
        let mut t_bs58_encode_us: u128 = 0;
        let mut t_read_i64_us: u128 = 0;
        let mut t_event_construct_us: u128 = 0;

        let t = Instant::now();
        let mut reader = BinaryReader::new(data);
        t_reader_new_us = since_us(t);

        let t = Instant::now();
        let user_bytes = reader.read_fixed_array(32)?;
        t_read_fixed_array_us += since_us(t);

        let t = Instant::now();
        let user = bs58_encode(user_bytes).into_string();
        t_bs58_encode_us += since_us(t);

        let t = Instant::now();
        let mint_bytes = reader.read_fixed_array(32)?;
        t_read_fixed_array_us += since_us(t);

        let t = Instant::now();
        let mint = bs58_encode(mint_bytes).into_string();
        t_bs58_encode_us += since_us(t);

        let t = Instant::now();
        let bonding_curve_bytes = reader.read_fixed_array(32)?;
        t_read_fixed_array_us += since_us(t);

        let t = Instant::now();
        let bonding_curve = bs58_encode(bonding_curve_bytes).into_string();
        t_bs58_encode_us += since_us(t);

        let t = Instant::now();
        let ts = reader.read_i64()?;
        t_read_i64_us = since_us(t);

        let timestamp = if ts >= 0 { ts as u64 } else { 0 };

        let t = Instant::now();
        let event = MemeEvent {
            event_type: TradeType::Complete,
            timestamp,
            idx: String::new(),
            slot: 0,
            signature: String::new(),
            user,
            base_mint: mint,
            quote_mint: SOL_MINT.to_string(),
            input_token: None,
            output_token: None,
            name: None,
            symbol: None,
            uri: None,
            decimals: None,
            total_supply: None,
            fee: None,
            protocol_fee: None,
            platform_fee: None,
            share_fee: None,
            creator_fee: None,
            protocol: Some(PUMP_FUN_PROGRAM_NAME.to_string()),
            platform_config: None,
            creator: None,
            bonding_curve: Some(bonding_curve),
            pool: None,
            pool_dex: None,
            pool_a_reserve: None,
            pool_b_reserve: None,
            pool_fee_rate: None,
        };
        t_event_construct_us = since_us(t);

        let total_us = since_us(t_all);
        debug!(
            "⏱️  decode_complete TOTAL: {}μs ({:.3}ms) | reader_new={}μs, read_fixed_array={}μs, bs58={}μs, read_i64={}μs, event_construct={}μs",
            total_us,
            total_us as f64 / 1000.0,
            t_reader_new_us,
            t_read_fixed_array_us,
            t_bs58_encode_us,
            t_read_i64_us,
            t_event_construct_us
        );

        Ok(event)
    }

    fn decode_migrate_event(&self, data: Vec<u8>) -> Result<MemeEvent, PumpfunError> {
        let t_all = Instant::now();
        let mut t_reader_new_us: u128 = 0;
        let mut t_read_fixed_array_us: u128 = 0;
        let mut t_bs58_encode_us: u128 = 0;
        let mut t_read_u64_us: u128 = 0;
        let mut t_read_i64_us: u128 = 0;
        let mut t_read_pubkey_us: u128 = 0;
        let mut t_event_construct_us: u128 = 0;

        let t = Instant::now();
        let mut reader = BinaryReader::new(data);
        t_reader_new_us = since_us(t);

        let t = Instant::now();
        let user_bytes = reader.read_fixed_array(32)?;
        t_read_fixed_array_us += since_us(t);

        let t = Instant::now();
        let user = bs58_encode(user_bytes).into_string();
        t_bs58_encode_us += since_us(t);

        let t = Instant::now();
        let mint_bytes = reader.read_fixed_array(32)?;
        t_read_fixed_array_us += since_us(t);

        let t = Instant::now();
        let mint = bs58_encode(mint_bytes).into_string();
        t_bs58_encode_us += since_us(t);

        let t = Instant::now();
        let _mint_amount = reader.read_u64()?;
        t_read_u64_us += since_us(t);

        let t = Instant::now();
        let _sol_amount = reader.read_u64()?;
        t_read_u64_us += since_us(t);

        let t = Instant::now();
        let _pool_migrate_fee = reader.read_u64()? as u128;
        t_read_u64_us += since_us(t);

        let t = Instant::now();
        let bonding_curve_bytes = reader.read_fixed_array(32)?;
        t_read_fixed_array_us += since_us(t);

        let t = Instant::now();
        let bonding_curve = bs58_encode(bonding_curve_bytes).into_string();
        t_bs58_encode_us += since_us(t);

        let t = Instant::now();
        let ts = reader.read_i64()?;
        t_read_i64_us = since_us(t);

        let timestamp = if ts >= 0 { ts as u64 } else { 0 };

        let t = Instant::now();
        let pool = reader.read_pubkey()?;
        t_read_pubkey_us = since_us(t);

        let t = Instant::now();
        let event = MemeEvent {
            event_type: TradeType::Migrate,
            timestamp,
            idx: String::new(),
            slot: 0,
            signature: String::new(),
            user,
            base_mint: mint,
            quote_mint: SOL_MINT.to_string(),
            input_token: None,
            output_token: None,
            name: None,
            symbol: None,
            uri: None,
            decimals: None,
            total_supply: None,
            fee: None,
            protocol_fee: None,
            platform_fee: None,
            share_fee: None,
            creator_fee: None,
            protocol: Some(PUMP_FUN_PROGRAM_NAME.to_string()),
            platform_config: None,
            creator: None,
            bonding_curve: Some(bonding_curve),
            pool: Some(pool),
            pool_dex: Some(PUMP_SWAP_PROGRAM_NAME.to_string()),
            pool_a_reserve: None,
            pool_b_reserve: None,
            pool_fee_rate: None,
        };
        t_event_construct_us = since_us(t);

        let total_us = since_us(t_all);
        debug!(
            "⏱️  decode_migrate TOTAL: {}μs ({:.3}ms) | reader_new={}μs, read_fixed_array={}μs, bs58={}μs, read_u64={}μs, read_i64={}μs, read_pubkey={}μs, event_construct={}μs",
            total_us,
            total_us as f64 / 1000.0,
            t_reader_new_us,
            t_read_fixed_array_us,
            t_bs58_encode_us,
            t_read_u64_us,
            t_read_i64_us,
            t_read_pubkey_us,
            t_event_construct_us
        );

        Ok(event)
    }
}

impl HasIdx for MemeEvent {
    #[inline]
    fn idx(&self) -> &str {
        &self.idx
    }
}