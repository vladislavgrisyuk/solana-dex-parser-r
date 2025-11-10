use std::collections::HashMap;
use std::str::FromStr;

use anyhow::{anyhow, Context, Result};
use solana_client::rpc_client::RpcClient;
use solana_client::rpc_config::RpcTransactionConfig;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::signature::Signature;
use solana_transaction_status::{
    EncodedConfirmedTransactionWithStatusMeta, EncodedTransaction, UiCompiledInstruction,
    UiInnerInstructions, UiInstruction, UiLoadedAddresses, UiMessage, UiParsedInstruction,
    UiTransactionEncoding, UiTransactionStatusMeta, UiTransactionTokenBalance,
};

use crate::types::{
    BalanceChange, InnerInstruction, SolanaInstruction, SolanaTransaction, TokenAmount,
    TokenBalance, TransactionMeta, TransactionStatus,
};

type MessageExtraction = (Vec<SolanaInstruction>, Vec<String>, Vec<String>, String);

/// Fetch a transaction from RPC and convert it into the internal SolanaTransaction type.
pub fn fetch_transaction(rpc_url: &str, signature: &str) -> Result<SolanaTransaction> {
    let client = RpcClient::new(rpc_url.to_string());
    let signature = Signature::from_str(signature).context("invalid signature")?;
    let config = RpcTransactionConfig {
        encoding: Some(UiTransactionEncoding::Json), // Uses base64 encoding for instruction data (20–50× faster than bs58)
        commitment: Some(CommitmentConfig::confirmed()),
        max_supported_transaction_version: Some(0),
    };

    let encoded = client
        .get_transaction_with_config(&signature, config)
        .with_context(|| format!("failed to fetch transaction {signature}"))?;
    convert_transaction(encoded)
}

fn convert_transaction(tx: EncodedConfirmedTransactionWithStatusMeta) -> Result<SolanaTransaction> {
    let meta = tx
        .transaction
        .meta
        .as_ref()
        .context("transaction missing status meta")?;
    let (instructions, account_keys, signers, signature) =
        extract_message(&tx.transaction.transaction, meta)?;

    let inner_instructions =
        convert_inner_instructions(meta.inner_instructions.as_ref().into(), &account_keys);
    let pre_token_balances =
        convert_token_balances(meta.pre_token_balances.as_ref().into(), &account_keys);
    let post_token_balances =
        convert_token_balances(meta.post_token_balances.as_ref().into(), &account_keys);

    let solana_tx = SolanaTransaction {
        slot: tx.slot,
        signature,
        block_time: tx.block_time.unwrap_or_default() as u64,
        signers,
        instructions,
        inner_instructions,
        transfers: Vec::new(),
        pre_token_balances,
        post_token_balances,
        meta: TransactionMeta {
            fee: meta.fee,
            compute_units: Option::<u64>::from(meta.compute_units_consumed.clone()).unwrap_or(0),
            status: if meta.err.is_some() {
                TransactionStatus::Failed
            } else {
                TransactionStatus::Success
            },
            sol_balance_changes: collect_sol_balance_changes(meta, &account_keys),
            token_balance_changes: HashMap::new(),
        },
    };

    Ok(solana_tx)
}

fn extract_message(
    encoded: &EncodedTransaction,
    meta: &UiTransactionStatusMeta,
) -> Result<MessageExtraction> {
    let ui_tx = match encoded {
        EncodedTransaction::Json(tx) => tx,
        _ => return Err(anyhow!("expected JSON encoded transaction")),
    };
    let signature = ui_tx
        .signatures
        .first()
        .cloned()
        .ok_or_else(|| anyhow!("transaction missing signature"))?;

    match &ui_tx.message {
        UiMessage::Raw(raw) => {
            let signers = raw
                .account_keys
                .iter()
                .take(raw.header.num_required_signatures as usize)
                .cloned()
                .collect();
            let mut account_keys = raw.account_keys.clone();
            append_loaded_addresses(&mut account_keys, meta);
            let instructions = raw
                .instructions
                .iter()
                .map(|ix| convert_compiled_instruction(ix, &account_keys))
                .collect();
            Ok((instructions, account_keys, signers, signature))
        }
        UiMessage::Parsed(parsed) => {
            let mut account_keys: Vec<String> = parsed
                .account_keys
                .iter()
                .map(|account| account.pubkey.clone())
                .collect();
            let signers = parsed
                .account_keys
                .iter()
                .filter(|account| account.signer)
                .map(|account| account.pubkey.clone())
                .collect();
            append_loaded_addresses(&mut account_keys, meta);
            let instructions = parsed
                .instructions
                .iter()
                .map(|ix| convert_ui_instruction(ix, &account_keys))
                .collect();
            Ok((instructions, account_keys, signers, signature))
        }
    }
}

fn append_loaded_addresses(keys: &mut Vec<String>, meta: &UiTransactionStatusMeta) {
    if let Some(loaded) = Option::<&UiLoadedAddresses>::from(meta.loaded_addresses.as_ref()) {
        keys.extend(loaded.writable.iter().cloned());
        keys.extend(loaded.readonly.iter().cloned());
    }
}

fn convert_inner_instructions(
    sets: Option<&Vec<UiInnerInstructions>>,
    account_keys: &[String],
) -> Vec<InnerInstruction> {
    sets.map(|inner_sets| {
        inner_sets
            .iter()
            .map(|set| InnerInstruction {
                index: set.index as usize,
                instructions: set
                    .instructions
                    .iter()
                    .map(|ix| convert_ui_instruction(ix, account_keys))
                    .collect(),
            })
            .collect()
    })
    .unwrap_or_default()
}

fn convert_token_balances(
    balances: Option<&Vec<UiTransactionTokenBalance>>,
    account_keys: &[String],
) -> Vec<TokenBalance> {
    balances
        .map(|items| {
            items
                .iter()
                .filter_map(|balance| {
                    let account = account_keys.get(balance.account_index as usize)?.clone();
                    Some(TokenBalance {
                        account,
                        mint: balance.mint.clone(),
                        owner: balance.owner.clone().into(),
                        ui_token_amount: TokenAmount {
                            amount: balance.ui_token_amount.amount.clone(),
                            ui_amount: balance.ui_token_amount.ui_amount,
                            decimals: balance.ui_token_amount.decimals,
                        },
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn collect_sol_balance_changes(
    meta: &UiTransactionStatusMeta,
    account_keys: &[String],
) -> HashMap<String, BalanceChange> {
    let mut changes = HashMap::new();
    for (idx, key) in account_keys.iter().enumerate() {
        if let (Some(pre), Some(post)) = (meta.pre_balances.get(idx), meta.post_balances.get(idx)) {
            if pre != post {
                changes.insert(
                    key.clone(),
                    BalanceChange {
                        pre: *pre as i128,
                        post: *post as i128,
                        change: *post as i128 - *pre as i128,
                    },
                );
            }
        }
    }
    changes
}

fn convert_compiled_instruction(
    instruction: &UiCompiledInstruction,
    account_keys: &[String],
) -> SolanaInstruction {
    let program_id = account_keys
        .get(instruction.program_id_index as usize)
        .cloned()
        .unwrap_or_default();
    let accounts = instruction
        .accounts
        .iter()
        .filter_map(|index| account_keys.get(*index as usize).cloned())
        .collect();
    SolanaInstruction {
        program_id,
        accounts,
        data: instruction.data.clone(),
    }
}

fn convert_ui_instruction(
    instruction: &UiInstruction,
    account_keys: &[String],
) -> SolanaInstruction {
    match instruction {
        UiInstruction::Compiled(compiled) => convert_compiled_instruction(compiled, account_keys),
        UiInstruction::Parsed(parsed) => match parsed {
            UiParsedInstruction::PartiallyDecoded(instruction) => SolanaInstruction {
                program_id: instruction.program_id.clone(),
                accounts: instruction.accounts.clone(),
                data: instruction.data.clone(),
            },
            UiParsedInstruction::Parsed(instruction) => SolanaInstruction {
                program_id: instruction.program_id.clone(),
                accounts: Vec::new(),
                data: instruction.parsed.to_string(),
            },
        },
    }
}
