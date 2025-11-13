use crate::core::constants::dex_program_names;
use crate::core::instruction_classifier::InstructionClassifier;
use crate::core::transaction_adapter::TransactionAdapter;
use crate::types::{DexInfo, FeeInfo, PoolEvent, TradeInfo, TradeType, TransferData, TransferMap};

pub struct TransactionUtils {
    pub(crate) adapter: TransactionAdapter,
}

impl TransactionUtils {
    pub fn new(adapter: TransactionAdapter) -> Self {
        Self { adapter }
    }

    pub fn get_dex_info(&self, classifier: &InstructionClassifier) -> DexInfo {
        let all_program_ids = classifier.get_all_program_ids();
        let program_id = all_program_ids.into_iter().next();
        let amm = program_id.as_ref().map(|id| dex_program_names::name(id).to_string());
        
        DexInfo {
            program_id: program_id.clone(),
            amm: amm.clone(),
            route: None,
        }
    }

    pub fn get_transfer_actions(&self) -> TransferMap {
        // В TypeScript версии transfers создаются из инструкций здесь
        // В Rust версии нужно создать transfers из инструкций, так как tx.transfers пусты
        // Сначала проверяем, есть ли уже transfers в транзакции
        let existing_transfers = self.adapter.get_transfer_actions();
        if !existing_transfers.is_empty() {
            return existing_transfers;
        }
        
        // Создаем transfers из инструкций, как в TypeScript версии
        Self::create_transfers_from_instructions(&self.adapter)
    }
    
    /// Создает transfers из инструкций (аналог TypeScript getTransferActions)
    fn create_transfers_from_instructions(adapter: &TransactionAdapter) -> TransferMap {
        use crate::core::constants::{SYSTEM_PROGRAMS, TOKENS};
        use std::collections::HashMap;
        
        const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
        const TOKEN_2022_PROGRAM_ID: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";
        const TRANSFER: u8 = 3;
        const TRANSFER_CHECKED: u8 = 12;
        
        let mut actions: TransferMap = HashMap::new();
        
        // Process inner instructions (как в TypeScript: process transfers of program instructions)
        for inner_set in adapter.inner_instructions() {
            let outer_index = inner_set.index;
            let outer_instruction = adapter.instructions().get(outer_index);
            let outer_program_id = outer_instruction.map(|ix| ix.program_id.as_str()).unwrap_or("");
            
            // Skip system programs
            if SYSTEM_PROGRAMS.contains(&outer_program_id) {
                continue;
            }
            
            let mut group_key = format!("{}:{}", outer_program_id, outer_index);
            
            for (inner_index, ix) in inner_set.instructions.iter().enumerate() {
                let inner_program_id = &ix.program_id;
                
                // Special case for meteora vault (как в TypeScript)
                if !SYSTEM_PROGRAMS.contains(&inner_program_id.as_str()) {
                    group_key = format!("{}:{}-{}", inner_program_id, outer_index, inner_index);
                    continue;
                }
                
                // Parse instruction action
                if let Some(transfer_data) = Self::parse_instruction_action(
                    adapter,
                    ix,
                    &format!("{}-{}", outer_index, inner_index),
                ) {
                    actions.entry(group_key.clone()).or_insert_with(Vec::new).push(transfer_data);
                }
            }
        }
        
        // Process outer instructions (как в TypeScript: process transfers without program)
        for (outer_index, ix) in adapter.instructions().iter().enumerate() {
            if let Some(transfer_data) = Self::parse_instruction_action(
                adapter,
                ix,
                &format!("{}", outer_index),
            ) {
                actions.entry("transfer".to_string()).or_insert_with(Vec::new).push(transfer_data);
            }
        }
        
        actions
    }
    
    /// Парсит инструкцию и создает TransferData (аналог TypeScript parseInstructionAction)
    fn parse_instruction_action(
        adapter: &TransactionAdapter,
        instruction: &crate::types::SolanaInstruction,
        idx: &str,
    ) -> Option<TransferData> {
        use crate::core::constants::{TOKENS, SYSTEM_PROGRAMS};
        use crate::core::utils::get_instruction_data;
        
        const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
        const TOKEN_2022_PROGRAM_ID: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";
        const TRANSFER: u8 = 3;
        const TRANSFER_CHECKED: u8 = 12;
        
        // Только для Token Program инструкций
        if instruction.program_id != TOKEN_PROGRAM_ID && instruction.program_id != TOKEN_2022_PROGRAM_ID {
            return None;
        }
        
        let data = get_instruction_data(instruction);
        if data.is_empty() {
            return None;
        }
        
        let instruction_type = data[0];
        let accounts = &instruction.accounts;
        
        match instruction_type {
            TRANSFER => {
                // transfer: [source, destination, authority]
                if accounts.len() >= 3 {
                    let source = accounts.get(0)?;
                    let destination = accounts.get(1)?;
                    Self::create_transfer_data(
                        adapter,
                        &instruction.program_id,
                        source,
                        destination,
                        None, // mint
                        None, // decimals
                        idx,
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
                // transferChecked: [source, mint, destination, authority, decimals]
                if accounts.len() >= 4 {
                    let source = accounts.get(0)?;
                    let mint = accounts.get(1)?;
                    let destination = accounts.get(2)?;
                    let decimals = if data.len() >= 10 { Some(data[9]) } else { None };
                    Self::create_transfer_data(
                        adapter,
                        &instruction.program_id,
                        source,
                        destination,
                        Some(mint),
                        decimals,
                        idx,
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
    
    /// Создает TransferData из данных инструкции
    fn create_transfer_data(
        adapter: &TransactionAdapter,
        program_id: &str,
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
        use crate::core::constants::TOKENS;
        
        const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
        const TOKEN_2022_PROGRAM_ID: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";
        
        // Определяем mint
        let mint = if let Some(m) = mint_opt {
            m.to_string()
        } else {
            // Пытаемся найти mint из token balances
            let dest_mint = adapter
                .token_account_info(destination)
                .map(|info| info.mint.clone());
            let source_mint = adapter
                .token_account_info(source)
                .map(|info| info.mint.clone());
            
            dest_mint.or(source_mint).unwrap_or_else(|| {
                // Если program_id - это Token Program, то это SOL или другой нативный токен
                if program_id == TOKEN_PROGRAM_ID || program_id == TOKEN_2022_PROGRAM_ID {
                    TOKENS.SOL.to_string()
                } else {
                    "".to_string()
                }
            })
        };
        
        if mint.is_empty() {
            return None;
        }
        
        // Получаем decimals
        let decimals = decimals_opt
            .or_else(|| {
                let d = adapter.get_token_decimals(&mint);
                if d > 0 { Some(d) } else { None }
            })
            .unwrap_or(9);
        
        // Читаем amount из данных
        let amount_raw = if data.len() >= 9 {
            // TRANSFER: amount is at offset 1 (u64)
            // TRANSFER_CHECKED: amount is at offset 1 (u64)
            let amount_bytes: [u8; 8] = data[1..9].try_into().ok()?;
            u64::from_le_bytes(amount_bytes)
        } else {
            0
        };
        
        let amount_ui = amount_raw as f64 / 10f64.powi(decimals as i32);
        
        // Получаем balances
        let source_balance = adapter.token_account_info(source).and_then(|info| {
            Some(crate::types::TokenAmount {
                amount: info.amount_raw.clone(),
                decimals: info.decimals,
                ui_amount: Some(info.amount),
            })
        });
        
        let destination_balance = adapter.token_account_info(destination).and_then(|info| {
            Some(crate::types::TokenAmount {
                amount: info.amount_raw.clone(),
                decimals: info.decimals,
                ui_amount: Some(info.amount),
            })
        });
        
        // Получаем authority
        const TRANSFER: u8 = 3;
        const TRANSFER_CHECKED: u8 = 12;
        let authority = if instruction_type == TRANSFER && accounts.len() >= 3 {
            accounts.get(2).cloned()
        } else if instruction_type == TRANSFER_CHECKED && accounts.len() >= 4 {
            accounts.get(3).cloned()
        } else {
            None
        };
        
        // Получаем destination owner
        let destination_owner = adapter.get_token_account_owner(destination);
        
        Some(TransferData {
            transfer_type: transfer_type.to_string(),
            program_id: program_id.to_string(),
            info: crate::types::TransferInfo {
                authority,
                destination: destination.to_string(),
                destination_owner,
                mint,
                source: source.to_string(),
                token_amount: crate::types::TokenAmount {
                    amount: amount_raw.to_string(),
                    decimals,
                    ui_amount: Some(amount_ui),
                },
                source_balance,
                source_pre_balance: None,
                destination_balance,
                destination_pre_balance: None,
                sol_balance_change: None,
            },
            idx: idx.to_string(),
            timestamp: adapter.block_time(),
            signature: adapter.signature().to_string(),
            is_fee: false,
        })
    }

    pub fn process_swap_data(
        &self,
        transfers: &[TransferData],
        dex_info: &DexInfo,
    ) -> Option<TradeInfo> {
        if transfers.len() < 2 {
            return None;
        }

        // Находим уникальные mints
        let mut unique_mints: Vec<&str> = Vec::new();
        for transfer in transfers {
            if !transfer.info.mint.is_empty() && !unique_mints.contains(&transfer.info.mint.as_str()) {
                unique_mints.push(&transfer.info.mint);
            }
        }

        if unique_mints.len() < 2 {
            return None;
        }

        // Определяем input и output mints (как в TypeScript: первый и последний unique token)
        let mut input_mint = unique_mints[0];
        let mut output_mint = unique_mints[unique_mints.len() - 1];

        // Проверяем направление swap (как в TypeScript calculateTokenAmounts)
        // Если outputToken.source == signer, то меняем местами
        let signer = self.adapter.signer();
        let output_transfer = transfers.iter().find(|t| t.info.mint == output_mint);
        if let Some(output) = output_transfer {
            if output.info.source == signer || output.info.authority.as_ref().map(|a| a == &signer).unwrap_or(false) {
                // Меняем местами input и output
                std::mem::swap(&mut input_mint, &mut output_mint);
            }
        }

        // Суммируем все transfers с каждым mint
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

        let input = input_transfer_ref.unwrap_or_else(|| transfers.first().unwrap());
        let output = output_transfer_ref.unwrap_or_else(|| transfers.get(1).unwrap());

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
            mint: input_mint.to_string(),
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
            mint: output_mint.to_string(),
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
            signer: Some(self.adapter.signers().to_vec()),
        })
    }

    pub fn attach_trade_fee(&self, mut trade: TradeInfo) -> TradeInfo {
        let fee_amount = self.adapter.fee();
        
        if fee_amount.amount != "0" {
            trade.fee = Some(FeeInfo {
                mint: "SOL".to_string(),
                amount: fee_amount.ui_amount.unwrap_or(0.0),
                amount_raw: fee_amount.amount.clone(),
                decimals: fee_amount.decimals,
                dex: None,
                fee_type: None,
                recipient: None,
            });
        }
        
        trade
    }

    pub fn attach_token_transfer_info(
        &self,
        trade: TradeInfo,
        _transfer_actions: &TransferMap,
    ) -> TradeInfo {
        trade
    }

    pub fn attach_user_balance_to_lps(&self, pools: Vec<PoolEvent>) -> Vec<PoolEvent> {
        let signer = self.adapter.signer();
        if !signer.is_empty() {
            pools
                .into_iter()
                .map(|mut pool| {
                    pool.idx = format!("{}-{}", signer, pool.idx);
                    pool
                })
                .collect()
        } else {
            pools
        }
    }
}

impl TransactionUtils {
    fn transfer_to_token_info(transfer: &TransferData) -> crate::types::TokenInfo {
        let amount = transfer.info.token_amount.ui_amount.unwrap_or_else(|| {
            transfer
                .info
                .token_amount
                .amount
                .parse::<f64>()
                .unwrap_or(0.0)
        });

        crate::types::TokenInfo {
            mint: transfer.info.mint.clone(),
            amount,
            amount_raw: transfer.info.token_amount.amount.clone(),
            decimals: transfer.info.token_amount.decimals,
            authority: transfer.info.authority.clone(),
            destination: Some(transfer.info.destination.clone()),
            destination_owner: transfer.info.destination_owner.clone(),
            destination_balance: transfer.info.destination_balance.clone(),
            destination_pre_balance: transfer.info.destination_pre_balance.clone(),
            source: Some(transfer.info.source.clone()),
            source_balance: transfer.info.source_balance.clone(),
            source_pre_balance: transfer.info.source_pre_balance.clone(),
            destination_balance_change: None,
            source_balance_change: None,
            balance_change: transfer.info.sol_balance_change.clone(),
        }
    }
}
