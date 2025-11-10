use serde::{Deserialize, Serialize};

/// Configuration for the parser mirroring the TypeScript structure.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ParseConfig {
    #[serde(
        default = "ParseConfig::default_try_unknown_dex",
        rename = "tryUnknowDEX"
    )]
    pub try_unknown_dex: bool,
    #[serde(default)]
    pub program_ids: Option<Vec<String>>,
    #[serde(default)]
    pub ignore_program_ids: Option<Vec<String>>,
    #[serde(default = "ParseConfig::default_throw_error")]
    pub throw_error: bool,
    #[serde(default = "ParseConfig::default_aggregate_trades")]
    pub aggregate_trades: bool,
}

impl Default for ParseConfig {
    fn default() -> Self {
        Self {
            try_unknown_dex: Self::default_try_unknown_dex(),
            program_ids: None,
            ignore_program_ids: None,
            throw_error: Self::default_throw_error(),
            aggregate_trades: Self::default_aggregate_trades(),
        }
    }
}

impl ParseConfig {
    const fn default_try_unknown_dex() -> bool {
        true
    }

    const fn default_throw_error() -> bool {
        false
    }

    const fn default_aggregate_trades() -> bool {
        true
    }
}
