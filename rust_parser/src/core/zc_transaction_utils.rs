//! Zero-copy transaction utils for ZcAdapter
//! 
//! This module provides zero-copy utilities for working with ZcAdapter,
//! parsing transfers directly from ZcInstruction without converting to SolanaInstruction.

use std::collections::HashMap;
use once_cell::sync::Lazy;

use crate::core::constants::{dex_program_names, SKIP_PROGRAM_IDS, SYSTEM_PROGRAMS};
use crate::core::zc_adapter::ZcAdapter;
use crate::types::{DexInfo, SolanaInstruction, TradeInfo, TradeType, TransferData, TransferMap};

/// Token Program ID as 32-byte array (decoded once at startup)
static TOKEN_PROGRAM_ID_BYTES: Lazy<[u8; 32]> = Lazy::new(|| {
    const TOKEN_PROGRAM_ID_STR: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
    if let Ok(decoded) = bs58::decode(TOKEN_PROGRAM_ID_STR).into_vec() {
        if decoded.len() == 32 {
            let mut key = [0u8; 32];
            key.copy_from_slice(&decoded);
            return key;
        }
    }
    [0u8; 32] // Fallback (should never happen)
});

/// Token 2022 Program ID as 32-byte array (decoded once at startup)
static TOKEN_2022_PROGRAM_ID_BYTES: Lazy<[u8; 32]> = Lazy::new(|| {
    const TOKEN_2022_PROGRAM_ID_STR: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";
    if let Ok(decoded) = bs58::decode(TOKEN_2022_PROGRAM_ID_STR).into_vec() {
        if decoded.len() == 32 {
            let mut key = [0u8; 32];
            key.copy_from_slice(&decoded);
            return key;
        }
    }
    [0u8; 32] // Fallback (should never happen)
});

/// Zero-copy transaction utils for ZcAdapter
pub struct ZcTransactionUtils<'a> {
    adapter: &'a ZcAdapter<'a>,
}

impl<'a> ZcTransactionUtils<'a> {
    /// Create new zero-copy transaction utils
    pub fn new(adapter: &'a ZcAdapter<'a>) -> Self {
        Self { adapter }
    }

    /// Get DEX info from instruction classifier (zero-copy)
    /// 
    /// # Arguments
    /// * `classifier` - Zero-copy instruction classifier
    /// 
    /// # Returns
    /// DEX info with program ID and AMM name
    pub fn get_dex_info(
        &self,
        classifier: &crate::core::zc_instruction_classifier::ZcInstructionClassifier<'a>,
    ) -> DexInfo {
        // ZERO-COPY: используем итератор, конвертируем в String только для первого program_id
        let program_id_opt = classifier
            .get_all_program_ids_iter()
            .next()
            .map(|pid| bs58::encode(pid).into_string());
        
        let amm = program_id_opt
            .as_ref()
            .map(|id| dex_program_names::name(id).to_string());
        
        DexInfo {
            program_id: program_id_opt.clone(),
            amm: amm.clone(),
            route: None,
        }
    }

    /// Get transfer actions from instructions (zero-copy)
    /// 
    /// # Returns
    /// Transfer map grouped by program ID
    pub fn get_transfer_actions(&self) -> TransferMap {
        Self::create_transfers_from_instructions_zc(self.adapter)
    }

    /// Create transfers from instructions (zero-copy version)
    /// 
    /// # Arguments
    /// * `adapter` - Zero-copy adapter
    /// 
    /// # Returns
    /// Transfer map grouped by program ID
    fn create_transfers_from_instructions_zc(adapter: &'a ZcAdapter<'a>) -> TransferMap {
        // Pre-allocate with estimated capacity
        let estimated_transfers = adapter.instructions().len() * 3;
        let mut actions: TransferMap = HashMap::with_capacity(estimated_transfers.min(32));
        
        // Buffer for formatting numbers (avoid format!)
        let mut idx_buf = String::with_capacity(16);
        
        // Process outer instructions (zero-copy: work with ZcInstruction directly)
        for (outer_index, instruction) in adapter.instructions().iter().enumerate() {
            // Get program ID (zero-copy: 32-byte array)
            let program_id = match adapter.program_id(instruction) {
                Some(pid) => pid,
                None => continue,
            };
            
            // Check if this is a Token Program instruction (zero-copy: compare 32-byte arrays directly)
            if program_id != &*TOKEN_PROGRAM_ID_BYTES && program_id != &*TOKEN_2022_PROGRAM_ID_BYTES {
                continue;
            }
            
            // Format idx (minimal allocation)
            idx_buf.clear();
            let mut num_buf = itoa::Buffer::new();
            idx_buf.push_str(num_buf.format(outer_index));
            
            // Parse instruction action (zero-copy: work with instruction data directly)
            if let Some(transfer_data) = Self::parse_instruction_action_zc(
                adapter,
                instruction,
                program_id,
                &idx_buf,
            ) {
                // Convert program_id to String for HashMap key (only once per program)
                let program_id_str = bs58::encode(program_id).into_string();
                actions
                    .entry(program_id_str)
                    .or_insert_with(|| Vec::with_capacity(4))
                    .push(transfer_data);
            }
        }
        
        // Process inner instructions from meta JSON (if available)
        if let Some(inner_instructions_json) = adapter.inner_instructions() {
            if let Some(inner_array) = inner_instructions_json.as_array() {
                for inner_set in inner_array {
                    if let Some(index) = inner_set.get("index").and_then(|v| v.as_u64()) {
                        let outer_index = index as usize;
                        
                        // Get outer instruction program ID
                        let outer_instruction = adapter.instruction(outer_index);
                        let outer_program_id = outer_instruction
                            .and_then(|ix| adapter.program_id(ix))
                            .map(|pid| bs58::encode(pid).into_string());
                        
                        // Skip system programs
                        if let Some(ref pid_str) = outer_program_id {
                            if SYSTEM_PROGRAMS.iter().any(|&p| p == pid_str) {
                                continue;
                            }
                            if SKIP_PROGRAM_IDS.iter().any(|&p| p == pid_str) {
                                continue;
                            }
                        }
                        
                        // Process inner instructions
                        if let Some(instructions_array) = inner_set.get("instructions").and_then(|v| v.as_array()) {
                            for (inner_index, inner_ix_json) in instructions_array.iter().enumerate() {
                                // Parse inner instruction from JSON
                                if let Some(transfer_data) = Self::parse_inner_instruction_zc(
                                    adapter,
                                    inner_ix_json,
                                    outer_index,
                                    inner_index,
                                    &outer_program_id,
                                ) {
                                    // Use outer program ID or "transfer" as key
                                    let program_id_str = outer_program_id
                                        .clone()
                                        .unwrap_or_else(|| "transfer".to_string());
                                    actions
                                        .entry(program_id_str)
                                        .or_insert_with(|| Vec::with_capacity(4))
                                        .push(transfer_data);
                                }
                            }
                        }
                    }
                }
            }
        }
        
        actions
    }

    /// Parse instruction action (zero-copy version)
    /// 
    /// # Arguments
    /// * `adapter` - Zero-copy adapter
    /// * `instruction` - Zero-copy instruction
    /// * `program_id` - Program ID as 32-byte array
    /// * `idx` - Instruction index string
    /// 
    /// # Returns
    /// Optional transfer data
    fn parse_instruction_action_zc(
        adapter: &'a ZcAdapter<'a>,
        instruction: &crate::core::zero_copy::ZcInstruction<'a>,
        program_id: &[u8; 32],
        idx: &str,
    ) -> Option<TransferData> {
        use crate::core::utils::get_instruction_data_zc;
        use crate::types::TokenAmount;
        
        const TRANSFER: u8 = 3;
        const TRANSFER_CHECKED: u8 = 12;
        
        // Get instruction data (zero-copy: reference to buffer)
        let data = get_instruction_data_zc(instruction);
        if data.is_empty() {
            return None;
        }
        
        let instruction_type = data[0];
        
        // Get instruction accounts (zero-copy: references)
        let account_indices = adapter.instruction_accounts(instruction);
        if account_indices.len() < 3 {
            return None;
        }
        
        // Get account keys from indices (zero-copy: convert to String only when needed)
        let source_index = account_indices[0] as usize;
        let destination_index = account_indices[1] as usize;
        let authority_index = account_indices.get(2).copied().map(|i| i as usize);
        
        let source_key = adapter.account_key(source_index)?;
        let destination_key = adapter.account_key(destination_index)?;
        
        // Convert to String only once (necessary for TransferData)
        let source = bs58::encode(source_key).into_string();
        let destination = bs58::encode(destination_key).into_string();
        
        match instruction_type {
            TRANSFER => {
                // TRANSFER: [source, destination, authority]
                Self::create_transfer_data_zc(
                    adapter,
                    program_id,
                    &source,
                    &destination,
                    None, // mint will be inferred from token balances
                    None, // decimals will be inferred from token balances
                    idx,
                    "transfer",
                    data,
                    TRANSFER,
                    &[],
                )
            }
            TRANSFER_CHECKED => {
                // TRANSFER_CHECKED: [source, destination, mint, authority]
                if account_indices.len() >= 4 {
                    let mint_index = account_indices[2] as usize;
                    let mint_key = adapter.account_key(mint_index)?;
                    let mint = bs58::encode(mint_key).into_string();
                    
                    Self::create_transfer_data_zc(
                        adapter,
                        program_id,
                        &source,
                        &destination,
                        Some(&mint),
                        None, // decimals will be inferred from token balances
                        idx,
                        "transferChecked",
                        data,
                        TRANSFER_CHECKED,
                        &[],
                    )
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Parse inner instruction from JSON (zero-copy where possible)
    /// 
    /// # Arguments
    /// * `adapter` - Zero-copy adapter
    /// * `inner_ix_json` - Inner instruction JSON
    /// * `outer_index` - Outer instruction index
    /// * `inner_index` - Inner instruction index
    /// * `outer_program_id` - Outer program ID string
    /// 
    /// # Returns
    /// Optional transfer data
    fn parse_inner_instruction_zc(
        adapter: &'a ZcAdapter<'a>,
        inner_ix_json: &serde_json::Value,
        outer_index: usize,
        inner_index: usize,
        outer_program_id: &Option<String>,
    ) -> Option<TransferData> {
        use crate::types::SolanaInstruction;
        
        // Parse inner instruction from JSON (this involves allocations, but necessary)
        // TODO: Create zero-copy version of InnerInstruction
        let inner_ix = match serde_json::from_value::<SolanaInstruction>(inner_ix_json.clone()) {
            Ok(ix) => ix,
            Err(_) => return None,
        };
        
        // Check if this is a Token Program instruction
        const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
        const TOKEN_2022_PROGRAM_ID: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";
        
        if inner_ix.program_id != TOKEN_PROGRAM_ID && inner_ix.program_id != TOKEN_2022_PROGRAM_ID {
            return None;
        }
        
        // Format idx
        let mut idx_buf = String::with_capacity(16);
        let mut num_buf = itoa::Buffer::new();
        idx_buf.push_str(num_buf.format(outer_index));
        idx_buf.push('-');
        idx_buf.push_str(num_buf.format(inner_index));
        
        // Decode instruction data from base64
        let data = match base64_simd::STANDARD.decode_to_vec(&inner_ix.data) {
            Ok(d) => d,
            Err(_) => return None,
        };
        
        if data.is_empty() {
            return None;
        }
        
        let instruction_type = data[0];
        let accounts = &inner_ix.accounts;
        
        const TRANSFER: u8 = 3;
        const TRANSFER_CHECKED: u8 = 12;
        
        // Decode program ID to 32-byte array
        let program_id_bytes = match bs58::decode(&inner_ix.program_id).into_vec() {
            Ok(v) if v.len() == 32 => {
                let mut array = [0u8; 32];
                array.copy_from_slice(&v);
                array
            },
            _ => return None,
        };
        
        match instruction_type {
            TRANSFER => {
                // TRANSFER: [source, destination, authority]
                if accounts.len() >= 3 {
                    let source = accounts.get(0)?.clone();
                    let destination = accounts.get(1)?.clone();
                    
                    Self::create_transfer_data_zc(
                        adapter,
                        &program_id_bytes,
                        &source,
                        &destination,
                        None, // mint will be inferred from token balances
                        None, // decimals will be inferred from token balances
                        &idx_buf,
                        "transfer",
                        &data,
                        TRANSFER,
                        accounts,
                    )
                } else {
                    None
                }
            }
            TRANSFER_CHECKED => {
                // TRANSFER_CHECKED: [source, destination, mint, authority]
                if accounts.len() >= 4 {
                    let source = accounts.get(0)?.clone();
                    let destination = accounts.get(1)?.clone();
                    let mint = accounts.get(2)?.clone();
                    
                    Self::create_transfer_data_zc(
                        adapter,
                        &program_id_bytes,
                        &source,
                        &destination,
                        Some(&mint),
                        None, // decimals will be inferred from token balances
                        &idx_buf,
                        "transferChecked",
                        &data,
                        TRANSFER_CHECKED,
                        accounts,
                    )
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Create transfer data (zero-copy where possible)
    /// 
    /// # Arguments
    /// * `adapter` - Zero-copy adapter
    /// * `program_id` - Program ID as 32-byte array
    /// * `source` - Source account string
    /// * `destination` - Destination account string
    /// * `mint_opt` - Optional mint string
    /// * `decimals_opt` - Optional decimals
    /// * `idx` - Instruction index string
    /// * `transfer_type` - Transfer type string
    /// * `data` - Instruction data bytes
    /// * `instruction_type` - Instruction type byte
    /// * `accounts` - Instruction accounts (for inner instructions)
    /// 
    /// # Returns
    /// Optional transfer data
    #[inline]
    fn create_transfer_data_zc(
        adapter: &'a ZcAdapter<'a>,
        program_id: &[u8; 32],
        source: &str,
        destination: &str,
        mint_opt: Option<&str>,
        decimals_opt: Option<u8>,
        idx: &str,
        transfer_type: &str,
        data: &[u8],
        instruction_type: u8,
        accounts: &[String],
    ) -> Option<TransferData> {
        use crate::types::TokenAmount;
        
        // Determine mint (optimized: use token balances from meta JSON)
        let mint = if let Some(m) = mint_opt {
            m.to_string()
        } else {
            // Try to find mint from token balances (parse from meta JSON)
            let mint_str = Self::find_mint_from_token_balances(adapter, source, destination);
            
            if mint_str.is_empty() {
                // If program_id is Token Program, this might be SOL or native token
                // For now, return None if mint is not found
                return None;
            }
            
            mint_str
        };
        
        if mint.is_empty() {
            return None;
        }
        
        // Get decimals (cache lookup or use default)
        let decimals = decimals_opt
            .unwrap_or_else(|| {
                // Try to get decimals from token balances
                Self::get_decimals_from_token_balances(adapter, &mint)
                    .unwrap_or(9) // Default to 9 for SOL
            });
        
        // Read amount from data (fast read without checks)
        let amount_raw = if data.len() >= 9 {
            // TRANSFER: amount is at offset 1 (u64)
            // TRANSFER_CHECKED: amount is at offset 1 (u64)
            let amount_bytes: [u8; 8] = data[1..9].try_into().ok()?;
            u64::from_le_bytes(amount_bytes)
        } else {
            return None; // Early exit if data is insufficient
        };
        
        // Fast calculation of amount_ui (avoid powi if possible)
        let amount_ui = if decimals == 9 {
            amount_raw as f64 / 1_000_000_000.0
        } else if decimals == 6 {
            amount_raw as f64 / 1_000_000.0
        } else {
            amount_raw as f64 / 10f64.powi(decimals as i32)
        };
        
        // Get token balances from meta JSON (zero-copy: references to JSON)
        let source_balance = Self::get_token_balance_from_meta(adapter, source);
        let dest_balance = Self::get_token_balance_from_meta(adapter, destination);
        
        // Create transfer data (allocations only for output struct)
        let program_id_str = bs58::encode(program_id).into_string();
        
        Some(TransferData {
            transfer_type: transfer_type.to_string(),
            program_id: program_id_str,
            info: crate::types::TransferInfo {
                source: source.to_string(),
                destination: destination.to_string(),
                mint: mint.clone(),
                token_amount: TokenAmount {
                    amount: amount_raw.to_string(),
                    decimals,
                    ui_amount: Some(amount_ui),
                },
                authority: accounts.get(2).or_else(|| accounts.get(3)).cloned(),
                destination_owner: None,
                destination_balance: dest_balance.clone(),
                destination_pre_balance: None,
                source_balance: source_balance.clone(),
                source_pre_balance: None,
                sol_balance_change: None,
            },
            idx: idx.to_string(),
            timestamp: adapter.block_time(),
            signature: adapter.signature().to_string(),
            is_fee: false,
        })
    }

    /// Process swap data from transfers (zero-copy version)
    /// 
    /// # Arguments
    /// * `transfers` - Transfer data slice
    /// * `dex_info` - DEX info
    /// 
    /// # Returns
    /// Optional trade info
    /// 
    /// # Note
    /// This method works directly with TransferData, avoiding TransactionAdapter.
    /// Allocations only occur when creating the final TradeInfo struct.
    pub fn process_swap_data(
        &self,
        transfers: &[TransferData],
        dex_info: &DexInfo,
    ) -> Option<TradeInfo> {
        if transfers.is_empty() {
            return None;
        }
        
        // Find unique mints (zero-copy: use references)
        let mut unique_mints: Vec<&str> = Vec::new();
        for transfer in transfers {
            if !transfer.info.mint.is_empty() && !unique_mints.contains(&transfer.info.mint.as_str()) {
                unique_mints.push(&transfer.info.mint);
            }
        }
        
        if unique_mints.len() < 2 {
            return None;
        }
        
        // Determine input and output mints (first and last unique token)
        let mut input_mint = unique_mints[0];
        let mut output_mint = unique_mints[unique_mints.len() - 1];
        
        // Check swap direction (if outputToken.source == signer, swap)
        let signer_key = self.adapter.signer();
        let signer_str = signer_key.map(|pk| bs58::encode(pk).into_string());
        let output_transfer = transfers.iter().find(|t| t.info.mint == output_mint);
        if let Some(output) = output_transfer {
            if let Some(ref signer) = signer_str {
                if output.info.source == *signer || output.info.authority.as_ref().map(|a| a == signer).unwrap_or(false) {
                    // Swap input and output
                    std::mem::swap(&mut input_mint, &mut output_mint);
                }
            }
        }
        
        // Sum all transfers for each mint
        let mut input_amount = 0.0;
        let mut input_amount_raw = 0u128;
        let mut output_amount = 0.0;
        let mut output_amount_raw = 0u128;
        let mut input_decimals = 0u8;
        let mut output_decimals = 0u8;
        let mut input_transfer_ref: Option<&TransferData> = None;
        let mut output_transfer_ref: Option<&TransferData> = None;
        
        for transfer in transfers {
            if transfer.info.mint == input_mint {
                let amount = transfer.info.token_amount.ui_amount.unwrap_or_else(|| {
                    transfer.info.token_amount.amount.parse::<f64>().unwrap_or(0.0)
                });
                let amount_raw = transfer.info.token_amount.amount.parse::<u128>().unwrap_or(0);
                input_amount += amount;
                input_amount_raw += amount_raw;
                input_decimals = transfer.info.token_amount.decimals;
                if input_transfer_ref.is_none() {
                    input_transfer_ref = Some(transfer);
                }
            } else if transfer.info.mint == output_mint {
                let amount = transfer.info.token_amount.ui_amount.unwrap_or_else(|| {
                    transfer.info.token_amount.amount.parse::<f64>().unwrap_or(0.0)
                });
                let amount_raw = transfer.info.token_amount.amount.parse::<u128>().unwrap_or(0);
                output_amount += amount;
                output_amount_raw += amount_raw;
                output_decimals = transfer.info.token_amount.decimals;
                if output_transfer_ref.is_none() {
                    output_transfer_ref = Some(transfer);
                }
            }
        }
        
        let input = match input_transfer_ref {
            Some(transfer) => transfer,
            None => transfers.first()?,
        };
        let output = match output_transfer_ref {
            Some(transfer) => transfer,
            None => transfers.get(1)?,
        };
        
        let program_id = dex_info
            .program_id
            .as_ref()
            .cloned()
            .unwrap_or_else(|| input.program_id.clone());
        let amm = dex_info
            .amm
            .as_ref()
            .cloned()
            .unwrap_or_else(|| dex_program_names::name(&program_id).to_string());
        
        let input_token = crate::types::TokenInfo {
            mint: input.info.mint.clone(),
            amount: input_amount,
            amount_raw: input_amount_raw.to_string(),
            decimals: input_decimals,
            authority: input.info.authority.clone(),
            destination: Some(input.info.destination.clone()),
            destination_owner: input.info.destination_owner.clone(),
            destination_balance: input.info.destination_balance.clone(),
            destination_pre_balance: input.info.destination_pre_balance.clone(),
            source: Some(input.info.source.clone()),
            source_balance: input.info.source_balance.clone(),
            source_pre_balance: input.info.source_pre_balance.clone(),
            destination_balance_change: None,
            source_balance_change: None,
            balance_change: input.info.sol_balance_change.clone(),
        };
        
        let output_token = crate::types::TokenInfo {
            mint: output.info.mint.clone(),
            amount: output_amount,
            amount_raw: output_amount_raw.to_string(),
            decimals: output_decimals,
            authority: output.info.authority.clone(),
            destination: Some(output.info.destination.clone()),
            destination_owner: output.info.destination_owner.clone(),
            destination_balance: output.info.destination_balance.clone(),
            destination_pre_balance: output.info.destination_pre_balance.clone(),
            source: Some(output.info.source.clone()),
            source_balance: output.info.source_balance.clone(),
            source_pre_balance: output.info.source_pre_balance.clone(),
            destination_balance_change: None,
            source_balance_change: None,
            balance_change: output.info.sol_balance_change.clone(),
        };
        
        Some(TradeInfo {
            trade_type: TradeType::Swap,
            pool: Vec::new(),
            input_token,
            output_token,
            slippage_bps: None,
            fee: None,
            fees: Vec::new(),
            user: Some(input.info.source.clone()),
            program_id: Some(program_id),
            amm: Some(amm),
            amms: None,
            route: dex_info.route.clone(),
            slot: self.adapter.slot(),
            timestamp: self.adapter.block_time(),
            signature: self.adapter.signature().to_string(),
            idx: input.idx.clone(),
            signer: Some(
                self.adapter.signers_iter()
                    .map(|pk| bs58::encode(pk).into_string())
                    .collect()
            ),
        })
    }
    
    /// Find mint from token balances (parse from meta JSON)
    fn find_mint_from_token_balances(
        adapter: &'a ZcAdapter<'a>,
        source: &str,
        destination: &str,
    ) -> String {
        // Try post token balances first
        if let Some(post_balances) = adapter.post_token_balances() {
            if let Some(balances_array) = post_balances.as_array() {
                for balance in balances_array {
                    if let Some(account) = balance.get("account").and_then(|v| v.as_str()) {
                        if account == source || account == destination {
                            if let Some(mint) = balance.get("mint").and_then(|v| v.as_str()) {
                                return mint.to_string();
                            }
                        }
                    }
                }
            }
        }
        
        // Try pre token balances
        if let Some(pre_balances) = adapter.pre_token_balances() {
            if let Some(balances_array) = pre_balances.as_array() {
                for balance in balances_array {
                    if let Some(account) = balance.get("account").and_then(|v| v.as_str()) {
                        if account == source || account == destination {
                            if let Some(mint) = balance.get("mint").and_then(|v| v.as_str()) {
                                return mint.to_string();
                            }
                        }
                    }
                }
            }
        }
        
        String::new()
    }
    
    /// Get decimals from token balances (parse from meta JSON)
    fn get_decimals_from_token_balances(
        adapter: &'a ZcAdapter<'a>,
        mint: &str,
    ) -> Option<u8> {
        // Try post token balances first
        if let Some(post_balances) = adapter.post_token_balances() {
            if let Some(balances_array) = post_balances.as_array() {
                for balance in balances_array {
                    if let Some(mint_str) = balance.get("mint").and_then(|v| v.as_str()) {
                        if mint_str == mint {
                            if let Some(ui_token_amount) = balance.get("uiTokenAmount") {
                                if let Some(decimals) = ui_token_amount.get("decimals").and_then(|v| v.as_u64()) {
                                    return Some(decimals as u8);
                                }
                            }
                        }
                    }
                }
            }
        }
        
        // Try pre token balances
        if let Some(pre_balances) = adapter.pre_token_balances() {
            if let Some(balances_array) = pre_balances.as_array() {
                for balance in balances_array {
                    if let Some(mint_str) = balance.get("mint").and_then(|v| v.as_str()) {
                        if mint_str == mint {
                            if let Some(ui_token_amount) = balance.get("uiTokenAmount") {
                                if let Some(decimals) = ui_token_amount.get("decimals").and_then(|v| v.as_u64()) {
                                    return Some(decimals as u8);
                                }
                            }
                        }
                    }
                }
            }
        }
        
        None
    }
    
    /// Get token balance from meta JSON
    fn get_token_balance_from_meta(
        adapter: &'a ZcAdapter<'a>,
        account: &str,
    ) -> Option<crate::types::TokenAmount> {
        // Try post token balances first
        if let Some(post_balances) = adapter.post_token_balances() {
            if let Some(balances_array) = post_balances.as_array() {
                for balance in balances_array {
                    if let Some(account_str) = balance.get("account").and_then(|v| v.as_str()) {
                        if account_str == account {
                            if let Some(ui_token_amount) = balance.get("uiTokenAmount") {
                                let amount = ui_token_amount.get("amount").and_then(|v| v.as_str())?;
                                let decimals = ui_token_amount.get("decimals").and_then(|v| v.as_u64())? as u8;
                                let ui_amount = ui_token_amount.get("uiAmount").and_then(|v| v.as_f64());
                                
                                return Some(crate::types::TokenAmount {
                                    amount: amount.to_string(),
                                    decimals,
                                    ui_amount,
                                });
                            }
                        }
                    }
                }
            }
        }
        
        None
    }
    
    /// Get token account owner from meta JSON
    fn get_token_account_owner_from_meta(
        adapter: &'a ZcAdapter<'a>,
        account: &str,
    ) -> Option<String> {
        // Try post token balances first
        if let Some(post_balances) = adapter.post_token_balances() {
            if let Some(balances_array) = post_balances.as_array() {
                for balance in balances_array {
                    if let Some(account_str) = balance.get("account").and_then(|v| v.as_str()) {
                        if account_str == account {
                            if let Some(owner) = balance.get("owner").and_then(|v| v.as_str()) {
                                return Some(owner.to_string());
                            }
                        }
                    }
                }
            }
        }
        
        // Try pre token balances
        if let Some(pre_balances) = adapter.pre_token_balances() {
            if let Some(balances_array) = pre_balances.as_array() {
                for balance in balances_array {
                    if let Some(account_str) = balance.get("account").and_then(|v| v.as_str()) {
                        if account_str == account {
                            if let Some(owner) = balance.get("owner").and_then(|v| v.as_str()) {
                                return Some(owner.to_string());
                            }
                        }
                    }
                }
            }
        }
        
        None
    }
}
