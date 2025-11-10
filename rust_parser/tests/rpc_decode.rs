use anyhow::Result;
use serde_json::Value;
use solana_dex_parser::{DexParser, ParseConfig};

#[path = "common/mod.rs"]
mod rpc_helpers;

use rpc_helpers::fetch_transaction_with_fallback;

const DEFAULT_SIGNATURE: &str =
    "b15toBqDHKvVy7KQeAMDiEfinqg4Y8tDorUNHBd4FVojvqGyvZMELVkAz5BrNrc9AiA1zvRAZ9FfWM7qjWUQW9u";

#[test]
#[ignore]
fn fetch_and_decode_live_transaction() -> Result<()> {
    let rpc_url = std::env::var("SOLANA_RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let requested_signature = std::env::var("SOLANA_TX_SIGNATURE").ok();
    let signature_str = requested_signature.as_deref().unwrap_or(DEFAULT_SIGNATURE);

    let tx = fetch_transaction_with_fallback(&rpc_url, Some(signature_str))?;
    let parser = DexParser::new();
    let result = parser.parse_all(tx, Some(ParseConfig::default()));

    // Help manual debugging by showing a readable summary of what we parsed.
    let summary: Value = serde_json::to_value(&result)?;
    println!(
        "Parsed result summary: {}",
        serde_json::to_string_pretty(&summary)?
    );

    // We do not assert on concrete trade output, but the parser should at least produce metadata.
    assert!(!result.signature.is_empty());
    assert!(result.slot > 0);

    Ok(())
}
