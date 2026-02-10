//! Code formatter for Elle Lisp
//!
//! Provides pretty-printing and formatting functionality for Elle source code.
//!
//! # Examples
//!
//! ```ignore
//! use elle::formatter::{format_code, FormatterConfig};
//!
//! let config = FormatterConfig::default();
//! let formatted = format_code("(+ 1 2)", config)?;
//! println!("{}", formatted);
//! ```

pub mod config;
pub mod core;

pub use config::FormatterConfig;
pub use core::format_code;
