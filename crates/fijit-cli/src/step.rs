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
    /// Comparison operator (required by `filter`; optional on `find`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub op: Option<Op>,
    /// The value to compare against, or a literal value for `set`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Variable name to write to (used by `set` and `map`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub var: Option<String>,
    /// Slack message template (required by `alert`).
    /// Supports `{url}`, `{text}`, `{class}`, `{href}`, `{value}`, and custom `[vars]`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Seconds to wait after page load before evaluating (default: 3).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wait: Option<u64>,
    /// Assumed initial value for `alert` with `on = "change"` when no prior state exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    /// Trigger mode for `alert` (default: `"any"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on: Option<AlertTrigger>,
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
    /// Keep only the first element whose `field` matches `value` (optionally via `op`).
    Find,
    /// Keep all elements whose `field` satisfies `op` against `value`.
    Filter,
    /// Store a literal `value` into a named `var`.
    Set,
    /// Collect `field` from all elements and join them into a named `var`.
    Map,
    /// Emit a Slack alert. Trigger behaviour is controlled by `on` (default: `"any"`).
    Alert,
    /// Print the current element list to stdout.
    Log,
}

/// Controls when an `alert` step fires.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AlertTrigger {
    /// Fire once when the element list is non-empty. Uses the first element's fields
    /// for template interpolation. This is the default.
    #[default]
    Any,
    /// Fire one alert per element.
    Each,
    /// Fire when the element list is empty.
    Empty,
    /// Fire when `field` of the first element changes from its previous value.
    /// State is persisted between runs. Use `default` to set the assumed initial value.
    /// No-op when the element list is empty.
    Change,
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
    /// Field starts with value.
    StartsWith,
    /// Field ends with value.
    EndsWith,
    /// Field, parsed as a number, is greater than value. Useful for price/stock
    /// thresholds. Currency symbols and thousands separators are ignored, so
    /// `"£1,299.00"` compares as `1299.0`. No match if either side isn't numeric.
    Gt,
    /// Field, parsed as a number, is less than value.
    Lt,
    /// Field, parsed as a number, is greater than or equal to value.
    Gte,
    /// Field, parsed as a number, is less than or equal to value.
    Lte,
}
