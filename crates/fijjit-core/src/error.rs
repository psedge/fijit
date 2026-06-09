use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("obscura process failed: {0}")]
    Obscura(String),

    #[error("no JSON found in obscura output: {0}")]
    NoJson(String),

    #[error("failed to parse scraper response: {0}")]
    Parse(#[from] serde_json::Error),

    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("config error: {0}")]
    Config(String),

    #[error("notification failed: {0}")]
    Notify(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
