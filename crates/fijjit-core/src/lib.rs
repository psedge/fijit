#![deny(unsafe_code)]
#![warn(missing_docs, clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

//! Core traits and utilities for the fijjit scraper framework.

pub mod config;
pub mod error;
pub mod notify;
pub mod obscura;
pub mod scraper;

pub use error::Error;
