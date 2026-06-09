use anyhow::Result;
use fijjit_core::{
    obscura::ObscuraRunner,
    scraper::{ScrapeResult, Scraper},
};
use serde::Deserialize;

const URL: &str = "https://www.bike-discount.de/en/cube-editor-pro-oatgrey-n-grey";
const TARGET_SIZE: &str = "58 cm";
const IN_STOCK_CLASS: &str = "--stock-1";

const EVAL: &str = r#"JSON.stringify(
  Array.from(document.querySelectorAll('.nele-product-detail-configurator-option.nele-stock-info'))
  .map(el => ({text: el.textContent.trim().split('\n')[0].trim(), class: el.className}))
)"#;

#[derive(Deserialize)]
struct Variant {
    text: String,
    class: String,
}

pub struct BikeDiscountScraper {
    runner: ObscuraRunner,
}

impl BikeDiscountScraper {
    pub fn new(obscura_path: &str) -> Self {
        Self {
            runner: ObscuraRunner::new(obscura_path),
        }
    }
}

impl Scraper for BikeDiscountScraper {
    fn name(&self) -> &str {
        "bike-discount"
    }

    fn description(&self) -> &str {
        "Watches bike-discount.de for Cube Editor Pro 58cm availability"
    }

    fn check(&self) -> Result<ScrapeResult> {
        let json = self.runner.eval_json(URL, EVAL, 8)?;
        let variants: Vec<Variant> = serde_json::from_str(&json)?;

        for v in &variants {
            let status = stock_label(&v.class);
            println!("  {:<8} {}", v.text, status);
        }

        let target = variants.iter().find(|v| v.text == TARGET_SIZE);
        match target {
            Some(v) if v.class.contains(IN_STOCK_CLASS) => Ok(ScrapeResult::Alert(format!(
                "🚲 *{TARGET_SIZE} Cube Editor Pro oatgrey'n'grey is IN STOCK!*\n<{URL}|Buy it now on bike-discount.de>"
            ))),
            _ => Ok(ScrapeResult::NoChange),
        }
    }
}

fn stock_label(class: &str) -> &str {
    if class.contains("--stock-1") {
        "✓ in stock"
    } else if class.contains("--stock-4") {
        "~ 6+ weeks"
    } else {
        "✗ unavailable"
    }
}
