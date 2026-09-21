// audited: 2026-09-21
//! Source-to-source rewriting engine.
//!
//! Token-level rewrite tool that performs mechanical source transformations
//! while preserving comments, whitespace, and formatting.

pub mod edit;
pub mod engine;
pub(crate) mod library;
pub mod rule;
pub mod run;
pub mod text;
