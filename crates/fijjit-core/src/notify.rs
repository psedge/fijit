use anyhow::{bail, Context, Result};
use serde_json::json;

pub fn slack(webhook: &str, message: &str) -> Result<()> {
    let resp = reqwest::blocking::Client::new()
        .post(webhook)
        .json(&json!({ "text": message }))
        .send()
        .context("sending Slack notification")?;

    if !resp.status().is_success() {
        bail!("Slack returned HTTP {}", resp.status());
    }
    Ok(())
}

pub fn slack_if_configured(webhook: Option<&str>, message: &str) {
    match webhook {
        Some(url) => {
            if let Err(e) = slack(url, message) {
                eprintln!("Slack notification failed: {e:#}");
            }
        }
        None => eprintln!("No Slack webhook configured — skipping notification"),
    }
}
