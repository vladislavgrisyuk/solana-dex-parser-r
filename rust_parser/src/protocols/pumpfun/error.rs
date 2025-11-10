use thiserror::Error;

use super::binary_reader::BinaryReaderError;

#[derive(Debug, Error)]
pub enum PumpfunError {
    #[error("failed to decode instruction data: {0}")]
    InstructionData(String),
    #[error("binary reader error: {0}")]
    BinaryReader(#[from] BinaryReaderError),
    #[error("missing token account info: {account}")]
    MissingAccount { account: &'static str },
    #[error("failed to deserialize value: {0}")]
    Json(#[from] serde_json::Error),
}

impl PumpfunError {
    pub fn instruction_data(message: impl Into<String>) -> Self {
        Self::InstructionData(message.into())
    }

    pub fn missing_account(account: &'static str) -> Self {
        Self::MissingAccount { account }
    }
}
