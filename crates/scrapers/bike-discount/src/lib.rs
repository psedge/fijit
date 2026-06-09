#![deny(unsafe_code)]

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stock_label_in_stock() {
        assert_eq!(
            stock_label(
                "nele-product-detail-configurator-option nele-stock-info dropdown-item --stock-1"
            ),
            "✓ in stock"
        );
    }

    #[test]
    fn stock_label_backordered() {
        assert_eq!(
            stock_label(
                "nele-product-detail-configurator-option nele-stock-info dropdown-item --stock-4"
            ),
            "~ 6+ weeks"
        );
    }

    #[test]
    fn stock_label_unavailable() {
        assert_eq!(stock_label("nele-stock-info --stock-99"), "✗ unavailable");
    }

    #[test]
    fn parses_variant_json() {
        let json = r#"[
            {"text": "54 cm", "class": "nele-stock-info --stock-1"},
            {"text": "58 cm", "class": "nele-stock-info --stock-4"}
        ]"#;
        let variants: Vec<Variant> = serde_json::from_str(json).unwrap();
        assert_eq!(variants.len(), 2);
        assert_eq!(variants[0].text, "54 cm");
        assert!(variants[1].class.contains("--stock-4"));
    }

    #[test]
    fn no_alert_when_target_backordered() {
        let variants: Vec<Variant> = serde_json::from_str(
            r#"[
            {"text": "58 cm", "class": "nele-stock-info --stock-4"}
        ]"#,
        )
        .unwrap();
        let target = variants.iter().find(|v| v.text == TARGET_SIZE);
        assert!(matches!(target, Some(v) if !v.class.contains(IN_STOCK_CLASS)));
    }

    #[test]
    fn alert_when_target_in_stock() {
        let variants: Vec<Variant> = serde_json::from_str(
            r#"[
            {"text": "58 cm", "class": "nele-stock-info --stock-1"}
        ]"#,
        )
        .unwrap();
        let target = variants.iter().find(|v| v.text == TARGET_SIZE);
        assert!(matches!(target, Some(v) if v.class.contains(IN_STOCK_CLASS)));
    }
}
