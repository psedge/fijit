use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub obscura_path: Option<String>,
    pub slack_webhook: Option<String>,

    #[serde(default)]
    pub scrapers: HashMap<String, ScraperConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ScraperConfig {
    /// Cron expression, e.g. "*/30 * * * *"
    pub schedule: Option<String>,
    /// Extra scraper-specific keys, e.g. target URL overrides
    #[serde(flatten)]
    pub extra: HashMap<String, toml::Value>,
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = Self::find_path()?;
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("reading config at {}", path.display()))?;
        toml::from_str(&raw).with_context(|| format!("parsing config at {}", path.display()))
    }

    pub fn load_or_default() -> Self {
        Self::load().unwrap_or_else(|_| Config {
            obscura_path: None,
            slack_webhook: None,
            scrapers: HashMap::new(),
        })
    }

    /// ./fijjit.toml → ~/.config/fijjit/config.toml
    pub fn find_path() -> Result<PathBuf> {
        let local = PathBuf::from("fijjit.toml");
        if local.exists() {
            return Ok(local);
        }
        let global = dirs_path().join("config.toml");
        if global.exists() {
            return Ok(global);
        }
        anyhow::bail!(
            "no config found — create ./fijjit.toml or {}",
            global.display()
        )
    }

    pub fn save_to_local(&self) -> Result<()> {
        let s = toml::to_string_pretty(self).context("serialising config")?;
        std::fs::write("fijjit.toml", s).context("writing fijjit.toml")
    }

    pub fn obscura(&self) -> &str {
        self.obscura_path.as_deref().unwrap_or("/tmp/obscura")
    }
}

fn dirs_path() -> PathBuf {
    std::env::var("HOME")
        .map(|h| PathBuf::from(h).join(".config").join("fijjit"))
        .unwrap_or_else(|_| PathBuf::from(".config/fijjit"))
}
