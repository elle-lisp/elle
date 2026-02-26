//! Linting infrastructure for Elle
//!
//! Pipeline-agnostic diagnostic types, linting rules, and CLI wrapper.

pub mod cli;
pub mod diagnostics;
pub mod rules;
pub mod run;

pub use cli::{LintConfig, Linter, OutputFormat};
pub use diagnostics::{Diagnostic, DiagnosticContext, Severity};
pub use rules::check_naming_convention;
