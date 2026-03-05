//! Source-to-source rewriting engine.
//!
//! Token-level rewrite tool that performs mechanical source transformations
//! while preserving comments, whitespace, and formatting.

pub mod edit;
pub mod engine;
pub mod rule;
pub mod run;
