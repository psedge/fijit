use thiserror::Error;

/// All errors that can originate from `fijjit-core`.
#[derive(Debug, Error)]
pub enum Error {
    /// The Obscura process exited with a non-zero status or could not be spawned.
    #[error("obscura process failed: {0}")]
    Obscura(String),
    /// Obscura produced output but no JSON line (`[` / `{`) was found.
    #[error("no JSON found in obscura output: {0}")]
    NoJson(String),
    /// The JSON returned by Obscura could not be deserialised into the expected type.
    #[error("failed to parse scraper response: {0}")]
    Parse(#[from] serde_json::Error),
    /// An HTTP request made by a scraper failed.
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    /// A configuration problem (missing file, parse error, invalid value).
    #[error("config error: {0}")]
    Config(String),
    /// A Slack (or other) notification could not be delivered.
    #[error("notification failed: {0}")]
    Notify(String),
    /// An I/O error from the standard library.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Convenience `Result` alias using [`Error`].
pub type Result<T> = std::result::Result<T, Error>;
