use crate::config::Config;
use crate::element::Element;
use crate::obscura::ObscuraRunner;
use crate::scraper::{ScrapeResult, Scraper};
use crate::step::{AlertTrigger, Op, SortOrder, Step};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
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
    /// Path this definition was loaded from. Set by [`load_scraper_files`]; used
    /// to persist alert state back into the scraper's own file. Not part of the
    /// TOML schema.
    #[serde(skip)]
    pub source_path: Option<std::path::PathBuf>,
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
            Ok(Ok(mut def)) => {
                def.source_path = Some(path);
                scrapers.push(ConfigScraper::new(def, config.clone()));
            }
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

/// Load a scraper's persisted alert state from a `[state]` table in its own TOML
/// file. Keyed by alert `id`; values are the last-seen reading each
/// `change`/`decrease`/`increase`/`added`/`removed` alert compares against.
/// Returns an empty map when the file has no `[state]` table yet.
fn load_state(source: Option<&Path>) -> std::collections::BTreeMap<String, String> {
    source
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|text| text.parse::<toml_edit::DocumentMut>().ok())
        .and_then(|doc| doc.get("state").and_then(|i| i.as_table().cloned()))
        .map(|table| {
            table
                .iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.to_owned(), s.to_owned())))
                .collect()
        })
        .unwrap_or_default()
}

/// Persist alert state back into the scraper's own TOML file as a `[state]`
/// table, rewriting only that table and preserving the rest of the document
/// (comments, step order, formatting) via `toml_edit`. No-op when there's
/// nothing to persist (e.g. a scraper using only `any`/`each`/`empty` alerts).
fn save_state(source: &Path, state: &std::collections::BTreeMap<String, String>) {
    if state.is_empty() {
        return;
    }
    let text = match std::fs::read_to_string(source) {
        Ok(t) => t,
        Err(e) => {
            eprintln!(
                "warn: could not read {} to save state: {e}",
                source.display()
            );
            return;
        }
    };
    let mut doc = match text.parse::<toml_edit::DocumentMut>() {
        Ok(d) => d,
        Err(e) => {
            eprintln!(
                "warn: {} is not valid TOML, not saving state: {e}",
                source.display()
            );
            return;
        }
    };
    let mut table = toml_edit::Table::new();
    for (k, v) in state {
        table.insert(k, toml_edit::value(v.clone()));
    }
    doc["state"] = toml_edit::Item::Table(table);
    if let Err(e) = std::fs::write(source, doc.to_string()) {
        eprintln!("warn: could not write state to {}: {e}", source.display());
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

/// Build the `JSON.stringify(...)` extraction script shared by `query_all` and
/// `follow`: map each matched element to its built-in fields plus any requested
/// extra `attrs`.
fn build_query_script(selector: &str, attrs: &[String]) -> String {
    let selector_js = serde_json::to_string(selector).unwrap_or_default();
    let attrs_js = attrs
        .iter()
        .map(|a| {
            let key = serde_json::to_string(a).unwrap_or_default();
            format!("{key}: el.getAttribute({key})")
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "JSON.stringify(Array.from(document.querySelectorAll({selector_js})).map(el => \
         ({{text: el.textContent.replace(/\\s+/g,' ').trim(), class: el.className, \
           href: el.getAttribute('href'), value: el.getAttribute('value'), \
           attrs: {{{attrs_js}}}}})))"
    )
}

/// Resolve a possibly-relative `href` against the page `base` URL. Absolute URLs
/// pass through unchanged; everything else is hung off the base's origin.
fn resolve_url(base: &str, href: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") {
        return href.to_owned();
    }
    let origin = base
        .find("://")
        .and_then(|s| base[s + 3..].find('/').map(|i| &base[..s + 3 + i]))
        .unwrap_or(base);
    format!("{origin}/{}", href.trim_start_matches('/'))
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
}

struct PipelineState {
    elements: Vec<Element>,
    vars: HashMap<String, String>,
    alerts: Vec<String>,
    /// Persisted alert state for this run, loaded from and flushed back to the
    /// scraper's `*.state.toml`. Keyed by alert `id`.
    state: std::collections::BTreeMap<String, String>,
}

fn run_pipeline(def: &ScraperDef, config: &Config) -> Result<ScrapeResult> {
    let url = def.url.as_deref().map(interpolate_env).unwrap_or_default();
    let mut vars = config.vars.clone();
    vars.insert("url".to_owned(), url.clone());

    let obscura = ObscuraRunner::new(config.obscura());
    let ctx = PipelineCtx {
        obscura: &obscura,
        url: &url,
    };
    let mut state = PipelineState {
        elements: Vec::new(),
        vars,
        alerts: Vec::new(),
        state: load_state(def.source_path.as_deref()),
    };

    for (step_idx, step) in def.steps.iter().enumerate() {
        execute_step(step, &ctx, &mut state, step_idx)?;
    }

    if let Some(source) = &def.source_path {
        save_state(source, &state.state);
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
            let script = build_query_script(&selector, attrs);
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
                    let rhs = interpolate_template(value, &element_vars(el, &ps.vars));
                    el.get_field(field).is_some_and(|v| match op {
                        Some(op) => eval_op(v, op, &rhs),
                        None => v == rhs.as_str(),
                    })
                })
                .cloned();
            ps.elements = found.into_iter().collect();
        }
        Step::Filter { field, op, value } => {
            ps.elements.retain(|el| {
                let rhs = interpolate_template(value, &element_vars(el, &ps.vars));
                el.get_field(field).is_some_and(|v| eval_op(v, op, &rhs))
            });
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
        Step::Sort {
            field,
            order,
            numeric,
        } => {
            ps.elements.sort_by(|a, b| {
                let (av, bv) = (
                    a.get_field(field).unwrap_or(""),
                    b.get_field(field).unwrap_or(""),
                );
                let ord = if *numeric {
                    match (parse_number(av), parse_number(bv)) {
                        (Some(x), Some(y)) => {
                            x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal)
                        }
                        // Numeric values sort before non-numeric ones.
                        (Some(_), None) => std::cmp::Ordering::Less,
                        (None, Some(_)) => std::cmp::Ordering::Greater,
                        (None, None) => std::cmp::Ordering::Equal,
                    }
                } else {
                    av.cmp(bv)
                };
                match order {
                    SortOrder::Asc => ord,
                    SortOrder::Desc => ord.reverse(),
                }
            });
        }
        Step::Compute { field, template } => {
            for el in &mut ps.elements {
                let rendered = interpolate_template(template, &element_vars(el, &ps.vars));
                el.set_field(field, rendered);
            }
        }
        Step::Follow {
            field,
            selector,
            attrs,
            wait,
        } => {
            let selector = interpolate_env(selector);
            let script = build_query_script(&selector, attrs);
            let links: Vec<String> = ps
                .elements
                .iter()
                .filter_map(|el| el.get_field(field).map(|l| resolve_url(ctx.url, l)))
                .collect();
            let mut collected = Vec::new();
            for target in links {
                let json = ctx.obscura.eval_json(&target, &script, wait.unwrap_or(3))?;
                let vals: Vec<serde_json::Value> =
                    serde_json::from_str(&json).context("follow: expected JSON array")?;
                for v in &vals {
                    let mut el = json_to_element(v);
                    // Expose which page each match came from, for templating.
                    el.set_field("source", target.clone());
                    collected.push(el);
                }
            }
            ps.elements = collected;
        }
        Step::Alert {
            message,
            on,
            field,
            var,
            default,
            id,
        } => {
            let state_key = id.clone().unwrap_or_else(|| step_idx.to_string());
            match on {
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
                AlertTrigger::Change | AlertTrigger::Decrease | AlertTrigger::Increase => {
                    // Resolve the watched value and the vars used to render the
                    // message: either a named `var`, or `field` of the first element.
                    let (current, tvars) = if let Some(var) = var {
                        (
                            ps.vars.get(var).cloned().unwrap_or_default(),
                            ps.vars.clone(),
                        )
                    } else {
                        let field = field.as_deref().context(
                            "alert on=change/decrease/increase requires 'field' or 'var'",
                        )?;
                        let Some(el) = ps.elements.first() else {
                            return Ok(());
                        };
                        (
                            el.get_field(field).unwrap_or("").to_owned(),
                            element_vars(el, &ps.vars),
                        )
                    };
                    let previous = ps
                        .state
                        .get(&state_key)
                        .cloned()
                        .or_else(|| default.clone());
                    let fire = match on {
                        AlertTrigger::Change => current != previous.clone().unwrap_or_default(),
                        // Direction triggers need both readings to be numeric.
                        AlertTrigger::Decrease | AlertTrigger::Increase => {
                            match (
                                parse_number(&current),
                                previous.as_deref().and_then(parse_number),
                            ) {
                                (Some(c), Some(p)) => {
                                    matches!(on, AlertTrigger::Decrease) == (c < p) && c != p
                                }
                                _ => false,
                            }
                        }
                        _ => unreachable!(),
                    };
                    if fire {
                        ps.alerts.push(interpolate_template(message, &tvars));
                    }
                    // Persist the latest reading so the next run compares against it.
                    // For direction triggers, only store parseable numbers so a
                    // transient non-numeric value doesn't reset the baseline.
                    let store =
                        matches!(on, AlertTrigger::Change) || parse_number(&current).is_some();
                    if store {
                        ps.state.insert(state_key, current);
                    }
                }
                AlertTrigger::Added | AlertTrigger::Removed => {
                    let field = field
                        .as_deref()
                        .context("alert on=added/removed requires 'field'")?;
                    let previous: HashSet<String> = ps
                        .state
                        .get(&state_key)
                        .map(|s| s.lines().map(str::to_owned).collect())
                        .unwrap_or_default();
                    let current_keys: Vec<String> = ps
                        .elements
                        .iter()
                        .filter_map(|el| el.get_field(field).map(str::to_owned))
                        .collect();
                    match on {
                        AlertTrigger::Added => {
                            for el in &ps.elements {
                                if el.get_field(field).is_some_and(|k| !previous.contains(k)) {
                                    ps.alerts.push(interpolate_template(
                                        message,
                                        &element_vars(el, &ps.vars),
                                    ));
                                }
                            }
                        }
                        AlertTrigger::Removed => {
                            let current: HashSet<&String> = current_keys.iter().collect();
                            for gone in previous.iter().filter(|k| !current.contains(k)) {
                                let mut tvars = ps.vars.clone();
                                tvars.insert(field.to_owned(), gone.clone());
                                ps.alerts.push(interpolate_template(message, &tvars));
                            }
                        }
                        _ => unreachable!(),
                    }
                    ps.state.insert(state_key, current_keys.join("\n"));
                }
            }
        }
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

    fn el(value: &str) -> Element {
        Element {
            value: Some(value.to_owned()),
            ..Default::default()
        }
    }

    fn run_step(step: &Step, ps: &mut PipelineState) {
        let obscura = ObscuraRunner::new("/bin/true");
        let ctx = PipelineCtx {
            obscura: &obscura,
            url: "https://shop.test/sale/bikes?x=1#list",
        };
        execute_step(step, &ctx, ps, 0).unwrap();
    }

    fn state(elements: Vec<Element>) -> PipelineState {
        PipelineState {
            elements,
            vars: HashMap::new(),
            alerts: Vec::new(),
            state: std::collections::BTreeMap::new(),
        }
    }

    /// Run one step against a carried-over state map (as a real multi-run scraper
    /// would persist between runs), returning the alerts it emitted.
    fn run_persist(
        step: &Step,
        mut ps: PipelineState,
        persisted: &mut std::collections::BTreeMap<String, String>,
    ) -> Vec<String> {
        ps.state = std::mem::take(persisted);
        run_step(step, &mut ps);
        *persisted = ps.state;
        ps.alerts
    }

    #[test]
    fn resolve_url_handles_absolute_relative_and_full() {
        let base = "https://shop.test/sale/bikes?x=1#list";
        assert_eq!(
            resolve_url(base, "/p/bike-123"),
            "https://shop.test/p/bike-123"
        );
        assert_eq!(
            resolve_url(base, "p/bike-123"),
            "https://shop.test/p/bike-123"
        );
        assert_eq!(
            resolve_url(base, "https://other.test/x"),
            "https://other.test/x"
        );
    }

    #[test]
    fn sort_numeric_ascending_then_descending() {
        let mut ps = state(vec![el("€1,799.00"), el("€1,499.00"), el("sold out")]);
        run_step(
            &Step::Sort {
                field: "value".into(),
                order: SortOrder::Asc,
                numeric: true,
            },
            &mut ps,
        );
        // Numeric values sort low→high; the non-numeric one lands last.
        let order: Vec<_> = ps.elements.iter().map(|e| e.value.as_deref()).collect();
        assert_eq!(
            order,
            vec![Some("€1,499.00"), Some("€1,799.00"), Some("sold out")]
        );

        run_step(
            &Step::Sort {
                field: "value".into(),
                order: SortOrder::Desc,
                numeric: true,
            },
            &mut ps,
        );
        assert_eq!(ps.elements[0].value.as_deref(), Some("sold out"));
        assert_eq!(ps.elements[1].value.as_deref(), Some("€1,799.00"));
    }

    #[test]
    fn compute_writes_a_derived_field() {
        let mut ps = state(vec![Element {
            text: Some("Backroad AL".into()),
            value: Some("€1,499.00".into()),
            ..Default::default()
        }]);
        run_step(
            &Step::Compute {
                field: "label".into(),
                template: "{text} @ {value}".into(),
            },
            &mut ps,
        );
        assert_eq!(
            ps.elements[0].get_field("label"),
            Some("Backroad AL @ €1,499.00")
        );
    }

    #[test]
    fn filter_compares_one_field_against_another() {
        let cheaper = Element {
            value: Some("1499".into()),
            attrs: HashMap::from([("old".to_owned(), "2099".to_owned())]),
            ..Default::default()
        };
        let not_cheaper = Element {
            value: Some("2099".into()),
            attrs: HashMap::from([("old".to_owned(), "2099".to_owned())]),
            ..Default::default()
        };
        let mut ps = state(vec![cheaper, not_cheaper]);
        run_step(
            &Step::Filter {
                field: "value".into(),
                op: Op::Lt,
                value: "{old}".into(), // field-to-field: current < old price
            },
            &mut ps,
        );
        assert_eq!(ps.elements.len(), 1);
        assert_eq!(ps.elements[0].value.as_deref(), Some("1499"));
    }

    fn alert(on: AlertTrigger, field: Option<&str>, var: Option<&str>, id: &str) -> Step {
        Step::Alert {
            message: "{text}{value}".into(),
            on,
            field: field.map(str::to_owned),
            var: var.map(str::to_owned),
            default: None,
            id: Some(id.to_owned()),
        }
    }

    fn el_text(text: &str) -> Element {
        Element {
            text: Some(text.to_owned()),
            ..Default::default()
        }
    }

    #[test]
    fn state_backed_triggers_fire_on_the_right_transitions() {
        // One in-memory map stands in for the scraper's `*.state.toml`, carried
        // across runs exactly as the pipeline persists it. Distinct ids coexist.
        let mut st = std::collections::BTreeMap::new();

        // decrease: no prior reading -> silent; then a drop -> fires; flat -> silent.
        let drop_alert = alert(AlertTrigger::Decrease, Some("value"), None, "price");
        assert!(run_persist(&drop_alert, state(vec![el("2099")]), &mut st).is_empty());
        assert_eq!(
            run_persist(&drop_alert, state(vec![el("1499")]), &mut st).len(),
            1
        );
        assert!(run_persist(&drop_alert, state(vec![el("1499")]), &mut st).is_empty());

        // increase: first reading silent, then a rise fires.
        let rise_alert = alert(AlertTrigger::Increase, Some("value"), None, "up");
        assert!(run_persist(&rise_alert, state(vec![el("1799")]), &mut st).is_empty());
        assert_eq!(
            run_persist(&rise_alert, state(vec![el("1899")]), &mut st).len(),
            1
        );

        // change watching a var rather than the first element.
        let set_alert = alert(AlertTrigger::Change, None, Some("models"), "set");
        for (val, expect) in [("A, B", 1usize), ("A, B", 0), ("A, B, C", 1)] {
            let mut ps = state(vec![]);
            ps.vars.insert("models".into(), val.into());
            assert_eq!(
                run_persist(&set_alert, ps, &mut st).len(),
                expect,
                "change-on-var for {val:?}"
            );
        }

        // added: empty baseline fires for all; next run fires only the new key.
        let add_alert = alert(AlertTrigger::Added, Some("text"), None, "models");
        let r = run_persist(
            &add_alert,
            state(vec![el_text("APEX"), el_text("GRX")]),
            &mut st,
        );
        assert_eq!(r.len(), 2);
        let r = run_persist(
            &add_alert,
            state(vec![el_text("APEX"), el_text("GRX"), el_text("FORCE")]),
            &mut st,
        );
        assert_eq!(r.len(), 1);
        assert!(r[0].contains("FORCE"), "only the new model fires");

        // removed: a key present last run but gone now fires, exposing its value.
        let rm_alert = alert(AlertTrigger::Removed, Some("text"), None, "rm");
        run_persist(
            &rm_alert,
            state(vec![el_text("APEX"), el_text("GRX")]),
            &mut st,
        );
        let r = run_persist(&rm_alert, state(vec![el_text("APEX")]), &mut st);
        assert_eq!(r.len(), 1);
        assert!(r[0].contains("GRX"), "the dropped model fires");
    }

    #[test]
    fn state_persists_into_the_scraper_file_preserving_comments() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            "# my scraper\n\
             name = \"x\"\n\
             url = \"https://e.test\"\n\n\
             [[steps]]\n\
             action = \"query_all\"\n\
             selector = \".p\"  # the products\n"
        )
        .unwrap();
        let path = f.path().to_path_buf();

        let mut map = std::collections::BTreeMap::new();
        map.insert("set".to_owned(), "A, B".to_owned());
        map.insert("models".to_owned(), "APEX\nGRX".to_owned());
        save_state(&path, &map);

        let written = std::fs::read_to_string(&path).unwrap();
        // The hand-written comments and structure survive the rewrite.
        assert!(
            written.contains("# my scraper"),
            "leading comment preserved"
        );
        assert!(
            written.contains("# the products"),
            "inline comment preserved"
        );
        assert!(written.contains("[state]"));

        // State round-trips back out, newline-joined sets included.
        assert_eq!(load_state(Some(&path)), map);

        // The file with its [state] table still parses as a scraper definition.
        let def: ScraperDef = toml::from_str(&written).unwrap();
        assert_eq!(def.name, "x");
        assert_eq!(def.steps.len(), 1);
    }
}
