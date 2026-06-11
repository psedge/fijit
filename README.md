# fijit

[![CI](https://github.com/psedge/fijit/actions/workflows/ci.yml/badge.svg)](https://github.com/psedge/fijit/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A declarative web scraper framework. Define a pipeline in TOML, get Slack alerts when something changes.

---

## How it works

Scrapers live in `scrapers/*.toml`. Each file is a pipeline: a sequence of steps that fetch a page, extract elements, and fire alerts. No Rust required.

Fijit uses [Obscura](https://github.com/h4ckf0r0day/obscura), a stealth headless browser, to bypass Cloudflare and similar bot protection. Standard HTTP clients get a 403; Obscura does not.

```
query_all -> find -> alert
    |          |        |
 fetch &     narrow   emit Slack
 extract     target   when triggered
```

---

## Installation

Download the latest binary from [releases](https://github.com/psedge/fijit/releases):

```bash
# macOS (Apple Silicon)
curl -L https://github.com/psedge/fijit/releases/latest/download/fijit-aarch64-apple-darwin.tar.gz | tar xz -C /usr/local/bin

# macOS (Intel)
curl -L https://github.com/psedge/fijit/releases/latest/download/fijit-x86_64-apple-darwin.tar.gz | tar xz -C /usr/local/bin

# Raspberry Pi (64-bit)
curl -L https://github.com/psedge/fijit/releases/latest/download/fijit-aarch64-unknown-linux-gnu.tar.gz | tar xz -C /usr/local/bin
```

You will also need [Obscura](https://github.com/h4ckf0r0day/obscura/releases):

```bash
# macOS (Apple Silicon)
curl -L https://github.com/h4ckf0r0day/obscura/releases/latest/download/obscura-aarch64-macos.tar.gz \
  | tar xz -C /usr/local/bin
```

---

## Quick start

```bash
# 1. generate a config file
fijit init-config > fijit.toml

# 2. edit fijit.toml with your Obscura path and Slack webhook
# 3. create a scraper in scrapers/my-scraper.toml
# 4. run it
fijit run my-scraper

# 5. schedule it
fijit schedule my-scraper --cron "*/30 * * * *"
```

---

## Configuration

Fijit looks for `./fijit.toml` first, then `~/.config/fijit/config.toml`.

```toml
obscura_path = "/usr/local/bin/obscura"
slack_webhook = "https://hooks.slack.com/services/..."

[vars]
# Custom variables available as {MY_VAR} in alert messages
MY_VAR = "value"
```

Config values support `${ENV_VAR}` interpolation:

```toml
slack_webhook = "${SLACK_WEBHOOK}"
```

---

## Writing a scraper

Scrapers live in `scrapers/*.toml` (gitignored, keep secrets there).

```toml
name        = "my-scraper"
description = "Watches something for changes"
url         = "https://example.com/product-page"
schedule    = "*/30 * * * *"
slack_webhook = "${SLACK_WEBHOOK}"   # overrides global; omit to use global

[on_error]
message = "⚠️ *{name} scraper failed*: {error}"

[[steps]]
action   = "query_all"
selector = ".stock-status"

[[steps]]
action = "filter"
field  = "text"
op     = "starts_with"
value  = "Target item"

[[steps]]
action = "filter"
field  = "class"
op     = "contains"
value  = "in-stock"

[[steps]]
action  = "alert"
message = "Target item is available: {url}"
```

The pipeline state is a list of `Element` objects. Each step reads from and writes back to that list. `alert` emits Slack messages without modifying the list.

Every element exposes the built-in fields `text`, `class`, `href`, and `value`. To capture other attributes (e.g. `data-price`, `aria-label`), list them in `attrs` on the `query_all` step. Each then works anywhere a `field` is used, and as a `{name}` template variable:

```toml
[[steps]]
action = "query_all"
selector = ".product"
attrs  = ["data-price", "data-sku"]

[[steps]]
action = "filter"
field  = "data-price"   # a captured attribute
op     = "lt"
value  = "100"

[[steps]]
action  = "alert"
message = "{data-sku} dropped to {data-price}: {url}"
```

---

## Action reference

| Action | What it does | Required fields |
|---|---|---|
| `query_all` | Fetch `url` with Obscura, query all elements matching `selector`. Replaces state. | `selector` |
| `eval_json` | Fetch `url`, run a JS `script`, parse the JSON result. Replaces state. | `script` |
| `find` | Keep the first element where `field` matches `value` (optionally via `op`). | `field`, `value` |
| `filter` | Keep all elements where `field op value` is true. | `field`, `op`, `value` |
| `sort` | Reorder the elements by `field`. | `field` |
| `compute` | Add a derived `field` to every element from a `template`. | `field`, `template` |
| `follow` | For each element, fetch the URL in `field` and extract `selector` on that page. Flattens the matches into state. | `selector` |
| `set` | Store a literal `value` into a named `var`. | `var`, `value` |
| `map` | Collect `field` from all elements into a named `var` (comma-joined). | `field`, `var` |
| `alert` | Emit a Slack alert. Trigger behaviour controlled by `on` (default: `any`). | `message` |
| `log` | Print the current element list to stdout. | - |

Optional on `query_all` and `follow`: `attrs` (extra HTML attributes to capture beyond the built-ins). Optional on `query_all`, `eval_json`, and `follow`: `wait` (seconds to pause after page load, default `3`).

### `sort`

Reorders the element list by `field`. `order` is `asc` (default) or `desc`; set `numeric = true` to compare values as numbers (using the same parser as the numeric ops, so `£1,299.00` sorts as `1299.0`). This keeps a `map` fingerprint stable regardless of how the page ordered the tiles.

```toml
[[steps]]
action  = "sort"
field   = "text"
order   = "asc"      # asc | desc
numeric = false      # true to sort by parsed number
```

### `compute`

Builds a new per-element field from a `template`, evaluated against each element's own fields and the current vars. Useful for readable labels or stable change-fingerprints without dropping into `eval_json`.

```toml
[[steps]]
action   = "compute"
field    = "label"               # written onto every element
template = "{text} @ {value}"    # same {name} placeholders as alert messages
```

### `follow`

For each element, fetches the URL in `field` (default `href`; relative links are resolved against the scraper `url`) and extracts `selector` on that page, one bounded level of link-walking. The element list is replaced by the flattened matches across every followed page. Each match also gains a `source` field holding the URL it came from. This is the route to detail-page data (sizes, per-variant stock) that a listing page doesn't expose.

```toml
[[steps]]
action   = "query_all"
selector = ".product-tile__link"   # listing tiles, each with an href

[[steps]]
action   = "follow"
field    = "href"                  # link to walk (default)
selector = ".size-selector option" # extracted on each detail page
attrs    = ["data-stock"]
```

### Alert triggers (`on`)

| Value | Behaviour |
|---|---|
| `any` (default) | Fire once when the element list is non-empty. Uses the first element's fields for template interpolation. |
| `each` | Fire one alert per element. |
| `empty` | Fire when the element list is empty. |
| `change` | Fire when the watched value changes from its previous value. State is persisted between runs. Use `default` to set the assumed initial value. |
| `added` | Fire once per element whose identity `field` was not present last run. The new element's fields are available to the message. |
| `removed` | Fire once per identity `field` present last run but now gone. Only the stored key is available, exposed under the field's name. |
| `decrease` | Fire when the watched value, parsed as a number, is strictly lower than last run (e.g. a price drop). |
| `increase` | Fire when the watched value, parsed as a number, is strictly higher than last run. |

`change`, `decrease`, and `increase` watch either `field` of the first element or, alternatively, a `var` (set `var = "models"` instead of `field`), so they can track an aggregate built by `map` rather than a single element. They persist state under `id`; set a stable `id` so the state survives reordering other steps. `added`/`removed` use `field` as the **identity key** and persist the set of keys seen. On the first run there is no prior state, so `change`/`added` fire once to establish a baseline; `decrease`/`increase` stay silent until they have two readings to compare.

### Persisted state

The last-seen value for every stateful alert is written back into the scraper's own file as a `[state]` table at the bottom, keyed by alert `id`:

```toml
[state]
rose-backroad-al-59-set = "Rose Backroad AL APEX XPLR 1x12, ..."
```

Writes are format-preserving, so your comments, step order, and layout above are left untouched. Inspect the table to see what the scraper last observed, or delete keys (or the whole table) to reset a baseline and re-fire on the next run. `scrapers/` is gitignored, so state never gets committed.

---

## Op reference

| Op | Description |
|---|---|
| `eq` | Exact equality |
| `not_eq` | Not equal |
| `contains` | Field contains value as a substring |
| `not_contains` | Field does not contain value |
| `starts_with` | Field starts with value |
| `ends_with` | Field ends with value |
| `matches` | Field matches value as a regular expression |
| `gt` | Field, parsed as a number, is greater than value |
| `lt` | Field, parsed as a number, is less than value |
| `gte` | Field, parsed as a number, is greater than or equal to value |
| `lte` | Field, parsed as a number, is less than or equal to value |

Numeric ops ignore currency symbols and thousands separators, so a scraped
price like `£1,299.00` compares as `1299.0`. They don't match if either side
isn't a number (e.g. `"sold out"`).

In `filter` and `find`, the `value` supports `{field}` interpolation against the
element being tested, so you can compare one field to another, not just to a
literal. For example, keep only genuinely discounted items:

```toml
[[steps]]
action = "filter"
field  = "current_price"
op     = "lt"
value  = "{old_price}"   # current price below the struck-through price
```

---

## Template variables

Alert messages support `{var}` placeholders:

| Variable | Source |
|---|---|
| `{url}` | Scraper `url` field |
| `{text}` | `text` of the matched element |
| `{class}` | `class` of the matched element |
| `{href}` | `href` of the matched element |
| `{value}` | `value` of the matched element |
| `{MY_VAR}` | Any key from `[vars]` in `fijit.toml` or stored by `set`/`map` |

In `[on_error]` messages only:

| Variable | Source |
|---|---|
| `{name}` | Scraper name |
| `{error}` | Error message |

---

## CLI reference

```
fijit list                                   # show all scrapers
fijit run <name>                             # run once
fijit test-notify                            # send a test Slack message
fijit schedule <name>                        # add to crontab (default: */30 * * * *)
fijit schedule <name> --cron "0 9 * * *"    # daily at 9am
fijit unschedule <name>                      # remove from crontab
fijit init-config                            # print an example fijit.toml
```

## Scheduling

`fijit schedule` writes an entry to the user's crontab. Run it from the directory that contains `fijit.toml`. The entry records the working directory and uses the absolute path to the binary, so it works correctly when cron runs it later. Output is logged to `/var/log/fijit/<name>.log`. Create the directory once before scheduling:

```bash
sudo mkdir -p /var/log/fijit && sudo chown $(whoami) /var/log/fijit
```

Common cron expressions:

| Expression | Meaning |
|---|---|
| `*/30 * * * *` | Every 30 minutes |
| `0 * * * *` | Every hour |
| `0 9 * * *` | Daily at 9am |
| `0 9 * * 1-5` | Weekdays at 9am |

To see scheduled scrapers: `crontab -l`

To remove one: `fijit unschedule <name>`

---

## Building from source

```bash
git clone https://github.com/psedge/fijit
cd fijit
cargo build --release
# binary at: target/release/fijit
```

Requires Rust stable (>=1.75). Cross-compilation targets (`aarch64-apple-darwin`, `x86_64-apple-darwin`, `aarch64-unknown-linux-gnu`) are built automatically on release via GitHub Actions.

---

## License

MIT
