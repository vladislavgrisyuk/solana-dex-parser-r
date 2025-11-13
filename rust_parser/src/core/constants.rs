pub mod dex_programs {
    pub const JUPITER: &str = "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4";
    pub const RAYDIUM: &str = "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8";
    pub const PUMP_FUN: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
    pub const PUMP_SWAP: &str = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA";
    pub const ORCA: &str = "whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc";
    pub const METEORA: &str = "LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo";
    pub const METEORA_DAMM: &str = "Eo7WjKq67rjJQSZxS6z3YkapzY3eMj6Xy8X5EQVn5UaB";
    pub const METEORA_DAMM_V2: &str = "cpamdpZCGKUy5JxQXB4dcpGPiikHawvSWAd6mEn1sGG";
    pub const METEORA_DBC: &str = "dbcij3LWUppWqq96dh6gJWwBifmcGfLSB5D4DuSMaqN";
    pub const UNKNOWN: &str = "UNKNOWN";
}

pub mod dex_program_names {
    use super::dex_programs;
    use once_cell::sync::Lazy;
    use std::collections::HashMap;

    static PROGRAM_NAME: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
        let mut map = HashMap::new();
        map.insert(dex_programs::JUPITER, "Jupiter");
        map.insert(dex_programs::RAYDIUM, "Raydium");
        map.insert(dex_programs::PUMP_FUN, "Pumpfun");
        map.insert(dex_programs::PUMP_SWAP, "Pumpswap");
        map.insert(dex_programs::ORCA, "Orca");
        map.insert(dex_programs::METEORA, "MeteoraDLMM");
        map.insert(dex_programs::METEORA_DAMM, "MeteoraDamm");
        map.insert(dex_programs::METEORA_DAMM_V2, "MeteoraDammV2");
        map.insert(dex_programs::METEORA_DBC, "MeteoraDBC");
        map
    });

    pub fn name(program_id: &str) -> &'static str {
        PROGRAM_NAME
            .get(program_id)
            .copied()
            .unwrap_or("Unknown DEX")
    }
}

pub const SYSTEM_PROGRAMS: &[&str] = &[
    "ComputeBudget111111111111111111111111111111",
    "11111111111111111111111111111111",
    "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
    "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb",
    "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL",
    "srmqPvymJeFKQ4zGQed1GFppgkRHL9kaELCbyksJtPX", // openbook
];

pub const SKIP_PROGRAM_IDS: &[&str] = &[
    "pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ", // Pumpswap Fee
];

#[allow(non_snake_case)]
pub struct Tokens {
    pub SOL: &'static str,
    pub USDC: &'static str,
    pub USDT: &'static str,
}

impl Tokens {
    pub fn values(&self) -> Vec<&'static str> {
        vec![self.SOL, self.USDC, self.USDT]
    }
}

pub const TOKENS: Tokens = Tokens {
    SOL: "So11111111111111111111111111111111111111112",
    USDC: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
    USDT: "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB",
};
