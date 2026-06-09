use serde::{Deserialize, Serialize};

/// A single declarative step in a scraper pipeline.
///
/// Which fields are required depends on `action` — see each variant's documentation.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Step {
    /// What this step does.
    pub action: Action,
    /// CSS selector (required by `query_all`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
    /// JavaScript expression (required by `eval_json`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
    /// Element field to inspect (`"text"`, `"class"`, `"href"`, `"value"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    /// Comparison operator (required by `filter`, `alert_if`, `alert_if_any`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub op: Option<Op>,
    /// The value to compare against, or a literal value for `set`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Variable name to write to (used by `set` and `map`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub var: Option<String>,
    /// Slack message template (required by `alert_*` actions).
    /// Supports `{url}`, `{text}`, `{class}`, `{href}`, `{value}`, and custom `[vars]`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Seconds to wait after page load before evaluating (default: 3).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wait: Option<u64>,
}

/// The operation a pipeline step performs.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Fetch the scraper URL and query all elements matching a CSS `selector`.
    /// Replaces the current element list with the matches.
    QueryAll,
    /// Fetch the scraper URL, evaluate a JS `script`, and parse the JSON result.
    /// Replaces the current element list with the parsed elements.
    EvalJson,
    /// Keep only the first element whose `field` equals `value`.
    Find,
    /// Keep all elements whose `field` satisfies `op` against `value`.
    Filter,
    /// Store a literal `value` into a named `var`.
    Set,
    /// Collect `field` from all elements and join them into a named `var`.
    Map,
    /// Emit an alert if the first matching element's `field` satisfies `op` against `value`.
    AlertIf,
    /// Emit an alert if the current element list is empty.
    AlertIfEmpty,
    /// Emit one alert per element whose `field` satisfies `op` against `value`.
    AlertIfAny,
    /// Print the current element list to stdout.
    Log,
}

/// A binary comparison operator.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    /// Exact equality.
    Eq,
    /// Not equal.
    NotEq,
    /// Field contains the value as a substring.
    Contains,
    /// Field does not contain the value as a substring.
    NotContains,
    /// Field matches the value as a regular expression.
    Matches,
}
