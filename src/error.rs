use thiserror::Error;

#[derive(Debug, Error)]
pub enum KslError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Rate limited: {reason}")]
    RateLimited { reason: String },

    #[error("Daily request cap exceeded ({cap} requests)")]
    DailyCapExceeded { cap: u32 },

    #[error("Parse error: {context}")]
    Parse { context: String },

    #[error("Config error: {0}")]
    #[allow(dead_code)]
    Config(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, KslError>;
