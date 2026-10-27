// audited: 2026-10-25
//! Code formatter for Elle Lisp
//!
//! src/formatter/README.md
//!
//! Opinionated formatter using Wadler-style pretty printing.
//! One canonical style. Zero configuration beyond line width and indent width.
//! The pipeline from source to formatted text is in the README.
//!
//! # Usage
//!
//! ```ignore
//! use elle::formatter::{format_code, FormatterConfig};
//!
//! let config = FormatterConfig::default();
//! let formatted = format_code("(+ 1 2)", &config)?;
//! println!("{}", formatted);
//! ```

pub mod columns;
pub mod comments;
pub mod config;
pub mod core;
pub mod doc;
pub mod format;
pub mod forms;
pub mod metrics;
pub mod pos;
pub mod render;
pub mod run;
pub mod trivia;

pub use config::FormatterConfig;
pub use core::format_code;
