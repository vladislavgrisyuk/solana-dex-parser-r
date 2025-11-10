use thiserror::Error;

#[derive(Debug, Error)]
pub enum ParserError {
    #[error("transaction parsing failed: {0}")]
    Generic(String),
}

impl ParserError {
    pub fn generic(message: impl Into<String>) -> Self {
        Self::Generic(message.into())
    }
}
