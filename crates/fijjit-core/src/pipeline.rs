use crate::config::Config;
use crate::element::Element;
use crate::obscura::ObscuraRunner;
use crate::scraper::{ScrapeResult, Scraper};
use crate::step::{Action, Op, Step};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// A scraper definition loaded from a TOML file in the `scrapers/` directory.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ScraperDef {
    /// Unique identifier used in CLI commands and crontab entries.
    pub name: String,
    /// Human-readable description shown in `fijjit list`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Page URL that `query_all` and `eval_json` steps will fetch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Default cron schedule displayed in `fijjit list`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schedule: Option<String>,
    /// Per-scraper Slack webhook — overrides the global value from `fijjit.toml`.
    /// Supports `${ENV_VAR}` interpolation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slack_webhook: Option<String>,
    /// Ordered list of pipeline steps.
    #[serde(default)]
    pub steps: Vec<Step>,
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

fn interpolate_template(s: &str, vars: &HashMap<String, String>) -> String {
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

fn eval_op(field_val: &str, op: &Op, value: &str) -> bool {
    match op {
        Op::Eq => field_val == value,
        Op::NotEq => field_val != value,
        Op::Contains => field_val.contains(value),
        Op::NotContains => !field_val.contains(value),
        Op::Matches => regex::Regex::new(value).is_ok_and(|re| re.is_match(field_val)),
        Op::StartsWith => field_val.starts_with(value),
        Op::EndsWith => field_val.ends_with(value),
    }
}

fn json_to_element(val: &serde_json::Value) -> Element {
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
    };

    for step in &def.steps {
        execute_step(step, &ctx, &mut state)?;
    }

    Ok(match state.alerts.len() {
        0 => ScrapeResult::NoChange,
        1 => ScrapeResult::Alert(state.alerts.remove(0)),
        _ => ScrapeResult::Alerts(state.alerts),
    })
}

#[allow(clippy::too_many_lines)]
fn execute_step(step: &Step, ctx: &PipelineCtx<'_>, ps: &mut PipelineState) -> Result<()> {
    match step.action {
        Action::QueryAll => {
            let selector = step
                .selector
                .as_deref()
                .context("query_all requires 'selector'")?;
            let selector = interpolate_env(selector);
            let selector_js = serde_json::to_string(&selector).unwrap_or_default();
            let script = format!(
                "JSON.stringify(Array.from(document.querySelectorAll({selector_js})).map(el => \
                 ({{text: el.textContent.replace(/\\s+/g,' ').trim(), class: el.className, \
                   href: el.getAttribute('href'), value: el.getAttribute('value')}})))"
            );
            let json = ctx
                .obscura
                .eval_json(ctx.url, &script, step.wait.unwrap_or(3))?;
            let vals: Vec<serde_json::Value> =
                serde_json::from_str(&json).context("query_all: expected JSON array")?;
            ps.elements = vals.iter().map(json_to_element).collect();
        }
        Action::EvalJson => {
            let script = step
                .script
                .as_deref()
                .context("eval_json requires 'script'")?;
            let json = ctx
                .obscura
                .eval_json(ctx.url, script, step.wait.unwrap_or(3))?;
            let vals: Vec<serde_json::Value> =
                serde_json::from_str(&json).context("eval_json: expected JSON array")?;
            ps.elements = vals.iter().map(json_to_element).collect();
        }
        Action::Find => {
            let field = step.field.as_deref().context("find requires 'field'")?;
            let value = step.value.as_deref().context("find requires 'value'")?;
            let found = ps
                .elements
                .iter()
                .find(|el| {
                    el.get_field(field).is_some_and(|v| match &step.op {
                        Some(op) => eval_op(v, op, value),
                        None => v == value,
                    })
                })
                .cloned();
            ps.elements = found.into_iter().collect();
        }
        Action::Filter => {
            let field = step.field.as_deref().context("filter requires 'field'")?;
            let op = step.op.as_ref().context("filter requires 'op'")?;
            let value = step.value.as_deref().context("filter requires 'value'")?;
            ps.elements
                .retain(|el| el.get_field(field).is_some_and(|v| eval_op(v, op, value)));
        }
        Action::Set => {
            let var = step.var.as_deref().context("set requires 'var'")?;
            let value = step.value.as_deref().context("set requires 'value'")?;
            ps.vars.insert(var.to_owned(), interpolate_env(value));
        }
        Action::Map => {
            let field = step.field.as_deref().context("map requires 'field'")?;
            let var = step.var.as_deref().context("map requires 'var'")?;
            let collected: Vec<String> = ps
                .elements
                .iter()
                .filter_map(|el| el.get_field(field).map(str::to_owned))
                .collect();
            ps.vars.insert(var.to_owned(), collected.join(", "));
        }
        Action::AlertIf => {
            let field = step.field.as_deref().context("alert_if requires 'field'")?;
            let op = step.op.as_ref().context("alert_if requires 'op'")?;
            let value = step.value.as_deref().context("alert_if requires 'value'")?;
            let message = step
                .message
                .as_deref()
                .context("alert_if requires 'message'")?;
            if let Some(el) = ps
                .elements
                .iter()
                .find(|el| el.get_field(field).is_some_and(|v| eval_op(v, op, value)))
            {
                ps.alerts
                    .push(interpolate_template(message, &element_vars(el, &ps.vars)));
            }
        }
        Action::AlertIfEmpty => {
            let message = step
                .message
                .as_deref()
                .context("alert_if_empty requires 'message'")?;
            if ps.elements.is_empty() {
                ps.alerts.push(interpolate_template(message, &ps.vars));
            }
        }
        Action::AlertIfAny => {
            let field = step
                .field
                .as_deref()
                .context("alert_if_any requires 'field'")?;
            let op = step.op.as_ref().context("alert_if_any requires 'op'")?;
            let value = step
                .value
                .as_deref()
                .context("alert_if_any requires 'value'")?;
            let message = step
                .message
                .as_deref()
                .context("alert_if_any requires 'message'")?;
            let matches: Vec<HashMap<String, String>> = ps
                .elements
                .iter()
                .filter(|el| el.get_field(field).is_some_and(|v| eval_op(v, op, value)))
                .map(|el| element_vars(el, &ps.vars))
                .collect();
            for ev in matches {
                ps.alerts.push(interpolate_template(message, &ev));
            }
        }
        Action::Log => {
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
    fn json_to_element_maps_fields() {
        let v = serde_json::json!({"text": "58 cm", "class": "item --stock-1", "href": null, "value": ""});
        let el = json_to_element(&v);
        assert_eq!(el.text.as_deref(), Some("58 cm"));
        assert_eq!(el.class.as_deref(), Some("item --stock-1"));
        assert!(el.href.is_none());
        assert!(el.value.is_none());
    }
}
