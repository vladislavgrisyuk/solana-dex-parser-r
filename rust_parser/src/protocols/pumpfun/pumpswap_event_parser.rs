use std::sync::Arc;

use crate::core::transaction_adapter::TransactionAdapter;
use crate::core::zc_adapter::ZcAdapter;
use crate::core::zc_instruction_classifier::ZcClassifiedInstruction;
use crate::types::ClassifiedInstruction;
use bs58;

use super::binary_reader::BinaryReaderRef;
use super::constants::discriminators::pumpswap_events;
use super::error::PumpfunError;
use super::util::{get_instruction_data, sort_by_idx, HasIdx};

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
    pub signature: Arc<String>,
    pub idx: String,
    pub idx_outer: u16,
    pub idx_inner: u16,
    pub signer: Option<Arc<Vec<String>>>,
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

pub struct PumpswapEventParser;

impl PumpswapEventParser {
    pub fn new() -> Self {
        Self
    }

    /// Parse instructions using TransactionAdapter (for backward compatibility)
    pub fn parse_instructions(
        &self,
        adapter: &TransactionAdapter,
        instructions: &[ClassifiedInstruction],
    ) -> Result<Vec<PumpswapEvent>, PumpfunError> {
        let mut events: Vec<PumpswapEvent> = Vec::with_capacity(instructions.len());
        let slot = adapter.slot();
        let timestamp = adapter.block_time();
        // ОПТИМИЗАЦИЯ: создаем Arc один раз для всех событий
        // ZERO-COPY: клонируем только один раз, переиспользуем Arc для всех событий
        let signature_arc = Arc::new(adapter.signature().to_string());
        let signers_arc = Arc::new(adapter.signers().to_vec());

        for classified in instructions {
            let data = match get_instruction_data(&classified.data) {
                Ok(d) => d,
                Err(_) => continue,
            };

            if let Some(event) = Self::parse_instruction_data(
                &data,
                slot,
                timestamp,
                &signature_arc,
                &signers_arc,
                classified.outer_index,
                classified.inner_index,
            )? {
                events.push(event);
            }
        }

        Ok(sort_by_idx(events))
    }
    
    /// Parse instructions using ZcAdapter (zero-copy version)
    /// 
    /// This method works directly with ZcInstruction data (references to buffer),
    /// avoiding base64 decoding and minimizing allocations.
    pub fn parse_instructions_zc<'a>(
        &self,
        adapter: &'a ZcAdapter<'a>,
        instructions: &[ZcClassifiedInstruction<'a>],
    ) -> Result<Vec<PumpswapEvent>, PumpfunError> {
        let mut events: Vec<PumpswapEvent> = Vec::with_capacity(instructions.len());
        let slot = adapter.slot();
        let timestamp = adapter.block_time();
        // ОПТИМИЗАЦИЯ: создаем Arc один раз для всех событий
        // ZERO-COPY: используем &str reference, но конвертируем в String только один раз
        let signature_arc = Arc::new(adapter.signature().to_string());
        // ZERO-COPY: собираем signers как Vec<String> один раз
        let signers_vec: Vec<String> = adapter.signers_iter()
            .map(|pk| bs58::encode(pk).into_string())
            .collect();
        let signers_arc = Arc::new(signers_vec);

        for classified in instructions {
            // ZERO-COPY: используем данные инструкции напрямую из буфера
            let data = adapter.instruction_data(classified.instruction);
            
            if data.len() < 16 {
                continue;
            }

            if let Some(event) = Self::parse_instruction_data(
                data,
                slot,
                timestamp,
                &signature_arc,
                &signers_arc,
                classified.outer_index,
                classified.inner_index,
            )? {
                events.push(event);
            }
        }

        Ok(sort_by_idx(events))
    }
    
    /// Parse instruction data (shared logic for both zero-copy and owned versions)
    /// 
    /// # Arguments
    /// * `data` - Instruction data bytes (either from buffer or decoded base64)
    /// * `slot` - Transaction slot
    /// * `timestamp` - Transaction timestamp
    /// * `signature_arc` - Shared signature Arc
    /// * `signers_arc` - Shared signers Arc
    /// * `outer_index` - Outer instruction index
    /// * `inner_index` - Inner instruction index (None for outer)
    /// 
    /// # Returns
    /// Optional event if discriminator matches
    fn parse_instruction_data(
        data: &[u8],
        slot: u64,
        timestamp: u64,
        signature_arc: &Arc<String>,
        signers_arc: &Arc<Vec<String>>,
        outer_index: usize,
        inner_index: Option<usize>,
    ) -> Result<Option<PumpswapEvent>, PumpfunError> {
        if data.len() < 16 {
            return Ok(None);
        }

        // ОПТИМИЗАЦИЯ: используем u128 для быстрого сравнения
        let disc_bytes: [u8; 16] = match data[..16].try_into() {
            Ok(b) => b,
            Err(_) => return Ok(None),
        };
        let disc_u128 = u128::from_le_bytes(disc_bytes);
        
        let event_type = match disc_u128 {
            x if x == pumpswap_events::CREATE_POOL_U128 => Some(PumpswapEventType::Create),
            x if x == pumpswap_events::ADD_LIQUIDITY_U128 => Some(PumpswapEventType::Add),
            x if x == pumpswap_events::REMOVE_LIQUIDITY_U128 => Some(PumpswapEventType::Remove),
            x if x == pumpswap_events::BUY_U128 => Some(PumpswapEventType::Buy),
            x if x == pumpswap_events::SELL_U128 => Some(PumpswapEventType::Sell),
            _ => {
                // ОПТИМИЗАЦИЯ: логируем только при debug уровне
                #[cfg(debug_assertions)]
                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!("Unexpected discriminator: {}", hex::encode(&data[..16]));
                }
                None
            }
        };

        if let Some(event_type) = event_type {
            // ZERO-COPY: передаем срез напрямую из буфера
            let payload = &data[16..];

            let data_enum = match event_type {
                PumpswapEventType::Buy => {
                    PumpswapEventData::Buy(Self::decode_buy_event(payload)?)
                }
                PumpswapEventType::Sell => {
                    PumpswapEventData::Sell(Self::decode_sell_event(payload)?)
                }
                PumpswapEventType::Create => {
                    PumpswapEventData::Create(Self::decode_create_event(payload)?)
                }
                PumpswapEventType::Add => {
                    PumpswapEventData::Deposit(Self::decode_add_liquidity(payload)?)
                }
                PumpswapEventType::Remove => {
                    PumpswapEventData::Withdraw(Self::decode_remove_liquidity(payload)?)
                }
            };

            let outer_idx = outer_index as u16;
            let inner_idx = inner_index.unwrap_or(0) as u16;
            // ОПТИМИЗАЦИЯ: создаем idx строку только для совместимости
            let idx = format!("{}-{}", outer_idx, inner_idx);

            Ok(Some(PumpswapEvent {
                event_type,
                data: data_enum,
                slot,
                timestamp,
                signature: Arc::clone(signature_arc),
                idx,
                idx_outer: outer_idx,
                idx_inner: inner_idx,
                signer: Some(Arc::clone(signers_arc)),
            }))
        } else {
            Ok(None)
        }
    }

    #[inline]
    fn decode_buy_event(data: &[u8]) -> Result<PumpswapBuyEvent, PumpfunError> {
        let mut reader = BinaryReaderRef::new_ref(data);

        // ОПТИМИЗАЦИЯ: inline normalize_timestamp
        let ts = reader.read_i64()?;
        let timestamp = if ts >= 0 { ts as u64 } else { 0 };
        let ev = PumpswapBuyEvent {
            timestamp,
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

        Ok(ev)
    }

    #[inline]
    fn decode_sell_event(data: &[u8]) -> Result<PumpswapSellEvent, PumpfunError> {
        let mut reader = BinaryReaderRef::new_ref(data);

        let ts = reader.read_i64()?;
        let timestamp = if ts >= 0 { ts as u64 } else { 0 };
        let ev = PumpswapSellEvent {
            timestamp,
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

        Ok(ev)
    }

    #[inline]
    fn decode_add_liquidity(data: &[u8]) -> Result<PumpswapDepositEvent, PumpfunError> {
        let mut reader = BinaryReaderRef::new_ref(data);

        let ts = reader.read_i64()?;
        let timestamp = if ts >= 0 { ts as u64 } else { 0 };
        let ev = PumpswapDepositEvent {
            timestamp,
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

        Ok(ev)
    }

    #[inline]
    fn decode_create_event(data: &[u8]) -> Result<PumpswapCreatePoolEvent, PumpfunError> {
        let mut reader = BinaryReaderRef::new_ref(data);

        let ts = reader.read_i64()?;
        let timestamp = if ts >= 0 { ts as u64 } else { 0 };
        let ev = PumpswapCreatePoolEvent {
            timestamp,
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

        Ok(ev)
    }

    #[inline]
    fn decode_remove_liquidity(data: &[u8]) -> Result<PumpswapWithdrawEvent, PumpfunError> {
        let mut reader = BinaryReaderRef::new_ref(data);

        let ts = reader.read_i64()?;
        let timestamp = if ts >= 0 { ts as u64 } else { 0 };
        let ev = PumpswapWithdrawEvent {
            timestamp,
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

        Ok(ev)
    }
}

impl HasIdx for PumpswapEvent {
    #[inline]
    fn idx(&self) -> &str { &self.idx }
    
    // ОПТИМИЗАЦИЯ: переопределяем idx_key для использования числовых значений напрямую
    #[inline]
    fn idx_key(&self) -> (u32, u32) {
        (self.idx_outer as u32, self.idx_inner as u32)
    }
}

