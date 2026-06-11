#![deny(unsafe_code)]

mod element;
mod error;
mod notify;
mod obscura;
mod pipeline;
mod scraper;
mod step;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use pipeline::{interpolate_template, load_scraper_files};
use scraper::{ScrapeResult, Scraper};

#[derive(Parser)]
#[command(name = "fijit", about = "Lightweight web scraper framework")]
struct Cli {
    /// Path to the Obscura binary. Defaults to `obscura` found on $PATH.
    #[arg(long, global = true)]
    obscura: Option<String>,
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
}

fn load_scrapers() -> Vec<Box<dyn Scraper>> {
    load_scraper_files()
        .into_iter()
        .map(|s| Box::new(s) as Box<dyn Scraper>)
        .collect()
}

/// Resolve the Obscura binary path from the `--obscura` flag, otherwise the
/// first `obscura` on `$PATH`. Errors if neither is available.
fn resolve_obscura(flag: Option<&str>) -> Result<String> {
    if let Some(p) = flag {
        return Ok(p.to_owned());
    }
    find_on_path("obscura")
        .ok_or_else(|| anyhow::anyhow!("obscura not found on $PATH; pass --obscura <path>"))
}

/// Return the full path to `bin` if it exists in any `$PATH` directory.
fn find_on_path(bin: &str) -> Option<String> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths).find_map(|dir| {
        let full = dir.join(bin);
        full.is_file().then(|| full.to_string_lossy().into_owned())
    })
}

fn find_scraper<'a>(scrapers: &'a [Box<dyn Scraper>], name: &str) -> Result<&'a dyn Scraper> {
    scrapers
        .iter()
        .find(|s| s.name() == name)
        .map(|s| s.as_ref())
        .ok_or_else(|| anyhow::anyhow!("unknown scraper '{}' (try `fijit list`)", name))
}

fn run_scraper(scraper: &dyn Scraper, obscura: &str) -> Result<()> {
    let webhook = scraper.slack_webhook();

    println!("[{}] checking…", scraper.name());
    match scraper.check(obscura) {
        Ok(ScrapeResult::NoChange) => println!("[{}] no change", scraper.name()),
        Ok(ScrapeResult::Alerts(msgs)) => {
            for msg in &msgs {
                println!("[{}] ALERT: {}", scraper.name(), msg);
                notify::slack_if_configured(webhook.as_deref(), msg);
            }
        }
        Err(e) => {
            eprintln!("[{}] ERROR: {e}", scraper.name());
            if let Some(tmpl) = scraper.on_error_message() {
                let mut vars = std::collections::HashMap::new();
                vars.insert("name".to_owned(), scraper.name().to_owned());
                vars.insert("error".to_owned(), e.to_string());
                let msg = interpolate_template(tmpl, &vars);
                notify::slack_if_configured(webhook.as_deref(), &msg);
            }
            return Err(e);
        }
    }
    Ok(())
}

fn cmd_list(scrapers: &[Box<dyn Scraper>]) {
    println!("{:<20} {:<12} DESCRIPTION", "NAME", "SCHEDULE");
    println!("{}", "-".repeat(70));
    for s in scrapers {
        let schedule = s.schedule().unwrap_or("-");
        println!("{:<20} {:<12} {}", s.name(), schedule, s.description());
    }
}

fn cmd_schedule(
    name: &str,
    cron: &str,
    obscura: &str,
    scrapers: &[Box<dyn Scraper>],
) -> Result<()> {
    find_scraper(scrapers, name)?;

    let binary = std::env::current_exe()?.to_string_lossy().into_owned();
    let cron_dir = std::env::current_dir()?.to_string_lossy().into_owned();

    let log_dir = "/var/log/fijit";
    if let Err(e) = std::fs::create_dir_all(log_dir) {
        anyhow::bail!(
            "cannot create log directory {log_dir}: {e}\n\
             Run: sudo mkdir -p {log_dir} && sudo chown $(whoami) {log_dir}"
        );
    }
    // Verify the directory is actually writable before embedding the path.
    let probe = std::path::Path::new(log_dir).join(".fijit-write-test");
    std::fs::write(&probe, b"").with_context(|| {
        format!(
            "{log_dir} exists but is not writable by the current user.\n\
             Run: sudo chown $(whoami) {log_dir}"
        )
    })?;
    let _ = std::fs::remove_file(&probe);
    let log_path = format!("{log_dir}/{name}.log");

    // Embed the resolved Obscura path so the cron job (with its minimal PATH)
    // does not depend on `obscura` being discoverable at run time.
    let entry = format!(
        "{cron}\tcd {cron_dir} && {binary} --obscura {obscura} run {name} >> {log_path} 2>&1"
    );
    let tag = format!("# fijit:{name}");

    let existing = read_crontab()?;
    let filtered = strip_scraper_lines(&existing, name);
    let new_crontab = format!("{}\n{tag}\n{entry}\n", filtered.join("\n").trim_end());
    write_crontab(&new_crontab)?;

    println!("Scheduled '{name}' with cron: {cron}");
    println!("Log: {log_path}");
    println!("Entry: {entry}");
    Ok(())
}

fn cmd_unschedule(name: &str) -> Result<()> {
    let existing = read_crontab()?;
    let filtered = strip_scraper_lines(&existing, name);
    write_crontab(&format!("{}\n", filtered.join("\n").trim_end()))?;
    println!("Removed '{name}' from crontab");
    Ok(())
}

/// Remove fijit's crontab lines for `name`: the `# fijit:<name>` tag line and any
/// entry line invoking `run <name>`. The tag is matched on the whole line and the
/// entry on a space-delimited ` run <name> `, so it stays correct regardless of
/// flags in the command (e.g. `--obscura`) and never clips a scraper whose name
/// is a prefix of another (e.g. `rose` vs `rose-backroad-al`).
fn strip_scraper_lines(existing: &str, name: &str) -> Vec<String> {
    let tag = format!("# fijit:{name}");
    let run = format!(" run {name} ");
    existing
        .lines()
        .filter(|l| l.trim() != tag && !l.contains(&run))
        .map(str::to_owned)
        .collect()
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

fn main() -> Result<()> {
    let cli = Cli::parse();
    let scrapers = load_scrapers();

    match &cli.command {
        Cmd::Run { name } => {
            let obscura = resolve_obscura(cli.obscura.as_deref())?;
            let scraper = find_scraper(&scrapers, name)?;
            run_scraper(scraper, &obscura)?;
        }
        Cmd::List => cmd_list(&scrapers),
        Cmd::Schedule { name, cron } => {
            let obscura = resolve_obscura(cli.obscura.as_deref())?;
            cmd_schedule(name, cron, &obscura, &scrapers)?;
        }
        Cmd::Unschedule { name } => cmd_unschedule(name)?,
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::strip_scraper_lines;

    #[test]
    fn strip_removes_tag_and_flag_bearing_entry() {
        let crontab = "SLACK_WEBHOOK=https://example\n\
             # fijit:rose-backroad-al\n\
             0 */6 * * *\tcd /home/peter && /usr/local/bin/fijit --obscura /usr/local/bin/obscura run rose-backroad-al >> /var/log/fijit/rose-backroad-al.log 2>&1\n";
        let left = strip_scraper_lines(crontab, "rose-backroad-al").join("\n");
        // The env line survives; the tag and the --obscura entry are gone (the bug).
        assert!(left.contains("SLACK_WEBHOOK=https://example"));
        assert!(!left.contains("rose-backroad-al"));
    }

    #[test]
    fn strip_clears_orphaned_entry_without_tag() {
        // An entry left behind by the old buggy dedup (no tag line) is still removed.
        let crontab = "*/30 * * * *\tcd /x && /usr/local/bin/fijit --obscura /o run bikester >> /l/bikester.log 2>&1\n";
        assert!(strip_scraper_lines(crontab, "bikester").is_empty());
    }

    #[test]
    fn strip_keeps_other_scrapers_and_prefix_overlaps() {
        let crontab = "# fijit:rose\n\
             0 6 * * *\t/f --obscura /o run rose >> /l/rose.log 2>&1\n\
             # fijit:rose-backroad-al\n\
             0 6 * * *\t/f --obscura /o run rose-backroad-al >> /l/rose-backroad-al.log 2>&1\n";
        // Removing `rose` must not touch `rose-backroad-al`.
        let left = strip_scraper_lines(crontab, "rose").join("\n");
        assert!(!left.contains("# fijit:rose\n") && !left.contains(" run rose "));
        assert!(left.contains("# fijit:rose-backroad-al"));
        assert!(left.contains(" run rose-backroad-al "));
    }
}
