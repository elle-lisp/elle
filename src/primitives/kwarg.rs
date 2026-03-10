//! Keyword argument extraction helpers for primitives.
//!
//! Provides `extract_keyword_timeout` for parsing optional `:timeout ms`
//! keyword arguments from primitive arg slices.

use crate::value::fiber::{SignalBits, SIG_ERROR};
use crate::value::{error_val, Value};
use std::time::Duration;

/// Scan args starting at `start` for keyword-value pairs.
///
/// Currently recognizes `:timeout ms` (non-negative integer).
/// Returns `Ok(None)` if `:timeout` is absent.
/// Returns `Err` on bad keyword, missing value, or bad type.
pub(crate) fn extract_keyword_timeout(
    args: &[Value],
    start: usize,
    prim_name: &str,
) -> Result<Option<Duration>, (SignalBits, Value)> {
    if args.len() <= start {
        return Ok(None);
    }

    let remaining = &args[start..];
    if !remaining.len().is_multiple_of(2) {
        return Err((
            SIG_ERROR,
            error_val(
                "arity-error",
                format!(
                    "{}: keyword arguments must be key-value pairs, got odd count",
                    prim_name
                ),
            ),
        ));
    }

    let mut timeout = None;
    let mut i = 0;
    while i < remaining.len() {
        let key = &remaining[i];
        let val = &remaining[i + 1];

        match key.as_keyword_name() {
            Some("timeout") => match val.as_int() {
                Some(ms) if ms >= 0 => {
                    timeout = Some(Duration::from_millis(ms as u64));
                }
                Some(ms) => {
                    return Err((
                        SIG_ERROR,
                        error_val(
                            "value-error",
                            format!("{}: :timeout must be non-negative, got {}", prim_name, ms),
                        ),
                    ));
                }
                None => {
                    return Err((
                        SIG_ERROR,
                        error_val(
                            "type-error",
                            format!(
                                "{}: :timeout value must be integer, got {}",
                                prim_name,
                                val.type_name()
                            ),
                        ),
                    ));
                }
            },
            Some(other) => {
                return Err((
                    SIG_ERROR,
                    error_val(
                        "value-error",
                        format!("{}: unknown keyword :{}", prim_name, other),
                    ),
                ));
            }
            None => {
                return Err((
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("{}: expected keyword, got {}", prim_name, key.type_name()),
                    ),
                ));
            }
        }
        i += 2;
    }

    Ok(timeout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_timeout_present() {
        let args = [Value::keyword("timeout"), Value::int(5000)];
        let result = extract_keyword_timeout(&args, 0, "test").unwrap();
        assert_eq!(result, Some(Duration::from_millis(5000)));
    }

    #[test]
    fn test_extract_timeout_absent() {
        let args: [Value; 0] = [];
        let result = extract_keyword_timeout(&args, 0, "test").unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_timeout_non_keyword_errors() {
        let args = [Value::int(5000)];
        // Odd count → arity error
        let result = extract_keyword_timeout(&args, 0, "test");
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_timeout_missing_value_errors() {
        let args = [Value::keyword("timeout")];
        let result = extract_keyword_timeout(&args, 0, "test");
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_timeout_negative_errors() {
        let args = [Value::keyword("timeout"), Value::int(-1)];
        let result = extract_keyword_timeout(&args, 0, "test");
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_timeout_bad_type_errors() {
        let args = [Value::keyword("timeout"), Value::string("foo")];
        let result = extract_keyword_timeout(&args, 0, "test");
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_unknown_keyword_errors() {
        let args = [Value::keyword("foo"), Value::int(1)];
        let result = extract_keyword_timeout(&args, 0, "test");
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_timeout_with_offset() {
        let args = [
            Value::string("positional"),
            Value::keyword("timeout"),
            Value::int(3000),
        ];
        let result = extract_keyword_timeout(&args, 1, "test").unwrap();
        assert_eq!(result, Some(Duration::from_millis(3000)));
    }

    #[test]
    fn test_extract_no_kwargs_with_offset() {
        let args = [Value::string("positional")];
        let result = extract_keyword_timeout(&args, 1, "test").unwrap();
        assert_eq!(result, None);
    }
}
