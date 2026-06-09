use crate::error::{Error, Result};
use std::process::Command;

pub struct ObscuraRunner {
    pub binary: String,
}

impl ObscuraRunner {
    pub fn new(binary: impl Into<String>) -> Self {
        Self {
            binary: binary.into(),
        }
    }

    /// Fetch a page with stealth mode, evaluate JS, and return stdout.
    pub fn eval(&self, url: &str, script: &str, wait_secs: u64) -> Result<String> {
        let output = Command::new(&self.binary)
            .args([
                "fetch",
                url,
                "--stealth",
                "--wait",
                &wait_secs.to_string(),
                "--eval",
                script,
                "--quiet",
            ])
            .output()
            .map_err(|e| Error::Obscura(format!("failed to run {}: {e}", self.binary)))?;

        if !output.status.success() {
            return Err(Error::Obscura(format!(
                "exited {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
                    .chars()
                    .take(300)
                    .collect::<String>()
            )));
        }

        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// Like `eval`, but extracts the first JSON line (`[` or `{`) from the output.
    pub fn eval_json(&self, url: &str, script: &str, wait_secs: u64) -> Result<String> {
        let raw = self.eval(url, script, wait_secs)?;
        extract_json(&raw).ok_or_else(|| Error::NoJson(raw.chars().take(400).collect()))
    }
}

/// Extract the first line starting with `[` or `{` from multi-line output.
fn extract_json(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let t = line.trim();
        (t.starts_with('[') || t.starts_with('{')).then(|| t.to_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_array_json_from_noisy_output() {
        let output = "log line\nwarning\n[{\"key\":\"val\"}]\ntrailing";
        assert_eq!(extract_json(output), Some(r#"[{"key":"val"}]"#.to_owned()));
    }

    #[test]
    fn extracts_object_json() {
        let output = "preamble\n{\"a\":1}";
        assert_eq!(extract_json(output), Some(r#"{"a":1}"#.to_owned()));
    }

    #[test]
    fn returns_none_when_no_json() {
        assert_eq!(extract_json("no json here\nstill nothing"), None);
    }
}
