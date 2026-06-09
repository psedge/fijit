#![deny(unsafe_code)]

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use fijit_core::{
    config::Config,
    notify,
    pipeline::load_scraper_files,
    scraper::{ScrapeResult, Scraper},
};

#[derive(Parser)]
#[command(name = "fijit", about = "Lightweight web scraper framework")]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run a scraper once
    Run {
        /// Scraper name (e.g. bike-discount)
        name: String,
    },
    /// List all available scrapers and their schedule
    List,
    /// Send a test Slack notification
    TestNotify,
    /// Schedule a scraper via crontab
    Schedule {
        /// Scraper name
        name: String,
        /// Cron expression, e.g. "*/30 * * * *"
        #[arg(long, default_value = "*/30 * * * *")]
        cron: String,
    },
    /// Remove a scraper from crontab
    Unschedule {
        /// Scraper name
        name: String,
    },
    /// Print an example fijit.toml to stdout
    InitConfig,
}

fn load_scrapers(config: &Config) -> Vec<Box<dyn Scraper>> {
    load_scraper_files(config)
        .into_iter()
        .map(|s| Box::new(s) as Box<dyn Scraper>)
        .collect()
}

fn find_scraper<'a>(scrapers: &'a [Box<dyn Scraper>], name: &str) -> Result<&'a dyn Scraper> {
    scrapers
        .iter()
        .find(|s| s.name() == name)
        .map(|s| s.as_ref())
        .ok_or_else(|| anyhow::anyhow!("unknown scraper '{}' — try `fijit list`", name))
}

fn run_scraper(scraper: &dyn Scraper, config: &Config) -> Result<()> {
    let webhook = scraper
        .slack_webhook()
        .or_else(|| config.slack_webhook.clone());

    println!("[{}] checking…", scraper.name());
    match scraper.check()? {
        ScrapeResult::NoChange => println!("[{}] no change", scraper.name()),
        ScrapeResult::Alert(msg) => {
            println!("[{}] ALERT: {}", scraper.name(), msg);
            notify::slack_if_configured(webhook.as_deref(), &msg);
        }
        ScrapeResult::Alerts(msgs) => {
            for msg in &msgs {
                println!("[{}] ALERT: {}", scraper.name(), msg);
                notify::slack_if_configured(webhook.as_deref(), msg);
            }
        }
    }
    Ok(())
}

fn cmd_list(scrapers: &[Box<dyn Scraper>]) {
    println!("{:<20} {:<12} DESCRIPTION", "NAME", "SCHEDULE");
    println!("{}", "-".repeat(70));
    for s in scrapers {
        let schedule = s.schedule().unwrap_or("—");
        println!("{:<20} {:<12} {}", s.name(), schedule, s.description());
    }
}

fn cmd_schedule(name: &str, cron: &str, scrapers: &[Box<dyn Scraper>]) -> Result<()> {
    find_scraper(scrapers, name)?;

    let binary = std::env::current_exe()?.to_string_lossy().into_owned();
    let cron_dir = std::env::current_dir()?.to_string_lossy().into_owned();
    let entry = format!("{cron}\tcd {cron_dir} && {binary} run {name}");
    let tag = format!("# fijit:{name}");

    let existing = read_crontab()?;
    let filtered: Vec<&str> = existing
        .lines()
        .filter(|l| !l.contains(&tag) && !l.contains(&format!("fijit run {name}")))
        .collect();

    let new_crontab = format!("{}\n{tag}\n{entry}\n", filtered.join("\n").trim_end());
    write_crontab(&new_crontab)?;

    println!("Scheduled '{name}' with cron: {cron}");
    println!("Entry: {entry}");
    Ok(())
}

fn cmd_unschedule(name: &str) -> Result<()> {
    let tag = format!("# fijit:{name}");
    let existing = read_crontab()?;
    let filtered: Vec<&str> = existing
        .lines()
        .filter(|l| !l.contains(&tag) && !l.contains(&format!("fijit run {name}")))
        .collect();
    write_crontab(&format!("{}\n", filtered.join("\n").trim_end()))?;
    println!("Removed '{name}' from crontab");
    Ok(())
}

fn read_crontab() -> Result<String> {
    let out = std::process::Command::new("crontab").arg("-l").output()?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn write_crontab(contents: &str) -> Result<()> {
    use std::io::Write;
    let mut tmp = tempfile::NamedTempFile::new()?;
    tmp.write_all(contents.as_bytes())?;
    let path = tmp.path().to_owned();
    let status = std::process::Command::new("crontab").arg(&path).status()?;
    if !status.success() {
        bail!("crontab write failed");
    }
    Ok(())
}

fn cmd_init_config() {
    use std::io::Write;
    let _ = std::io::stdout().write_all(INIT_CONFIG_TEMPLATE.as_bytes());
}

const INIT_CONFIG_TEMPLATE: &str = "\
# fijit.toml — global config (gitignored, keep your secrets here)
#
# Scrapers live in scrapers/*.toml (also gitignored).
# Run: fijit run <name>   to test a scraper
#      fijit schedule <name>   to add it to crontab

obscura_path = \"/usr/local/bin/obscura\"
slack_webhook = \"https://hooks.slack.com/services/...\"
# or: slack_webhook = \"${SLACK_WEBHOOK}\"   to read from the environment

# [vars]
# Global template variables available as {MY_VAR} in alert messages.
# MY_KEY = \"my-value\"
";

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = Config::load_or_default();
    let scrapers = load_scrapers(&config);

    match &cli.command {
        Cmd::Run { name } => {
            let scraper = find_scraper(&scrapers, name)?;
            run_scraper(scraper, &config)?;
        }
        Cmd::List => cmd_list(&scrapers),
        Cmd::TestNotify => {
            let msg = "🧪 *fijit test* — notifications are working!";
            notify::slack_if_configured(config.slack_webhook.as_deref(), msg);
            println!("Test notification sent.");
        }
        Cmd::Schedule { name, cron } => cmd_schedule(name, cron, &scrapers)?,
        Cmd::Unschedule { name } => cmd_unschedule(name)?,
        Cmd::InitConfig => cmd_init_config(),
    }

    Ok(())
}
