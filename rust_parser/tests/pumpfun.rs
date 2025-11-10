use std::fs;

use anyhow::Result;
use serde_json::to_string_pretty;
use solana_dex_parser::types::TradeType;
use solana_dex_parser::{DexParser, SolanaTransaction};

#[path = "common/mod.rs"]
mod rpc_helpers;

use rpc_helpers::fetch_transaction_with_fallback;

const PUMP_FUN_PROGRAM: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
const SOL_MINT: &str = "So11111111111111111111111111111111111111112";
const TEST_MINT: &str = "4wBqpZM9xaSheZzJSMawUKKwhdpChKbZ5eu5ky4Vigw";
const TEST_USER: &str = "5Pk716N113awdSaUDZEPZVi9Zs6hJmG5KCJtp5qQK3LB";
const DEFAULT_PUMPFUN_SIGNATURE: &str =
    "b15toBqDHKvVy7KQeAMDiEfinqg4Y8tDorUNHBd4FVojvqGyvZMELVkAz5BrNrc9AiA1zvRAZ9FfWM7qjWUQW9u";

fn approx_eq(actual: f64, expected: f64) {
    let diff = (actual - expected).abs();
    assert!(diff < 1e-6, "expected {expected}, got {actual}");
}

#[test]
fn pumpfun_buy_trade_is_parsed() -> Result<()> {
    let tx_data = fs::read_to_string("tests/fixtures/pumpfun_trade.json")?;
    let tx: SolanaTransaction = serde_json::from_str(&tx_data)?;

    let parser = DexParser::new();
    let result = parser.parse_all(tx, None);
    println!("{}", to_string_pretty(&result)?);

    assert_eq!(result.signature, "pumpfun-signature");
    assert_eq!(result.trades.len(), 1);
    assert!(result.aggregate_trade.is_some(), "aggregate trade missing");

    let trade = &result.trades[0];
    assert_eq!(trade.trade_type, TradeType::Buy);
    assert_eq!(trade.program_id.as_deref(), Some(PUMP_FUN_PROGRAM));
    assert_eq!(trade.amm.as_deref(), Some("Pumpfun"));
    assert_eq!(trade.user.as_deref(), Some(TEST_USER));
    assert_eq!(trade.input_token.mint, SOL_MINT);
    approx_eq(trade.input_token.amount, 0.5);
    assert_eq!(trade.output_token.mint, TEST_MINT);
    approx_eq(trade.output_token.amount, 12_345.6);
    assert_eq!(trade.signature, "pumpfun-signature");
    assert_eq!(trade.idx, "0-0");

    assert_eq!(result.meme_events.len(), 1);
    let event = &result.meme_events[0];
    assert_eq!(event.event_type, TradeType::Buy);
    assert_eq!(event.user, TEST_USER);
    assert_eq!(event.base_mint, TEST_MINT);
    assert_eq!(event.quote_mint, SOL_MINT);
    let input = event
        .input_token
        .as_ref()
        .expect("pumpfun trade input token");
    approx_eq(input.amount, 0.5);
    let output = event
        .output_token
        .as_ref()
        .expect("pumpfun trade output token");
    approx_eq(output.amount, 12_345.6);

    Ok(())
}

#[test]
#[ignore]
fn pumpfun_real_transaction_is_parsed() -> Result<()> {
    let rpc_url = std::env::var("SOLANA_RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let signature = std::env::var("SOLANA_PUMPFUN_SIGNATURE")
        .unwrap_or_else(|_| DEFAULT_PUMPFUN_SIGNATURE.to_string());

    let tx = fetch_transaction_with_fallback(&rpc_url, Some(&signature))?;
    let parser = DexParser::new();
    let result = parser.parse_all(tx, None);
    println!("{}", to_string_pretty(&result)?);

    assert_eq!(result.signature, signature);
    assert!(
        result
            .trades
            .iter()
            .any(|trade| trade.program_id.as_deref() == Some(PUMP_FUN_PROGRAM)),
        "expected at least one Pumpfun trade"
    );
    assert!(
        result
            .meme_events
            .iter()
            .any(|event| event.protocol.as_deref() == Some("Pumpfun")),
        "expected at least one Pumpfun meme event"
    );

    Ok(())
}
