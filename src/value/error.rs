//! Error value construction.
//!
//! Errors in Elle are tuples: `[:keyword "message"]`. This module provides
//! a helper to construct them, centralizing the keyword interning.

use super::repr::Value;

/// Construct an error value: `[:kind "message"]`
///
/// The kind string is interned as a keyword via the thread-local symbol table.
/// If no symbol table is available (e.g., during testing), falls back to a
/// plain string `"kind: message"`.
pub fn error_val(kind: &str, msg: impl Into<String>) -> Value {
    let msg_string = msg.into();
    unsafe {
        if let Some(symbols_ptr) = crate::ffi::primitives::context::get_symbol_table() {
            let id = (*symbols_ptr).intern(kind);
            Value::tuple(vec![Value::keyword(id.0), Value::string(msg_string)])
        } else {
            // Fallback: plain string (no symbol table in context)
            Value::string(format!("{}: {}", kind, msg_string))
        }
    }
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
                // Try to resolve keyword name
                if let Some(id) = elems[0].as_keyword() {
                    unsafe {
                        if let Some(symbols_ptr) =
                            crate::ffi::primitives::context::get_symbol_table()
                        {
                            if let Some(name) = (*symbols_ptr).name(crate::value::SymbolId(id)) {
                                return format!("{}: {}", name, msg);
                            }
                        }
                    }
                    return format!(":{}: {}", id, msg);
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
