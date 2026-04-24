//! Code formatter for Elle Lisp
//!
//! Opinionated formatter using Wadler-style pretty printing.
//! One canonical style. Zero configuration beyond line width and indent width.
//!
//! # Architecture
//!
//! ```text
//! Source → Lexer (with comments) → Syntax tree + CommentMap
//!                                      ↓
//!                               Doc generator
//!                                      ↓
//!                               Doc tree (Wadler algebra)
//!                                      ↓
//!                               Renderer (best(w, k, doc))
//!                                      ↓
//!                               Formatted string
//! ```
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

pub mod comments;
pub mod config;
pub mod core;
pub mod doc;
pub mod format;
pub mod forms;
pub mod render;
pub mod run;
pub mod trivia;

pub use config::FormatterConfig;
pub use core::format_code;
