use std::str::FromStr;

use anyhow::{Context, Result};
use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;

use solana_dex_parser::rpc;
use solana_dex_parser::types::SolanaTransaction;

pub fn fetch_transaction_with_fallback(
    rpc_url: &str,
    explicit_signature: Option<&str>,
) -> Result<SolanaTransaction> {
    if let Some(sig) = explicit_signature {
        return rpc::fetch_transaction(rpc_url, sig);
    }

    let client = RpcClient::new(rpc_url.to_string());
    let signature = fetch_recent_signature(&client)?;
    rpc::fetch_transaction(rpc_url, &signature.to_string())
}

fn fetch_recent_signature(client: &RpcClient) -> Result<Signature> {
    let address = Pubkey::from_str("11111111111111111111111111111111")?;
    let mut signatures = client.get_signatures_for_address(&address)?;
    let sig = signatures
        .drain(..)
        .next()
        .context("no signatures returned for system program")?;
    Signature::from_str(&sig.signature).context("invalid signature from RPC response")
}
