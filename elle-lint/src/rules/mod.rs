//! Linting rules for Elle Lisp

use crate::diagnostics::Diagnostic;
use elle::value::Value;

pub mod arity;
pub mod naming;

pub use arity::check_arity;
pub use naming::check_naming_conventions;

/// Check all rules for a value
pub fn check_all(
    value: &Value,
    filename: &str,
    line: usize,
    diagnostics: &mut Vec<Diagnostic>,
    symbols: &elle::SymbolTable,
) {
    check_naming_conventions(value, filename, line, diagnostics, symbols);
    check_arity(value, filename, line, diagnostics, symbols);
}
