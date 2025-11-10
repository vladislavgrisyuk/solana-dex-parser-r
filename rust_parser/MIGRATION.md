# Migration Notes: TypeScript → Rust

This document outlines how the public API of the original TypeScript package maps to the Rust crate.

## Modules

| TypeScript | Rust |
|------------|------|
| `DexParser` class (`src/dex-parser.ts`) | `solana_dex_parser::DexParser` |
| `ParseConfig` interface | `solana_dex_parser::ParseConfig` (`config.rs`) |
| `types/*` interfaces | `solana_dex_parser::types` structs (serde renamed for JSON parity) |
| Transaction helpers (`transaction-adapter.ts`, `transaction-utils.ts`) | `core::transaction_adapter`, `core::transaction_utils` |
| Protocol specific parsers | `protocols::simple` (extensible registry in Rust) |

## API Coverage

| Feature | Status |
|---------|--------|
| `parse_all` | ✅ identical return type (`ParseResult`) |
| `parse_trades` / `parse_liquidity` / `parse_transfers` | ✅ vector outputs matching TypeScript JSON |
| Block parsing (`parseBlockRaw` / `parseBlockParsed`) | ✅ provided as `DexParser::parse_block_raw`, `parse_block_parsed`, and `parse_block` |
| Meme event parsing | ✅ through `protocols::simple::SimpleMemeParser` |

## Differences and Assumptions

- The Rust crate currently bundles simplified protocol adapters that cover the major DEX families (Jupiter/Raydium/Orca/Meteora/Pumpfun).
  Fine-grained variants from the TypeScript implementation can be added by extending the registry in `protocols/`.
- Unknown program heuristics mirror the TypeScript behaviour: controlled by `ParseConfig::try_unknown_dex`.
- Transaction and block inputs must follow the same JSON layout that the TypeScript library expects. Deserialisation failures are
  reported as `ParserError::Generic`.

## CLI

The TypeScript repository exposed examples through scripts. The Rust port bundles an optional `dexp` binary (feature `cli`) with
`parse-tx` and `parse-block` subcommands that return JSON payloads identical to the library output.

## Error Handling

Instead of throwing JavaScript errors, the Rust version uses `thiserror` (`ParserError`) and returns a populated `ParseResult`
with `state = false` when `ParseConfig::throw_error` is enabled.
