//! Arity validation rules

use crate::diagnostics::Diagnostic;
use elle::value::Value;

/// Check function call arities
pub fn check_arity(
    _value: &Value,
    _filename: &str,
    _line: usize,
    _diagnostics: &mut Vec<Diagnostic>,
    _symbols: &elle::SymbolTable,
) {
    // This is a placeholder for now
    // Full implementation would require tracking function definitions
}
