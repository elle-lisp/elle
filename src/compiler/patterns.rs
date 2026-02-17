use super::ast::Pattern;
use crate::symbol::SymbolTable;
use crate::value::Value;

/// Convert a value to a pattern for pattern matching
pub fn value_to_pattern(value: &Value, symbols: &SymbolTable) -> Result<Pattern, String> {
    if value.is_nil() || value.is_empty_list() {
        Ok(Pattern::Nil)
    } else if let Some(id) = value.as_symbol() {
        // Check if it's a wildcard
        let sym_id = crate::value::SymbolId(id);
        if let Some(name) = symbols.name(sym_id) {
            if name == "_" {
                return Ok(Pattern::Wildcard);
            }
        }
        // Otherwise it's a variable binding
        Ok(Pattern::Var(sym_id))
    } else if value.is_int() || value.is_float() || value.is_bool() || value.is_string() {
        Ok(Pattern::Literal(*value))
    } else if value.is_cons() {
        let vec = value.list_to_vec()?;
        if vec.is_empty() {
            Ok(Pattern::Nil)
        } else {
            // List pattern: (1 2 3) matches a list with elements 1, 2, 3
            // Single-element list (1) matches a list with one element 1
            let patterns: Result<Vec<_>, _> =
                vec.iter().map(|v| value_to_pattern(v, symbols)).collect();
            Ok(Pattern::List(patterns?))
        }
    } else {
        Err(format!("Cannot convert {:?} to pattern", value))
    }
}
