use anyhow::{bail, Context, Result};
use std::process::Command;

pub struct ObscuraRunner {
    pub binary: String,
}

impl ObscuraRunner {
    pub fn new(binary: impl Into<String>) -> Self {
        Self { binary: binary.into() }
    }

    /// Fetch a page with stealth mode, evaluate JS, return the output.
    pub fn eval(&self, url: &str, script: &str, wait_secs: u64) -> Result<String> {
        let output = Command::new(&self.binary)
            .args([
                "fetch", url,
                "--stealth",
                "--wait", &wait_secs.to_string(),
                "--eval", script,
                "--quiet",
            ])
            .output()
            .with_context(|| format!("running obscura at {}", self.binary))?;

        if !output.status.success() {
            bail!(
                "obscura exited {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).chars().take(300).collect::<String>()
            );
        }

        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// Extract the first line of output that starts with `[` or `{` (JSON).
    pub fn eval_json(&self, url: &str, script: &str, wait_secs: u64) -> Result<String> {
        let raw = self.eval(url, script, wait_secs)?;
        for line in raw.lines() {
            let t = line.trim();
            if t.starts_with('[') || t.starts_with('{') {
                return Ok(t.to_owned());
            }
        }
        bail!("no JSON found in obscura output:\n{}", &raw[..raw.len().min(400)])
    }
}
