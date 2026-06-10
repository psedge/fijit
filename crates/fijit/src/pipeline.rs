use crate::config::Config;
use crate::element::Element;
use crate::obscura::ObscuraRunner;
use crate::scraper::{ScrapeResult, Scraper};
use crate::step::{AlertTrigger, Op, Step};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Error handler for a scraper — sent to Slack when `check()` returns an error.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OnError {
    /// Slack message template. Supports `{name}` and `{error}` placeholders.
    pub message: String,
}

/// A scraper definition loaded from a TOML file in the `scrapers/` directory.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ScraperDef {
    /// Unique identifier used in CLI commands and crontab entries.
    pub name: String,
    /// Human-readable description shown in `fijit list`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Page URL that `query_all` and `eval_json` steps will fetch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Default cron schedule displayed in `fijit list`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schedule: Option<String>,
    /// Per-scraper Slack webhook — overrides the global value from `fijit.toml`.
    /// Supports `${ENV_VAR}` interpolation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slack_webhook: Option<String>,
    /// Ordered list of pipeline steps.
    #[serde(default)]
    pub steps: Vec<Step>,
    /// Optional error handler — notifies Slack when the pipeline returns an error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_error: Option<OnError>,
}

/// A config-file-driven scraper that executes a declarative pipeline.
pub struct ConfigScraper {
    /// Parsed scraper definition.
    pub def: ScraperDef,
    /// Global config (obscura path, global webhook, vars).
    pub config: Config,
}

impl ConfigScraper {
    /// Create a new `ConfigScraper` from a definition and the global config.
    #[must_use]
    pub fn new(def: ScraperDef, config: Config) -> Self {
        Self { def, config }
    }

    /// Resolve the Slack webhook, preferring the per-scraper value over the global one.
    /// Both support `${ENV_VAR}` interpolation.
    #[must_use]
    pub fn resolved_webhook(&self) -> Option<String> {
        self.def
            .slack_webhook
            .as_deref()
            .map(interpolate_env)
            .filter(|s| !s.is_empty())
            .or_else(|| {
                self.config
                    .slack_webhook
                    .as_deref()
                    .map(interpolate_env)
                    .filter(|s| !s.is_empty())
            })
    }
}

impl Scraper for ConfigScraper {
    fn name(&self) -> &str {
        &self.def.name
    }

    fn description(&self) -> &str {
        self.def.description.as_deref().unwrap_or("")
    }

    fn check(&self) -> Result<ScrapeResult> {
        run_pipeline(&self.def, &self.config)
    }

    fn slack_webhook(&self) -> Option<String> {
        self.resolved_webhook()
    }

    fn schedule(&self) -> Option<&str> {
        self.def.schedule.as_deref()
    }

    fn on_error_message(&self) -> Option<&str> {
        self.def.on_error.as_ref().map(|e| e.message.as_str())
    }
}

/// Scan `./scrapers/*.toml` and return a `ConfigScraper` for each valid file.
///
/// Parse errors are printed as warnings and skipped.
#[must_use]
pub fn load_scraper_files(config: &Config) -> Vec<ConfigScraper> {
    let dir = Path::new("scrapers");
    if !dir.exists() {
        return vec![];
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("warn: could not read scrapers/: {e}");
            return vec![];
        }
    };
    let mut scrapers = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        match std::fs::read_to_string(&path).map(|s| toml::from_str::<ScraperDef>(&s)) {
            Ok(Ok(def)) => scrapers.push(ConfigScraper::new(def, config.clone())),
            Ok(Err(e)) => eprintln!("warn: parse error in {}: {e}", path.display()),
            Err(e) => eprintln!("warn: could not read {}: {e}", path.display()),
        }
    }
    scrapers
}

/// Expand `${ENV_VAR}` placeholders in `s` using the process environment.
/// Unknown variables are replaced with an empty string.
#[must_use]
pub fn interpolate_env(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '$' && chars.peek() == Some(&'{') {
            chars.next();
            let key: String = chars.by_ref().take_while(|&ch| ch != '}').collect();
            out.push_str(&std::env::var(&key).unwrap_or_default());
        } else {
            out.push(c);
        }
    }
    out
}

fn state_path(scraper_name: &str, state_key: &str) -> std::path::PathBuf {
    let base = std::env::var("HOME")
        .map_or_else(|_| std::path::PathBuf::from("."), std::path::PathBuf::from);
    base.join(".local")
        .join("share")
        .join("fijit")
        .join("state")
        .join(format!("{scraper_name}-{state_key}"))
}

fn read_state(scraper_name: &str, state_key: &str) -> Option<String> {
    std::fs::read_to_string(state_path(scraper_name, state_key)).ok()
}

fn write_state(scraper_name: &str, state_key: &str, value: &str) {
    let path = state_path(scraper_name, state_key);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(&path, value) {
        eprintln!("warn: could not write state to {}: {e}", path.display());
    }
}

/// Expand `{key}` placeholders in `s` using `vars`. Unknown keys are left as-is.
#[must_use]
pub fn interpolate_template<S: std::hash::BuildHasher>(
    s: &str,
    vars: &HashMap<String, String, S>,
) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' {
            let key: String = chars.by_ref().take_while(|&ch| ch != '}').collect();
            if let Some(val) = vars.get(&key) {
                out.push_str(val);
            } else {
                out.push('{');
                out.push_str(&key);
                out.push('}');
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Extract the first number from `s`, ignoring currency symbols, surrounding
/// text, and thousands separators (commas). For example `"£1,299.00 incl. VAT"`
/// parses to `1299.0`. Returns `None` if no digit is present.
fn parse_number(s: &str) -> Option<f64> {
    let mut num = String::new();
    let mut seen_digit = false;
    let mut seen_dot = false;
    for c in s.chars() {
        match c {
            '0'..='9' => {
                num.push(c);
                seen_digit = true;
            }
            '.' if seen_digit && !seen_dot => {
                num.push(c);
                seen_dot = true;
            }
            ',' if seen_digit => {} // thousands separator
            '-' if num.is_empty() => num.push(c),
            _ => {
                if seen_digit {
                    break;
                }
                num.clear();
            }
        }
    }
    if seen_digit {
        num.parse::<f64>().ok()
    } else {
        None
    }
}

/// Compare `field_val` and `value` numerically. No match if either is non-numeric.
fn num_cmp(field_val: &str, value: &str, f: impl Fn(f64, f64) -> bool) -> bool {
    matches!((parse_number(field_val), parse_number(value)), (Some(a), Some(b)) if f(a, b))
}

fn eval_op(field_val: &str, op: &Op, value: &str) -> bool {
    match op {
        Op::Eq => field_val == value,
        Op::NotEq => field_val != value,
        Op::Contains => field_val.contains(value),
        Op::NotContains => !field_val.contains(value),
        Op::Matches => regex::Regex::new(value).is_ok_and(|re| re.is_match(field_val)),
        Op::StartsWith => field_val.starts_with(value),
        Op::EndsWith => field_val.ends_with(value),
        Op::Gt => num_cmp(field_val, value, |a, b| a > b),
        Op::Lt => num_cmp(field_val, value, |a, b| a < b),
        Op::Gte => num_cmp(field_val, value, |a, b| a >= b),
        Op::Lte => num_cmp(field_val, value, |a, b| a <= b),
    }
}

fn json_to_element(val: &serde_json::Value) -> Element {
    let attrs = val["attrs"]
        .as_object()
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| {
                    v.as_str()
                        .filter(|s| !s.is_empty())
                        .map(|s| (k.clone(), s.to_owned()))
                })
                .collect()
        })
        .unwrap_or_default();
    Element {
        text: val["text"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned),
        class: val["class"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned),
        href: val["href"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned),
        value: val["value"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned),
        attrs,
    }
}

fn element_vars(el: &Element, base: &HashMap<String, String>) -> HashMap<String, String> {
    let mut m = base.clone();
    if let Some(v) = &el.text {
        m.insert("text".to_owned(), v.clone());
    }
    if let Some(v) = &el.class {
        m.insert("class".to_owned(), v.clone());
    }
    if let Some(v) = &el.href {
        m.insert("href".to_owned(), v.clone());
    }
    if let Some(v) = &el.value {
        m.insert("value".to_owned(), v.clone());
    }
    for (k, v) in &el.attrs {
        m.insert(k.clone(), v.clone());
    }
    m
}

struct PipelineCtx<'a> {
    obscura: &'a ObscuraRunner,
    url: &'a str,
    scraper_name: &'a str,
}

struct PipelineState {
    elements: Vec<Element>,
    vars: HashMap<String, String>,
    alerts: Vec<String>,
}

fn run_pipeline(def: &ScraperDef, config: &Config) -> Result<ScrapeResult> {
    let url = def.url.as_deref().map(interpolate_env).unwrap_or_default();
    let mut vars = config.vars.clone();
    vars.insert("url".to_owned(), url.clone());

    let obscura = ObscuraRunner::new(config.obscura());
    let ctx = PipelineCtx {
        obscura: &obscura,
        url: &url,
        scraper_name: &def.name,
    };
    let mut state = PipelineState {
        elements: Vec::new(),
        vars,
        alerts: Vec::new(),
    };

    for (step_idx, step) in def.steps.iter().enumerate() {
        execute_step(step, &ctx, &mut state, step_idx)?;
    }

    Ok(if state.alerts.is_empty() {
        ScrapeResult::NoChange
    } else {
        ScrapeResult::Alerts(state.alerts)
    })
}

#[allow(clippy::too_many_lines)]
fn execute_step(
    step: &Step,
    ctx: &PipelineCtx<'_>,
    ps: &mut PipelineState,
    step_idx: usize,
) -> Result<()> {
    match step {
        Step::QueryAll {
            selector,
            attrs,
            wait,
        } => {
            let selector = interpolate_env(selector);
            let selector_js = serde_json::to_string(&selector).unwrap_or_default();
            let attrs_js = attrs
                .iter()
                .map(|a| {
                    let key = serde_json::to_string(a).unwrap_or_default();
                    format!("{key}: el.getAttribute({key})")
                })
                .collect::<Vec<_>>()
                .join(", ");
            let script = format!(
                "JSON.stringify(Array.from(document.querySelectorAll({selector_js})).map(el => \
                 ({{text: el.textContent.replace(/\\s+/g,' ').trim(), class: el.className, \
                   href: el.getAttribute('href'), value: el.getAttribute('value'), \
                   attrs: {{{attrs_js}}}}})))"
            );
            let json = ctx.obscura.eval_json(ctx.url, &script, wait.unwrap_or(3))?;
            let vals: Vec<serde_json::Value> =
                serde_json::from_str(&json).context("query_all: expected JSON array")?;
            ps.elements = vals.iter().map(json_to_element).collect();
        }
        Step::EvalJson { script, wait } => {
            let json = ctx.obscura.eval_json(ctx.url, script, wait.unwrap_or(3))?;
            let vals: Vec<serde_json::Value> =
                serde_json::from_str(&json).context("eval_json: expected JSON array")?;
            ps.elements = vals.iter().map(json_to_element).collect();
        }
        Step::Find { field, op, value } => {
            let found = ps
                .elements
                .iter()
                .find(|el| {
                    el.get_field(field).is_some_and(|v| match op {
                        Some(op) => eval_op(v, op, value),
                        None => v == value.as_str(),
                    })
                })
                .cloned();
            ps.elements = found.into_iter().collect();
        }
        Step::Filter { field, op, value } => {
            ps.elements
                .retain(|el| el.get_field(field).is_some_and(|v| eval_op(v, op, value)));
        }
        Step::Set { var, value } => {
            ps.vars.insert(var.clone(), interpolate_env(value));
        }
        Step::Map { field, var } => {
            let collected: Vec<String> = ps
                .elements
                .iter()
                .filter_map(|el| el.get_field(field).map(str::to_owned))
                .collect();
            ps.vars.insert(var.clone(), collected.join(", "));
        }
        Step::Alert {
            message,
            on,
            field,
            default,
            id,
        } => match on {
            AlertTrigger::Any => {
                if let Some(el) = ps.elements.first() {
                    ps.alerts
                        .push(interpolate_template(message, &element_vars(el, &ps.vars)));
                }
            }
            AlertTrigger::Each => {
                let alerts: Vec<String> = ps
                    .elements
                    .iter()
                    .map(|el| interpolate_template(message, &element_vars(el, &ps.vars)))
                    .collect();
                ps.alerts.extend(alerts);
            }
            AlertTrigger::Empty => {
                if ps.elements.is_empty() {
                    ps.alerts.push(interpolate_template(message, &ps.vars));
                }
            }
            AlertTrigger::Change => {
                let field = field
                    .as_deref()
                    .context("alert on=change requires 'field'")?;
                if ps.elements.is_empty() {
                    return Ok(());
                }
                let el = &ps.elements[0];
                let current = el.get_field(field).unwrap_or("").to_owned();
                let state_key = id.clone().unwrap_or_else(|| step_idx.to_string());
                let previous = read_state(ctx.scraper_name, &state_key)
                    .or_else(|| default.clone())
                    .unwrap_or_default();
                if current != previous {
                    ps.alerts
                        .push(interpolate_template(message, &element_vars(el, &ps.vars)));
                    write_state(ctx.scraper_name, &state_key, &current);
                }
            }
        },
        Step::Log => {
            println!("[pipeline:log] {} element(s)", ps.elements.len());
            for (i, el) in ps.elements.iter().enumerate() {
                println!(
                    "  [{}] text={:?} class={:?} href={:?} value={:?}",
                    i, el.text, el.class, el.href, el.value
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolate_env_replaces_known_var() {
        std::env::set_var("FIJJIT_TEST_VAR", "hello");
        assert_eq!(
            interpolate_env("prefix_${FIJJIT_TEST_VAR}_suffix"),
            "prefix_hello_suffix"
        );
    }

    #[test]
    fn interpolate_env_unknown_var_becomes_empty() {
        assert_eq!(interpolate_env("${FIJJIT_DEFINITELY_NOT_SET_XYZ}"), "");
    }

    #[test]
    fn interpolate_template_replaces_known_key() {
        let mut vars = HashMap::new();
        vars.insert("url".to_owned(), "https://example.com".to_owned());
        assert_eq!(
            interpolate_template("buy at {url}", &vars),
            "buy at https://example.com"
        );
    }

    #[test]
    fn interpolate_template_leaves_unknown_key() {
        let vars = HashMap::new();
        assert_eq!(interpolate_template("{unknown}", &vars), "{unknown}");
    }

    #[test]
    fn eval_op_contains() {
        assert!(eval_op("foo bar baz", &Op::Contains, "bar"));
        assert!(!eval_op("foo baz", &Op::Contains, "bar"));
    }

    #[test]
    fn eval_op_matches_regex() {
        assert!(eval_op("--stock-1 active", &Op::Matches, r"--stock-\d"));
        assert!(!eval_op("--stock-4 active", &Op::Matches, r"--stock-1$"));
    }

    #[test]
    fn parse_number_extracts_from_noisy_strings() {
        assert_eq!(parse_number("£1,299.00 incl. VAT"), Some(1299.0));
        assert_eq!(parse_number("$42.99"), Some(42.99));
        assert_eq!(parse_number("12 in stock"), Some(12.0));
        assert_eq!(parse_number("-5°C"), Some(-5.0));
        assert_eq!(parse_number("sold out"), None);
    }

    #[test]
    fn eval_op_numeric_comparisons() {
        assert!(eval_op("£1,299.00", &Op::Lt, "1500"));
        assert!(eval_op("£1,299.00", &Op::Gt, "1000"));
        assert!(eval_op("42", &Op::Gte, "42"));
        assert!(eval_op("42", &Op::Lte, "42"));
        assert!(!eval_op("42", &Op::Gt, "42"));
        // Non-numeric field never matches a numeric op.
        assert!(!eval_op("sold out", &Op::Lt, "100"));
    }

    #[test]
    fn json_to_element_maps_fields() {
        let v = serde_json::json!({"text": "58 cm", "class": "item --stock-1", "href": null, "value": ""});
        let el = json_to_element(&v);
        assert_eq!(el.text.as_deref(), Some("58 cm"));
        assert_eq!(el.class.as_deref(), Some("item --stock-1"));
        assert!(el.href.is_none());
        assert!(el.value.is_none());
        assert!(el.attrs.is_empty());
    }

    #[test]
    fn json_to_element_captures_extra_attrs() {
        let v = serde_json::json!({
            "text": "Bike",
            "attrs": {"data-price": "£1,299.00", "aria-label": "", "data-sku": "ABC"}
        });
        let el = json_to_element(&v);
        // Empty attribute values are dropped, non-empty ones are addressable by name.
        assert_eq!(el.get_field("data-price"), Some("£1,299.00"));
        assert_eq!(el.get_field("data-sku"), Some("ABC"));
        assert_eq!(el.get_field("aria-label"), None);
        assert_eq!(el.get_field("text"), Some("Bike"));
        // A custom attr is usable by a numeric op (e.g. price thresholds).
        assert!(eval_op(
            el.get_field("data-price").unwrap(),
            &Op::Lt,
            "1500"
        ));
    }

    #[test]
    fn steps_parse_into_typed_variants() {
        let src = r#"
name = "t"
[[steps]]
action = "query_all"
selector = ".p"
attrs = ["data-price"]

[[steps]]
action = "alert"
message = "hi {data-price}"
"#;
        let def: ScraperDef = toml::from_str(src).unwrap();
        assert_eq!(def.steps.len(), 2);
        assert!(matches!(
            &def.steps[0],
            Step::QueryAll { selector, attrs, .. } if selector == ".p" && attrs == &["data-price"]
        ));
        assert!(matches!(
            &def.steps[1],
            Step::Alert {
                on: AlertTrigger::Any,
                ..
            }
        ));
    }

    #[test]
    fn missing_required_field_is_parse_error() {
        // query_all without a selector must fail at parse time, not at runtime.
        let src = "name = \"t\"\n[[steps]]\naction = \"query_all\"\n";
        assert!(toml::from_str::<ScraperDef>(src).is_err());
    }
}
