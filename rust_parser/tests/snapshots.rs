use std::fs;

use anyhow::Result;
use serde_json::Value;
use solana_dex_parser::{DexParser, SolanaTransaction};

#[test]
fn sample_transaction_matches_expected() -> Result<()> {
    let tx_data = fs::read_to_string("tests/fixtures/sample_tx.json")?;
    let expected_data = fs::read_to_string("tests/expected/sample_all.json")?;

    let tx: SolanaTransaction = serde_json::from_str(&tx_data)?;
    let parser = DexParser::new();
    let result = parser.parse_all(tx, None);

    let actual: Value = serde_json::to_value(result)?;
    let expected: Value = serde_json::from_str(&expected_data)?;

    assert_eq!(actual, expected);

    Ok(())
}
