//! Error value construction.
//!
//! Errors in Elle are tuples: `[:keyword "message"]`. This module provides
//! a helper to construct them using interned keywords.

use super::repr::Value;

/// Construct an error value: `[:kind "message"]`
///
/// The kind string is interned as a keyword.
pub fn error_val(kind: &str, msg: impl Into<String>) -> Value {
    Value::tuple(vec![Value::keyword(kind), Value::string(msg.into())])
}

/// Extract a human-readable error message from an error value.
///
/// Handles both tuple errors `[:kind "msg"]` and plain string errors.
/// Returns the formatted string representation.
pub fn format_error(value: Value) -> String {
    // Tuple error: [:kind "msg"]
    if let Some(elems) = value.as_tuple() {
        if elems.len() == 2 {
            if let Some(msg) = elems[1].as_string() {
                if let Some(name) = elems[0].as_keyword_name() {
                    return format!("{}: {}", name, msg);
                }
                return msg.to_string();
            }
        }
    }

    // Plain string error
    if let Some(s) = value.as_string() {
        return s.to_string();
    }

    // Fallback: display the value
    format!("{}", value)
}
