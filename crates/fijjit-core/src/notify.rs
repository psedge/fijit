use crate::error::{Error, Result};
use serde_json::json;

/// Send a Slack message via incoming webhook.
pub fn slack(webhook: &str, message: &str) -> Result<()> {
    let resp = reqwest::blocking::Client::new()
        .post(webhook)
        .json(&json!({ "text": message }))
        .send()
        .map_err(Error::Http)?;

    if !resp.status().is_success() {
        return Err(Error::Notify(format!(
            "Slack returned HTTP {}",
            resp.status()
        )));
    }
    Ok(())
}

/// Send a Slack message if a webhook is configured; log to stderr on failure.
pub fn slack_if_configured(webhook: Option<&str>, message: &str) {
    match webhook {
        Some(url) => {
            if let Err(e) = slack(url, message) {
                eprintln!("Slack notification failed: {e}");
            }
        }
        None => eprintln!("No Slack webhook configured — skipping notification"),
    }
}
