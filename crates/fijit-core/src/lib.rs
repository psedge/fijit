#![deny(unsafe_code)]
#![warn(missing_docs, clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

//! Core traits and utilities for the fijit scraper framework.

/// Global configuration loaded from `fijit.toml`.
pub mod config;
/// DOM element type used as pipeline state.
pub mod element;
/// Typed error enum for the core library.
pub mod error;
/// Slack notification helpers.
pub mod notify;
/// Wrapper around the Obscura headless browser binary.
pub mod obscura;
/// Config-driven pipeline executor and scraper loader.
pub mod pipeline;
/// `Scraper` trait and `ScrapeResult` type.
pub mod scraper;
/// Declarative pipeline step types.
pub mod step;

pub use error::Error;
