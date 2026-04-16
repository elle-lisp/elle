//! Error value construction.
//!
//! Errors in Elle are structs: `{:error :keyword :message "message"}`. This module provides
//! a helper to construct them using interned keywords.

use super::heap::TableKey;
use super::repr::Value;
use super::types::sorted_struct_get;
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

/// Construct an error value with extra context fields:
/// `{:error :keyword :message "message" :key1 val1 ...}`
///
/// Identical to `error_val` when `extra` is empty.
/// Extra fields are passed through `format_error` unchanged;
/// it reads only `:error` and `:message`.
///
/// `Value` is `Copy`, so `&[(&str, Value)]` works without ownership complications.
pub fn error_val_extra(kind: &str, msg: impl Into<String>, extra: &[(&str, Value)]) -> Value {
    let mut fields = BTreeMap::new();
    fields.insert(TableKey::Keyword("error".into()), Value::keyword(kind));
    fields.insert(
        TableKey::Keyword("message".into()),
        Value::string(msg.into()),
    );
    for (key, val) in extra {
        fields.insert(TableKey::Keyword((*key).into()), *val);
    }
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
        let error = sorted_struct_get(fields, &TableKey::Keyword("error".into()));
        let msg = sorted_struct_get(fields, &TableKey::Keyword("message".into()));
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
    use crate::value::types::sorted_struct_contains;

    #[test]
    fn test_error_val_creates_struct() {
        let err = error_val("type-error", "expected integer");

        // Should be a struct
        assert!(err.as_struct().is_some());

        // Should have :error and :message keys
        let fields = err.as_struct().unwrap();
        assert!(sorted_struct_contains(
            fields,
            &TableKey::Keyword("error".into())
        ));
        assert!(sorted_struct_contains(
            fields,
            &TableKey::Keyword("message".into())
        ));

        // Values should be correct
        let error_key = sorted_struct_get(fields, &TableKey::Keyword("error".into())).unwrap();
        assert_eq!(error_key.as_keyword_name().as_deref(), Some("type-error"));

        let msg_key = sorted_struct_get(fields, &TableKey::Keyword("message".into())).unwrap();
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

    #[test]
    fn test_error_val_extra_creates_struct() {
        let err = error_val_extra(
            "io-error",
            "slurp: failed to read '/no/such': file not found",
            &[("path", Value::string("/no/such"))],
        );
        let fields = err.as_struct().unwrap();
        // :error keyword correct
        assert_eq!(
            sorted_struct_get(fields, &TableKey::Keyword("error".into()))
                .unwrap()
                .as_keyword_name()
                .as_deref(),
            Some("io-error"),
        );
        // :message correct
        assert!(sorted_struct_contains(
            fields,
            &TableKey::Keyword("message".into())
        ));
        // :path extra field present
        let path_val = sorted_struct_get(fields, &TableKey::Keyword("path".into())).unwrap();
        assert_eq!(
            path_val.with_string(|s| s.to_string()),
            Some("/no/such".to_string()),
        );
    }

    #[test]
    fn test_error_val_extra_empty_extras_matches_error_val() {
        let a = error_val("type-error", "expected integer");
        let b = error_val_extra("type-error", "expected integer", &[]);
        // Both produce identical structs
        assert_eq!(a, b);
    }

    #[test]
    fn test_format_error_ignores_extra_fields() {
        let err = error_val_extra(
            "io-error",
            "slurp: failed to read '/tmp/x': not found",
            &[("path", Value::string("/tmp/x"))],
        );
        let formatted = format_error(err);
        // format_error reads :error and :message; extra fields are silently ignored
        assert_eq!(
            formatted,
            "io-error: slurp: failed to read '/tmp/x': not found"
        );
    }
}
