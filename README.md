# fijjit

A lightweight, extensible web scraper framework in Rust. Define scrapers as simple trait implementations, get Slack notifications and cron scheduling out of the box.

## How it works

Fijjit uses [Obscura](https://github.com/h4ckf0r0day/obscura) — a stealth headless browser — to fetch pages that would otherwise block standard HTTP clients (e.g. Cloudflare-protected sites). Scrapers run on a schedule and notify you via Slack when something changes.

## Installation

Download the latest binary for your platform from [releases](https://github.com/psedge/fijjit/releases):

```bash
# macOS (Apple Silicon)
curl -L https://github.com/psedge/fijjit/releases/latest/download/fijjit-<version>-aarch64-apple-darwin.tar.gz | tar xz
# macOS (Intel)
curl -L https://github.com/psedge/fijjit/releases/latest/download/fijjit-<version>-x86_64-apple-darwin.tar.gz | tar xz
# Raspberry Pi (64-bit)
curl -L https://github.com/psedge/fijjit/releases/latest/download/fijjit-<version>-aarch64-unknown-linux-gnu.tar.gz | tar xz
```

You'll also need [Obscura](https://github.com/h4ckf0r0day/obscura/releases):

```bash
# macOS (Apple Silicon)
curl -L https://github.com/h4ckf0r0day/obscura/releases/latest/download/obscura-aarch64-macos.tar.gz | tar xz -C /usr/local/bin
```

## Configuration

Fijjit looks for config at `./fijjit.toml` first, then `~/.config/fijjit/config.toml`.

```bash
fijjit init-config > fijjit.toml
```

```toml
# fijjit.toml
obscura_path = "/usr/local/bin/obscura"
slack_webhook = "https://hooks.slack.com/services/..."

[scrapers.bike-discount]
schedule = "*/30 * * * *"
```

## Usage

```bash
fijjit list                              # show all scrapers and their schedules
fijjit run bike-discount                 # run a scraper once
fijjit test-notify                       # send a test Slack message
fijjit schedule bike-discount            # add to crontab (default: every 30 min)
fijjit schedule bike-discount --cron "0 9 * * *"  # daily at 9am
fijjit unschedule bike-discount          # remove from crontab
```

## Adding a scraper

1. Create a new crate under `crates/scrapers/your-scraper/`
2. Implement the `Scraper` trait from `fijjit-core`:

```rust
use fijjit_core::scraper::{ScrapeResult, Scraper};

pub struct MyScraper { /* ... */ }

impl Scraper for MyScraper {
    fn name(&self) -> &str { "my-scraper" }
    fn description(&self) -> &str { "Watches something useful" }

    fn check(&self) -> anyhow::Result<ScrapeResult> {
        // fetch, parse, compare
        Ok(ScrapeResult::Alert("Something changed!".into()))
    }
}
```

3. Register it in `crates/fijjit-cli/src/main.rs` → `load_scrapers()`

## License

MIT
