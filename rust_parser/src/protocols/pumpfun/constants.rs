pub const PUMP_FUN_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
pub const PUMP_FUN_PROGRAM_NAME: &str = "Pumpfun";

pub const PUMP_SWAP_PROGRAM_ID: &str = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA";
pub const PUMP_SWAP_PROGRAM_NAME: &str = "Pumpswap";

pub const SOL_MINT: &str = "So11111111111111111111111111111111111111112";

pub mod discriminators {
    pub mod pumpfun_instructions {
        pub const CREATE: [u8; 8] = [24, 30, 200, 40, 5, 28, 7, 119];
        pub const MIGRATE: [u8; 8] = [155, 234, 231, 146, 236, 158, 162, 30];
        pub const BUY: [u8; 8] = [102, 6, 61, 18, 1, 218, 235, 234];
        pub const SELL: [u8; 8] = [51, 230, 133, 164, 1, 127, 131, 173];
    }

    pub mod pumpfun_events {
        pub const TRADE: [u8; 16] = [
            228, 69, 165, 46, 81, 203, 154, 29, 189, 219, 127, 211, 78, 230, 97, 238,
        ];
        pub const CREATE: [u8; 16] = [
            228, 69, 165, 46, 81, 203, 154, 29, 27, 114, 169, 77, 222, 235, 99, 118,
        ];
        pub const COMPLETE: [u8; 16] = [
            228, 69, 165, 46, 81, 203, 154, 29, 95, 114, 97, 156, 212, 46, 152, 8,
        ];
        pub const MIGRATE: [u8; 16] = [
            228, 69, 165, 46, 81, 203, 154, 29, 189, 233, 93, 185, 92, 148, 234, 148,
        ];
    }

    pub mod pumpswap_instructions {
        pub const CREATE_POOL: [u8; 8] = [233, 146, 209, 142, 207, 104, 64, 188];
        pub const ADD_LIQUIDITY: [u8; 8] = [242, 35, 198, 137, 82, 225, 242, 182];
        pub const REMOVE_LIQUIDITY: [u8; 8] = [183, 18, 70, 156, 148, 109, 161, 34];
        pub const BUY: [u8; 8] = [102, 6, 61, 18, 1, 218, 235, 234];
        pub const SELL: [u8; 8] = [51, 230, 133, 164, 1, 127, 131, 173];
    }

    pub mod pumpswap_events {
        pub const CREATE_POOL: [u8; 16] = [
            228, 69, 165, 46, 81, 203, 154, 29, 177, 49, 12, 210, 160, 118, 167, 116,
        ];
        pub const ADD_LIQUIDITY: [u8; 16] = [
            228, 69, 165, 46, 81, 203, 154, 29, 120, 248, 61, 83, 31, 142, 107, 144,
        ];
        pub const REMOVE_LIQUIDITY: [u8; 16] = [
            228, 69, 165, 46, 81, 203, 154, 29, 22, 9, 133, 26, 160, 44, 71, 192,
        ];
        pub const BUY: [u8; 16] = [
            228, 69, 165, 46, 81, 203, 154, 29, 103, 244, 82, 31, 44, 245, 119, 119,
        ];
        pub const SELL: [u8; 16] = [
            228, 69, 165, 46, 81, 203, 154, 29, 62, 47, 55, 10, 165, 3, 220, 42,
        ];
    }
}
