//! Error value construction.
//!
//! Errors in Elle are structs: `{:error :keyword :message "message"}`. This module provides
//! a helper to construct them using interned keywords.

use super::heap::TableKey;
use super::repr::Value;
use std::collections::BTreeMap;

/// Construct an error value: `{:error :keyword :message "message"}`
///
/// The kind string is interned as a keyword.
pub fn error_val(kind: &str, msg: impl Into<String>) -> Value {
    let mut fields = BTreeMap::new();
    fields.insert(TableKey::Keyword("error".into()), Value::keyword(kind));
    fields.insert(
        TableKey::Keyword("message".into()),
        Value::string(msg.into()),
    );
    Value::struct_from(fields)
}

/// Extract a human-readable error message from an error value.
///
/// Handles struct errors `{:error :keyword :message "string"}`, legacy array errors
/// `[:kind "msg"]` (for backward compatibility with user-constructed errors),
/// plain string errors, and arbitrary values.
/// Returns the formatted string representation.
pub fn format_error(value: Value) -> String {
    // Struct error: {:error :keyword :message "string"}
    if let Some(fields) = value.as_struct() {
        let error = fields.get(&TableKey::Keyword("error".into()));
        let msg = fields.get(&TableKey::Keyword("message".into()));
        if let (Some(error_val), Some(msg_val)) = (error, msg) {
            if let (Some(name), Some(text)) = (
                error_val.as_keyword_name(),
                msg_val.with_string(|s| s.to_string()),
            ) {
                return format!("{}: {}", name, text);
            }
        }
    }

    // Legacy array error: [:error "msg"] (backward compat for user-constructed errors)
    if let Some(elems) = value.as_array() {
        if elems.len() == 2 {
            if let Some(msg) = elems[1].with_string(|s| s.to_string()) {
                if let Some(name) = elems[0].as_keyword_name() {
                    return format!("{}: {}", name, msg);
                }
                return msg;
            }
        }
    }

    // Plain string error
    if let Some(s) = value.with_string(|s| s.to_string()) {
        return s;
    }

    // Fallback: display the value
    format!("{}", value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_val_creates_struct() {
        let err = error_val("type-error", "expected integer");

        // Should be a struct
        assert!(err.as_struct().is_some());

        // Should have :error and :message keys
        let fields = err.as_struct().unwrap();
        assert!(fields.contains_key(&TableKey::Keyword("error".into())));
        assert!(fields.contains_key(&TableKey::Keyword("message".into())));

        // Values should be correct
        let error_key = fields.get(&TableKey::Keyword("error".into())).unwrap();
        assert_eq!(error_key.as_keyword_name().as_deref(), Some("type-error"));

        let msg_key = fields.get(&TableKey::Keyword("message".into())).unwrap();
        assert_eq!(
            msg_key.with_string(|s| s.to_string()),
            Some("expected integer".to_string())
        );
    }

    #[test]
    fn test_format_error_struct() {
        let err = error_val("type-error", "expected integer");
        let formatted = format_error(err);
        assert_eq!(formatted, "type-error: expected integer");
    }

    #[test]
    fn test_format_error_legacy_array() {
        // Legacy array error for backward compatibility
        let err = Value::array(vec![
            Value::keyword("type-error"),
            Value::string("expected integer"),
        ]);
        let formatted = format_error(err);
        assert_eq!(formatted, "type-error: expected integer");
    }

    #[test]
    fn test_format_error_plain_string() {
        let err = Value::string("something went wrong");
        let formatted = format_error(err);
        assert_eq!(formatted, "something went wrong");
    }

    #[test]
    fn test_format_error_arbitrary_value() {
        let err = Value::int(42);
        let formatted = format_error(err);
        // Should fall back to display representation
        assert_eq!(formatted, "42");
    }

    #[test]
    fn test_format_error_struct_with_string_message() {
        let err = error_val("runtime-error", "division by zero");
        let formatted = format_error(err);
        assert_eq!(formatted, "runtime-error: division by zero");
    }
}
