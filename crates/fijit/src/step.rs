use serde::{Deserialize, Serialize};

/// A single declarative step in a scraper pipeline.
///
/// Steps are internally tagged by `action` in TOML, and each variant carries
/// exactly the fields that action needs, so a config that omits a required
/// field (or supplies one that doesn't apply) fails at parse time rather than
/// during execution.
///
/// ```toml
/// [[steps]]
/// action = "query_all"
/// selector = ".product"
/// attrs = ["data-price"]
/// ```
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Step {
    /// Fetch the scraper URL and query all elements matching a CSS `selector`.
    /// Replaces the current element list with the matches.
    QueryAll {
        /// CSS selector to match.
        selector: String,
        /// Extra HTML attributes to capture beyond the built-in
        /// `text`/`class`/`href`/`value`, e.g. `["data-price", "aria-label"]`.
        /// Each becomes addressable by `field` and as a `{name}` template var.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        attrs: Vec<String>,
        /// Seconds to wait after page load before evaluating (default: 3).
        #[serde(skip_serializing_if = "Option::is_none")]
        wait: Option<u64>,
    },
    /// Fetch the scraper URL, evaluate a JS `script`, and parse the JSON result.
    /// Replaces the current element list with the parsed elements.
    EvalJson {
        /// JavaScript expression returning a JSON array of elements.
        script: String,
        /// Seconds to wait after page load before evaluating (default: 3).
        #[serde(skip_serializing_if = "Option::is_none")]
        wait: Option<u64>,
    },
    /// Keep only the first element whose `field` matches `value` (optionally via `op`).
    Find {
        /// Element field to inspect (a built-in or a captured attribute).
        field: String,
        /// Comparison operator. Defaults to exact equality when omitted.
        #[serde(skip_serializing_if = "Option::is_none")]
        op: Option<Op>,
        /// Value to compare against.
        value: String,
    },
    /// Keep all elements whose `field` satisfies `op` against `value`.
    Filter {
        /// Element field to inspect (a built-in or a captured attribute).
        field: String,
        /// Comparison operator.
        op: Op,
        /// Value to compare against.
        value: String,
    },
    /// Store a literal `value` into a named `var`.
    Set {
        /// Variable name to write to.
        var: String,
        /// Literal value (supports `${ENV_VAR}` interpolation).
        value: String,
    },
    /// Collect `field` from all elements and join them into a named `var`.
    Map {
        /// Element field to collect.
        field: String,
        /// Variable name to write the comma-joined result to.
        var: String,
    },
    /// Reorder the element list by `field`. Stable; ties keep their input order.
    Sort {
        /// Element field to sort by (a built-in or a captured attribute).
        field: String,
        /// Sort direction (default: `"asc"`).
        #[serde(default, skip_serializing_if = "SortOrder::is_default")]
        order: SortOrder,
        /// Compare values numerically via the same parser as the numeric ops
        /// (currency symbols and thousands separators ignored). Non-numeric
        /// values sort after numeric ones. Default: lexical comparison.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        numeric: bool,
    },
    /// Derive a new per-element `field` from a `template`, evaluated against each
    /// element's fields and the current vars. Adds (or overwrites) that field on
    /// every element. Useful for building readable labels or stable fingerprints.
    Compute {
        /// Name of the field to write on each element.
        field: String,
        /// Template string, e.g. `"{text} @ {value}"`. Supports the same
        /// `{name}` placeholders as alert messages.
        template: String,
    },
    /// For each element, fetch the URL in `field` and extract elements matching
    /// `selector` on that page. The element list is replaced by the flattened
    /// matches across every followed page, one bounded level of link-walking.
    Follow {
        /// Element field holding the link to follow (default: `"href"`).
        /// Relative links are resolved against the scraper URL.
        #[serde(default = "default_link_field")]
        field: String,
        /// CSS selector to match on each followed page.
        selector: String,
        /// Extra HTML attributes to capture, as in `query_all`.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        attrs: Vec<String>,
        /// Seconds to wait after each page load before evaluating (default: 3).
        #[serde(skip_serializing_if = "Option::is_none")]
        wait: Option<u64>,
    },
    /// Emit a Slack alert. Trigger behaviour is controlled by `on` (default: `"any"`).
    Alert {
        /// Slack message template. Supports `{url}`, `{text}`, `{class}`,
        /// `{href}`, `{value}`, captured attributes, and custom `[vars]`.
        message: String,
        /// Trigger mode (default: `"any"`).
        #[serde(default, skip_serializing_if = "AlertTrigger::is_default")]
        on: AlertTrigger,
        /// Field to watch, used by `change`/`decrease`/`increase` (the first
        /// element's value) and as the identity key for `added`/`removed`.
        #[serde(skip_serializing_if = "Option::is_none")]
        field: Option<String>,
        /// Var to watch instead of `field`, for `change`/`decrease`/`increase`.
        /// Mutually exclusive with `field`; lets these triggers track an
        /// aggregate built by `map` rather than the first element.
        #[serde(skip_serializing_if = "Option::is_none")]
        var: Option<String>,
        /// Assumed initial value for `change`/`decrease`/`increase` when no
        /// prior state exists.
        #[serde(skip_serializing_if = "Option::is_none")]
        default: Option<String>,
        /// Stable identifier for the persisted state of an `on = "change"` alert.
        /// State is stored under this id, so it survives inserting or reordering
        /// other steps. When omitted, state falls back to the step's position,
        /// which shifts if the pipeline changes, so set an `id` to avoid spurious alerts.
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    /// Print the current element list to stdout.
    Log,
}

/// Default link field for the `follow` step.
fn default_link_field() -> String {
    "href".to_owned()
}

/// Sort direction for the `sort` step.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SortOrder {
    /// Ascending (A→Z, low→high). The default.
    #[default]
    Asc,
    /// Descending (Z→A, high→low).
    Desc,
}

impl SortOrder {
    /// Whether this is the default order (`Asc`); used to skip serialization.
    #[must_use]
    pub fn is_default(&self) -> bool {
        matches!(self, SortOrder::Asc)
    }
}

/// Controls when an `alert` step fires.
#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, Eq)]
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
    /// Fire when `field` of the first element (or the watched `var`) changes from
    /// its previous value. State is persisted between runs. Use `default` to set
    /// the assumed initial value. No-op when watching a `field` and the element
    /// list is empty.
    Change,
    /// Fire once per element whose identity `field` was not present on the
    /// previous run. State (the set of seen keys) is persisted between runs.
    /// The new element's fields are available to the message.
    Added,
    /// Fire once per identity `field` that was present last run but is now gone.
    /// Only the stored key value is available to the message (the element itself
    /// is no longer in the list), exposed under the watched field's name.
    Removed,
    /// Fire when the watched numeric value (`field` of the first element, or
    /// `var`) is strictly lower than its previous value. State is persisted; use
    /// `default` for the assumed initial value. Parsed like the numeric ops.
    Decrease,
    /// Fire when the watched numeric value is strictly higher than its previous
    /// value. The mirror of `decrease`.
    Increase,
}

impl AlertTrigger {
    /// Whether this is the default trigger (`Any`); used to skip serialization.
    #[must_use]
    pub fn is_default(&self) -> bool {
        matches!(self, AlertTrigger::Any)
    }
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
