use super::ast::Pattern;
use crate::symbol::SymbolTable;
use crate::value::Value;

/// Convert a value to a pattern for pattern matching
pub fn value_to_pattern(value: &Value, symbols: &SymbolTable) -> Result<Pattern, String> {
    match value {
        Value::Nil => Ok(Pattern::Nil),
        Value::Symbol(id) => {
            // Check if it's a wildcard
            if let Some(name) = symbols.name(*id) {
                if name == "_" {
                    return Ok(Pattern::Wildcard);
                }
            }
            // Otherwise it's a variable binding
            Ok(Pattern::Var(*id))
        }
        _ if matches!(
            value,
            Value::Int(_) | Value::Float(_) | Value::Bool(_) | Value::String(_)
        ) =>
        {
            Ok(Pattern::Literal(value.clone()))
        }
        Value::Cons(_) => {
            let vec = value.list_to_vec()?;
            if vec.is_empty() {
                Ok(Pattern::Nil)
            } else if vec.len() == 1 {
                // Single-element list is just that pattern (unwrap it)
                // e.g., (1) in (match 2 ((1) "one")) becomes just the pattern 1
                value_to_pattern(&vec[0], symbols)
            } else {
                // Multi-element list is a list pattern
                let patterns: Result<Vec<_>, _> =
                    vec.iter().map(|v| value_to_pattern(v, symbols)).collect();
                Ok(Pattern::List(patterns?))
            }
        }
        _ => Err(format!("Cannot convert {:?} to pattern", value)),
    }
}
