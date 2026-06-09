use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Global configuration loaded from `fijit.toml` or `~/.config/fijit/config.toml`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    /// Path to the Obscura binary. Defaults to `/tmp/obscura`.
    pub obscura_path: Option<String>,
    /// Global Slack webhook URL. Supports `${ENV_VAR}` interpolation.
    pub slack_webhook: Option<String>,
    /// Global template variables available in all scraper message templates.
    #[serde(default)]
    pub vars: HashMap<String, String>,
    /// Per-scraper overrides keyed by scraper name.
    #[serde(default)]
    pub scrapers: HashMap<String, ScraperConfig>,
}

/// Per-scraper overrides that can appear under `[scrapers.<name>]` in `fijit.toml`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ScraperConfig {
    /// Cron expression, e.g. `"*/30 * * * *"`
    pub schedule: Option<String>,
    /// Scraper-specific overrides (e.g. target URL)
    #[serde(flatten)]
    pub extra: HashMap<String, toml::Value>,
}

impl Config {
    /// Load config from `./fijit.toml` or `~/.config/fijit/config.toml`.
    ///
    /// # Errors
    /// Returns an error if no config file is found or if parsing fails.
    pub fn load() -> Result<Self> {
        let path = Self::find_path()?;
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| Error::Config(format!("reading {}: {e}", path.display())))?;
        toml::from_str(&raw).map_err(|e| Error::Config(format!("parsing {}: {e}", path.display())))
    }

    /// Like `load`, but returns an empty default if no config file is found.
    #[must_use]
    pub fn load_or_default() -> Self {
        Self::load().unwrap_or_else(|_| Config {
            obscura_path: None,
            slack_webhook: None,
            vars: HashMap::new(),
            scrapers: HashMap::new(),
        })
    }

    /// Resolve config path: `./fijit.toml` → `~/.config/fijit/config.toml`.
    ///
    /// # Errors
    /// Returns an error if neither path exists.
    pub fn find_path() -> Result<PathBuf> {
        let local = PathBuf::from("fijit.toml");
        if local.exists() {
            return Ok(local);
        }
        let global = global_config_dir().join("config.toml");
        if global.exists() {
            return Ok(global);
        }
        Err(Error::Config(format!(
            "no config found — create ./fijit.toml or {}",
            global.display()
        )))
    }

    /// Write the current config to `./fijit.toml`.
    ///
    /// # Errors
    /// Returns an error if serialisation or the file write fails.
    pub fn save_to_local(&self) -> Result<()> {
        let s = toml::to_string_pretty(self)
            .map_err(|e| Error::Config(format!("serialising config: {e}")))?;
        std::fs::write("fijit.toml", s)
            .map_err(|e| Error::Config(format!("writing fijit.toml: {e}")))
    }

    /// Return the Obscura binary path, defaulting to `/tmp/obscura`.
    #[must_use]
    pub fn obscura(&self) -> &str {
        self.obscura_path.as_deref().unwrap_or("/tmp/obscura")
    }
}

fn global_config_dir() -> PathBuf {
    std::env::var("HOME").map_or_else(
        |_| PathBuf::from(".config/fijit"),
        |h| PathBuf::from(h).join(".config").join("fijit"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp_config(contents: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(f, "{contents}").unwrap();
        f
    }

    #[test]
    fn parses_valid_config() {
        let f = write_temp_config(
            r#"
            obscura_path = "/usr/bin/obscura"
            slack_webhook = "https://hooks.slack.com/test"

            [scrapers.bike-discount]
            schedule = "*/30 * * * *"
            "#,
        );
        let raw = std::fs::read_to_string(f.path()).unwrap();
        let config: Config = toml::from_str(&raw).unwrap();
        assert_eq!(config.obscura(), "/usr/bin/obscura");
        assert_eq!(
            config.slack_webhook.as_deref(),
            Some("https://hooks.slack.com/test")
        );
        assert_eq!(
            config.scrapers["bike-discount"].schedule.as_deref(),
            Some("*/30 * * * *")
        );
    }

    #[test]
    fn defaults_obscura_path() {
        let config = Config {
            obscura_path: None,
            slack_webhook: None,
            vars: HashMap::new(),
            scrapers: HashMap::new(),
        };
        assert_eq!(config.obscura(), "/tmp/obscura");
    }

    #[test]
    fn load_or_default_does_not_panic_without_file() {
        // Change to a temp dir with no fijit.toml
        let dir = tempfile::tempdir().unwrap();
        let orig = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        let config = Config::load_or_default();
        std::env::set_current_dir(orig).unwrap();
        assert!(config.scrapers.is_empty());
    }
}
