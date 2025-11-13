pub mod program_ids {
    pub const METEORA: &str = "LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo";
    pub const METEORA_DAMM: &str = "Eo7WjKq67rjJQSZxS6z3YkapzY3eMj6Xy8X5EQVn5UaB";
    pub const METEORA_DAMM_V2: &str = "cpamdpZCGKUy5JxQXB4dcpGPiikHawvSWAd6mEn1sGG";
    pub const METEORA_DBC: &str = "dbcij3LWUppWqq96dh6gJWwBifmcGfLSB5D4DuSMaqN";
}

pub mod program_names {
    pub const METEORA: &str = "MeteoraDLMM";
    pub const METEORA_DAMM: &str = "MeteoraDamm";
    pub const METEORA_DAMM_V2: &str = "MeteoraDammV2";
    pub const METEORA_DBC: &str = "MeteoraDBC";
}

pub mod discriminators {
    // METEORA_DLMM liquidity discriminators (8 bytes)
    pub mod meteora_dlmm {
        pub mod swap {
            pub const SWAP: [u8; 8] = [248, 198, 158, 145, 225, 117, 135, 200]; // swap (same as METEORA_DBC.SWAP)
            pub const SWAP_V2: [u8; 8] = [65, 75, 63, 76, 235, 91, 91, 136]; // swapV2 (same as METEORA_DBC.SWAP_V2)
        }

        pub mod add_liquidity {
            pub const ADD_LIQUIDITY: [u8; 8] = [181, 157, 89, 67, 143, 182, 52, 72];
            pub const ADD_LIQUIDITY_BY_STRATEGY: [u8; 8] = [7, 3, 150, 127, 148, 40, 61, 200];
            pub const ADD_LIQUIDITY_BY_STRATEGY2: [u8; 8] = [3, 221, 149, 218, 111, 141, 118, 213];
            pub const ADD_LIQUIDITY_BY_STRATEGY_ONE_SIDE: [u8; 8] = [41, 5, 238, 175, 100, 225, 6, 205];
            pub const ADD_LIQUIDITY_ONE_SIDE: [u8; 8] = [94, 155, 103, 151, 70, 95, 220, 165];
            pub const ADD_LIQUIDITY_ONE_SIDE_PRECISE: [u8; 8] = [161, 194, 103, 84, 171, 71, 250, 154];
            pub const ADD_LIQUIDITY_BY_WEIGHT: [u8; 8] = [28, 140, 238, 99, 231, 162, 21, 149];
        }

        pub mod remove_liquidity {
            pub const REMOVE_LIQUIDITY: [u8; 8] = [80, 85, 209, 72, 24, 206, 177, 108];
            pub const REMOVE_LIQUIDITY_BY_RANGE: [u8; 8] = [26, 82, 102, 152, 240, 74, 105, 26];
            pub const REMOVE_LIQUIDITY_BY_RANGE2: [u8; 8] = [204, 2, 195, 145, 53, 145, 145, 205];
            pub const REMOVE_ALL_LIQUIDITY: [u8; 8] = [10, 51, 61, 35, 112, 105, 24, 85];
            pub const CLAIM_FEE: [u8; 8] = [169, 32, 79, 137, 136, 232, 70, 137];
            pub const CLAIM_FEE_V2: [u8; 8] = [112, 191, 101, 171, 28, 144, 127, 187];
        }
    }

    // METEORA_DAMM liquidity discriminators (8 bytes)
    pub mod meteora_damm {
        pub const CREATE: [u8; 8] = [7, 166, 138, 171, 206, 171, 236, 244];
        pub const ADD_LIQUIDITY: [u8; 8] = [168, 227, 50, 62, 189, 171, 84, 176];
        pub const REMOVE_LIQUIDITY: [u8; 8] = [133, 109, 44, 179, 56, 238, 114, 33];
        pub const ADD_IMBALANCE_LIQUIDITY: [u8; 8] = [79, 35, 122, 84, 173, 15, 93, 191];
    }

    // METEORA_DAMM_V2 liquidity discriminators (8 bytes)
    pub mod meteora_damm_v2 {
        pub const INITIALIZE_POOL: [u8; 8] = [95, 180, 10, 172, 84, 174, 232, 40];
        pub const INITIALIZE_CUSTOM_POOL: [u8; 8] = [20, 161, 241, 24, 189, 221, 180, 2];
        pub const INITIALIZE_POOL_WITH_DYNAMIC_CONFIG: [u8; 8] = [149, 82, 72, 197, 253, 252, 68, 15];
        pub const ADD_LIQUIDITY: [u8; 8] = [181, 157, 89, 67, 143, 182, 52, 72];
        pub const CLAIM_POSITION_FEE: [u8; 8] = [180, 38, 154, 17, 133, 33, 162, 211];
        pub const REMOVE_LIQUIDITY: [u8; 8] = [80, 85, 209, 72, 24, 206, 177, 108];
        pub const REMOVE_ALL_LIQUIDITY: [u8; 8] = [10, 51, 61, 35, 112, 105, 24, 85];
    }

    // u64 константы для быстрого сравнения дискриминаторов (8 bytes)
    pub mod meteora_dlmm_u64 {
        use super::meteora_dlmm;
        pub const SWAP_U64: u64 = u64::from_le_bytes(meteora_dlmm::swap::SWAP);
        pub const SWAP_V2_U64: u64 = u64::from_le_bytes(meteora_dlmm::swap::SWAP_V2);
        pub const ADD_LIQUIDITY_U64: u64 = u64::from_le_bytes(meteora_dlmm::add_liquidity::ADD_LIQUIDITY);
        pub const ADD_LIQUIDITY_BY_STRATEGY_U64: u64 = u64::from_le_bytes(meteora_dlmm::add_liquidity::ADD_LIQUIDITY_BY_STRATEGY);
        pub const ADD_LIQUIDITY_BY_STRATEGY2_U64: u64 = u64::from_le_bytes(meteora_dlmm::add_liquidity::ADD_LIQUIDITY_BY_STRATEGY2);
        pub const ADD_LIQUIDITY_BY_STRATEGY_ONE_SIDE_U64: u64 = u64::from_le_bytes(meteora_dlmm::add_liquidity::ADD_LIQUIDITY_BY_STRATEGY_ONE_SIDE);
        pub const ADD_LIQUIDITY_ONE_SIDE_U64: u64 = u64::from_le_bytes(meteora_dlmm::add_liquidity::ADD_LIQUIDITY_ONE_SIDE);
        pub const ADD_LIQUIDITY_ONE_SIDE_PRECISE_U64: u64 = u64::from_le_bytes(meteora_dlmm::add_liquidity::ADD_LIQUIDITY_ONE_SIDE_PRECISE);
        pub const ADD_LIQUIDITY_BY_WEIGHT_U64: u64 = u64::from_le_bytes(meteora_dlmm::add_liquidity::ADD_LIQUIDITY_BY_WEIGHT);
        pub const REMOVE_LIQUIDITY_U64: u64 = u64::from_le_bytes(meteora_dlmm::remove_liquidity::REMOVE_LIQUIDITY);
        pub const REMOVE_LIQUIDITY_BY_RANGE_U64: u64 = u64::from_le_bytes(meteora_dlmm::remove_liquidity::REMOVE_LIQUIDITY_BY_RANGE);
        pub const REMOVE_LIQUIDITY_BY_RANGE2_U64: u64 = u64::from_le_bytes(meteora_dlmm::remove_liquidity::REMOVE_LIQUIDITY_BY_RANGE2);
        pub const REMOVE_ALL_LIQUIDITY_U64: u64 = u64::from_le_bytes(meteora_dlmm::remove_liquidity::REMOVE_ALL_LIQUIDITY);
        pub const CLAIM_FEE_U64: u64 = u64::from_le_bytes(meteora_dlmm::remove_liquidity::CLAIM_FEE);
        pub const CLAIM_FEE_V2_U64: u64 = u64::from_le_bytes(meteora_dlmm::remove_liquidity::CLAIM_FEE_V2);
    }

    pub mod meteora_damm_u64 {
        use super::meteora_damm;
        pub const CREATE_U64: u64 = u64::from_le_bytes(meteora_damm::CREATE);
        pub const ADD_LIQUIDITY_U64: u64 = u64::from_le_bytes(meteora_damm::ADD_LIQUIDITY);
        pub const REMOVE_LIQUIDITY_U64: u64 = u64::from_le_bytes(meteora_damm::REMOVE_LIQUIDITY);
        pub const ADD_IMBALANCE_LIQUIDITY_U64: u64 = u64::from_le_bytes(meteora_damm::ADD_IMBALANCE_LIQUIDITY);
    }

    pub mod meteora_damm_v2_u64 {
        use super::meteora_damm_v2;
        pub const INITIALIZE_POOL_U64: u64 = u64::from_le_bytes(meteora_damm_v2::INITIALIZE_POOL);
        pub const INITIALIZE_CUSTOM_POOL_U64: u64 = u64::from_le_bytes(meteora_damm_v2::INITIALIZE_CUSTOM_POOL);
        pub const INITIALIZE_POOL_WITH_DYNAMIC_CONFIG_U64: u64 = u64::from_le_bytes(meteora_damm_v2::INITIALIZE_POOL_WITH_DYNAMIC_CONFIG);
        pub const ADD_LIQUIDITY_U64: u64 = u64::from_le_bytes(meteora_damm_v2::ADD_LIQUIDITY);
        pub const CLAIM_POSITION_FEE_U64: u64 = u64::from_le_bytes(meteora_damm_v2::CLAIM_POSITION_FEE);
        pub const REMOVE_LIQUIDITY_U64: u64 = u64::from_le_bytes(meteora_damm_v2::REMOVE_LIQUIDITY);
        pub const REMOVE_ALL_LIQUIDITY_U64: u64 = u64::from_le_bytes(meteora_damm_v2::REMOVE_ALL_LIQUIDITY);
    }

    // METEORA_DBC discriminators (8 bytes)
    pub mod meteora_dbc {
        pub const SWAP: [u8; 8] = [248, 198, 158, 145, 225, 117, 135, 200];
        pub const SWAP_V2: [u8; 8] = [65, 75, 63, 76, 235, 91, 91, 136];
        pub const INITIALIZE_VIRTUAL_POOL_WITH_SPL_TOKEN: [u8; 8] = [140, 85, 215, 176, 102, 54, 104, 79];
        pub const INITIALIZE_VIRTUAL_POOL_WITH_TOKEN2022: [u8; 8] = [169, 118, 51, 78, 145, 110, 220, 155];
        pub const METEORA_DBC_MIGRATE_DAMM: [u8; 8] = [27, 1, 48, 22, 180, 63, 118, 217];
        pub const METEORA_DBC_MIGRATE_DAMM_V2: [u8; 8] = [156, 169, 230, 103, 53, 228, 80, 64];
    }

    pub mod meteora_dbc_u64 {
        use super::meteora_dbc;
        pub const SWAP_U64: u64 = u64::from_le_bytes(meteora_dbc::SWAP);
        pub const SWAP_V2_U64: u64 = u64::from_le_bytes(meteora_dbc::SWAP_V2);
        pub const INITIALIZE_VIRTUAL_POOL_WITH_SPL_TOKEN_U64: u64 = u64::from_le_bytes(meteora_dbc::INITIALIZE_VIRTUAL_POOL_WITH_SPL_TOKEN);
        pub const INITIALIZE_VIRTUAL_POOL_WITH_TOKEN2022_U64: u64 = u64::from_le_bytes(meteora_dbc::INITIALIZE_VIRTUAL_POOL_WITH_TOKEN2022);
        pub const METEORA_DBC_MIGRATE_DAMM_U64: u64 = u64::from_le_bytes(meteora_dbc::METEORA_DBC_MIGRATE_DAMM);
        pub const METEORA_DBC_MIGRATE_DAMM_V2_U64: u64 = u64::from_le_bytes(meteora_dbc::METEORA_DBC_MIGRATE_DAMM_V2);
    }

    // METEORA_DAMM_V2 event discriminators (16 bytes)
    pub mod meteora_damm_v2_events {
        pub const CREATE_POSITION_EVENT: [u8; 16] = [
            228, 69, 165, 46, 81, 203, 154, 29, 156, 15, 119, 198, 29, 181, 221, 55,
        ];
    }

    pub mod meteora_damm_v2_events_u128 {
        use super::meteora_damm_v2_events;
        pub const CREATE_POSITION_EVENT_U128: u128 = u128::from_le_bytes(meteora_damm_v2_events::CREATE_POSITION_EVENT);
    }
}

