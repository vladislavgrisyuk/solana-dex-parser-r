use std::fs;
use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use serde_json::Value;
use solana_dex_parser::rpc;
use solana_dex_parser::types::FromJsonValue;
use solana_dex_parser::{DexParser, ParseConfig, SolanaBlock, SolanaTransaction};

const DEFAULT_RPC_URL: &str = "https://api.mainnet-beta.solana.com";

#[derive(Parser)]
#[command(author, version, about = "Parse Solana DEX transactions", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Parse a single transaction JSON file
    ParseTx {
        /// Path to a JSON file containing a transaction
        #[arg(long)]
        file: PathBuf,
        /// Output mode
        #[arg(long, value_enum, default_value = "all")]
        mode: TxMode,
    },
    /// Parse a block JSON file
    ParseBlock {
        /// Path to a JSON file containing block information
        #[arg(long)]
        file: PathBuf,
        /// Block parsing mode
        #[arg(long, value_enum, default_value = "parsed")]
        mode: BlockMode,
    },
    /// Fetch a transaction by signature via RPC
    ParseSig {
        /// Transaction signature to fetch
        #[arg(long)]
        signature: String,
        /// RPC endpoint URL (can also be set via SOLANA_RPC_URL)
        #[arg(long, env = "SOLANA_RPC_URL", default_value = DEFAULT_RPC_URL)]
        rpc_url: String,
        /// Output mode
        #[arg(long, value_enum, default_value = "all")]
        mode: TxMode,
    },
}

#[derive(Clone, ValueEnum)]
enum TxMode {
    All,
    Trades,
    Liquidity,
    Transfers,
}

#[derive(Clone, ValueEnum)]
enum BlockMode {
    Raw,
    Parsed,
}

fn read_json(file: &PathBuf) -> Result<Value> {
    // Optimized: read as bytes and parse with from_slice (faster than from_str)
    let data = fs::read(file).with_context(|| format!("failed to read {:?}", file))?;
    serde_json::from_slice(&data).with_context(|| format!("failed to parse JSON in {:?}", file))
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let parser = DexParser::new();
    let config = ParseConfig::default();

    match cli.command {
        Commands::ParseTx { file, mode } => {
            // Optimized: read bytes and parse directly
            let data = fs::read(&file).with_context(|| format!("failed to read {:?}", file))?;
            let tx = SolanaTransaction::from_slice(&data, &config)
                .map_err(|err| anyhow!("{err}"))?;
            let output = parse_with_mode(&parser, tx, mode, &config)?;
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        Commands::ParseBlock { file, mode } => {
            // Optimized: read bytes and parse directly
            let data = fs::read(&file).with_context(|| format!("failed to read {:?}", file))?;
            match mode {
                BlockMode::Raw => {
                    // Use optimized bytes parsing
                    let result = parser.parse_block_raw_bytes(&data, Some(config))?;
                    println!("{}", serde_json::to_string_pretty(&result)?);
                }
                BlockMode::Parsed => {
                    let block: SolanaBlock = serde_json::from_slice(&data)?;
                    let result = parser.parse_block_parsed(&block, Some(config));
                    println!("{}", serde_json::to_string_pretty(&result)?);
                }
            }
        }
        Commands::ParseSig {
            signature,
            rpc_url,
            mode,
        } => {
            let tx = rpc::fetch_transaction(&rpc_url, &signature)?;
            let output = parse_with_mode(&parser, tx, mode, &config)?;
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
    }

    Ok(())
}

fn parse_with_mode(
    parser: &DexParser,
    tx: SolanaTransaction,
    mode: TxMode,
    config: &ParseConfig,
) -> Result<Value> {
    Ok(match mode {
        TxMode::All => serde_json::to_value(parser.parse_all(tx, Some(config.clone())))?,
        TxMode::Trades => serde_json::to_value(parser.parse_trades(tx, Some(config.clone())))?,
        TxMode::Liquidity => {
            serde_json::to_value(parser.parse_liquidity(tx, Some(config.clone())))?
        }
        TxMode::Transfers => {
            serde_json::to_value(parser.parse_transfers(tx, Some(config.clone())))?
        }
    })
}
