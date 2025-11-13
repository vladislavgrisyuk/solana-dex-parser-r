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

pub struct PumpfunEventParser;

impl PumpfunEventParser {
    /// Оптимизация: создаем пустую структуру, адаптер передаем по ссылке
    pub fn new() -> Self {
        Self
    }

    /// Оптимизация: принимаем адаптер по ссылке вместо хранения
    pub fn parse_instructions(
        &self,
        adapter: &TransactionAdapter,
        instructions: &[ClassifiedInstruction],
    ) -> Result<Vec<MemeEvent>, PumpfunError> {
        let mut events = Vec::with_capacity(instructions.len());
        let signature = adapter.signature().to_string();
        let slot = adapter.slot();
        let timestamp = adapter.block_time();

        for classified in instructions {
            let data = get_instruction_data(&classified.data)?;

            if data.len() < 16 {
                continue;
            }

            let discriminator = &data[..16];
            // ОПТИМИЗАЦИЯ: передаем срез вместо to_vec(), копирование будет только внутри decode методов для BinaryReader
            let payload = &data[16..];

            let event = if discriminator == pumpfun_events::TRADE {
                self.decode_trade_event(payload).ok()
            } else if discriminator == pumpfun_events::CREATE {
                self.decode_create_event(payload).ok()
            } else if discriminator == pumpfun_events::COMPLETE {
                self.decode_complete_event(payload).ok()
            } else if discriminator == pumpfun_events::MIGRATE {
                self.decode_migrate_event(payload).ok()
            } else {
                None
            };

            if let Some(mut meme_event) = event {
                if matches!(meme_event.event_type, TradeType::Buy | TradeType::Sell) {
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
                }

                meme_event.signature = signature.clone();
                meme_event.slot = slot;
                meme_event.timestamp = timestamp;
                meme_event.idx = format!(
                    "{}-{}",
                    classified.outer_index,
                    classified.inner_index.unwrap_or(0)
                );

                events.push(meme_event);
            }
        }

        Ok(sort_by_idx(events))
    }

    fn decode_trade_event(&self, data: &[u8]) -> Result<MemeEvent, PumpfunError> {
        // ОПТИМИЗАЦИЯ: делаем to_vec() только один раз для BinaryReader
        let mut reader = BinaryReader::new(data.to_vec());

        let mint = reader.read_pubkey()?;
        let quote_mint = SOL_MINT.to_string();
        let sol_amount = reader.read_u64()? as u128;
        let token_amount = reader.read_u64()? as u128;
        let is_buy = reader.read_u8()? == 1;
        let user_bytes = reader.read_fixed_array(32)?;
        let user = bs58_encode(user_bytes).into_string();
        let _event_timestamp = reader.read_i64()?;
        let _virtual_sol = reader.read_u64()?;
        let _virtual_token = reader.read_u64()?;

        let mut fee = None;
        let mut creator = None;
        let mut creator_fee = None;

        if reader.remaining() >= 52 {
            let _real_sol_reserves = reader.read_u64()?;
            let _real_token_reserves = reader.read_u64()?;
            let _fee_recipient = reader.read_pubkey()?;
            let _fee_basis_points = reader.read_u16()?;
            let raw_fee = reader.read_u64()?;
            let creator_key = reader.read_pubkey()?;
            let _creator_fee_basis_points = reader.read_u16()?;
            let raw_creator_fee = reader.read_u64()?;

            fee = Some(raw_fee as f64);
            creator = Some(creator_key);
            creator_fee = Some(raw_creator_fee as f64);
        }

        let (input_mint, input_amount, input_decimals, output_mint, output_amount, output_decimals) =
            if is_buy {
                (&quote_mint, sol_amount, 9, &mint, token_amount, 6)
            } else {
                (&mint, token_amount, 6, &quote_mint, sol_amount, 9)
            };

        let input_token = build_token_info(input_mint, input_amount, input_decimals, None);
        let output_token = build_token_info(output_mint, output_amount, output_decimals, None);
        let trade_type = get_trade_type(input_mint, output_mint);

        Ok(MemeEvent {
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
        })
    }

    fn decode_create_event(&self, data: &[u8]) -> Result<MemeEvent, PumpfunError> {
        let mut reader = BinaryReader::new(data.to_vec());

        let name = reader.read_string()?;
        let symbol = reader.read_string()?;
        let uri = reader.read_string()?;
        let mint_bytes = reader.read_fixed_array(32)?;
        let mint = bs58_encode(mint_bytes).into_string();
        let bonding_curve_bytes = reader.read_fixed_array(32)?;
        let bonding_curve = bs58_encode(bonding_curve_bytes).into_string();
        let user_bytes = reader.read_fixed_array(32)?;
        let user = bs58_encode(user_bytes).into_string();

        let mut creator = None;
        let mut timestamp = 0;

        if reader.remaining() >= 16 {
            creator = Some(reader.read_pubkey()?);
            let ts = reader.read_i64()?;
            if ts >= 0 {
                timestamp = ts as u64;
            }
        }

        if reader.remaining() >= 32 {
            let _virtual_token_reserves = reader.read_u64()?;
            let _virtual_sol_reserves = reader.read_u64()?;
            let _real_token_reserves = reader.read_u64()?;
            let _token_total_supply = reader.read_u64()?;
        }

        Ok(MemeEvent {
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
        })
    }

    fn decode_complete_event(&self, data: &[u8]) -> Result<MemeEvent, PumpfunError> {
        let mut reader = BinaryReader::new(data.to_vec());

        let user_bytes = reader.read_fixed_array(32)?;
        let user = bs58_encode(user_bytes).into_string();
        let mint_bytes = reader.read_fixed_array(32)?;
        let mint = bs58_encode(mint_bytes).into_string();
        let bonding_curve_bytes = reader.read_fixed_array(32)?;
        let bonding_curve = bs58_encode(bonding_curve_bytes).into_string();
        let ts = reader.read_i64()?;
        let timestamp = if ts >= 0 { ts as u64 } else { 0 };

        Ok(MemeEvent {
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
        })
    }

    fn decode_migrate_event(&self, data: &[u8]) -> Result<MemeEvent, PumpfunError> {
        let mut reader = BinaryReader::new(data.to_vec());

        let user_bytes = reader.read_fixed_array(32)?;
        let user = bs58_encode(user_bytes).into_string();
        let mint_bytes = reader.read_fixed_array(32)?;
        let mint = bs58_encode(mint_bytes).into_string();
        let _mint_amount = reader.read_u64()?;
        let _sol_amount = reader.read_u64()?;
        let _pool_migrate_fee = reader.read_u64()? as u128;
        let bonding_curve_bytes = reader.read_fixed_array(32)?;
        let bonding_curve = bs58_encode(bonding_curve_bytes).into_string();
        let ts = reader.read_i64()?;
        let timestamp = if ts >= 0 { ts as u64 } else { 0 };
        let pool = reader.read_pubkey()?;

        Ok(MemeEvent {
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
        })
    }
}

impl HasIdx for MemeEvent {
    #[inline]
    fn idx(&self) -> &str {
        &self.idx
    }
}