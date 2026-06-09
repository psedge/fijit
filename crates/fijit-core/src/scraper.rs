/// What a scraper found on a given run.
pub enum ScrapeResult {
    /// Nothing worth notifying about.
    NoChange,
    /// Something the user should know — send this to Slack.
    Alert(String),
    /// Multiple independent alerts (e.g. several items changed).
    Alerts(Vec<String>),
}

/// Implement this for every scraper in the `scrapers/` crates.
pub trait Scraper: Send + Sync {
    /// Short unique identifier used in CLI commands, e.g. `"bike-discount"`.
    fn name(&self) -> &str;

    /// Human-readable description shown in `fijit list`.
    fn description(&self) -> &str;

    /// Run one check and return what changed (if anything).
    ///
    /// Uses `anyhow::Result` so scrapers can use `?` with any error type.
    ///
    /// # Errors
    /// Returns an error if the scraper encounters a fatal problem (network, parse, etc.).
    fn check(&self) -> anyhow::Result<ScrapeResult>;

    /// Per-scraper Slack webhook, overriding the global config value.
    /// Return `None` to fall back to the global webhook.
    fn slack_webhook(&self) -> Option<String> {
        None
    }

    /// Default cron schedule, shown in `fijit list`.
    fn schedule(&self) -> Option<&str> {
        None
    }
}
