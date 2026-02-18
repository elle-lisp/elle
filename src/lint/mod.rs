//! Linting infrastructure for Elle
//!
//! Pipeline-agnostic diagnostic types and linting rules.

pub mod diagnostics;
pub mod rules;

pub use diagnostics::{Diagnostic, DiagnosticContext, Severity};
pub use rules::{builtin_arity, check_call_arity, check_naming_convention};
