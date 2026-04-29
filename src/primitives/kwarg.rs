//! Keyword argument extraction helpers for primitives.
//!
//! Provides `extract_keyword_timeout` for parsing optional `:timeout ms`
//! keyword arguments from primitive arg slices.

use crate::io::request::SocketOptions;
use crate::value::fiber::{SignalBits, SIG_ERROR};
use crate::value::{error_val, Value};
use std::time::Duration;

/// Parsed keyword arguments for connect primitives.
pub(crate) struct ConnectKwargs {
    pub timeout: Option<Duration>,
    pub options: SocketOptions,
}

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

        match key.as_keyword_name().as_deref() {
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

/// Extract connect keyword arguments: `:timeout`, `:sndbuf`, `:rcvbuf`, `:nodelay`, `:keepalive`.
///
/// Returns `ConnectKwargs` with parsed socket options.
pub(crate) fn extract_connect_kwargs(
    args: &[Value],
    start: usize,
    prim_name: &str,
) -> Result<ConnectKwargs, (SignalBits, Value)> {
    let mut result = ConnectKwargs {
        timeout: None,
        options: SocketOptions::default(),
    };

    if args.len() <= start {
        return Ok(result);
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

    let mut i = 0;
    while i < remaining.len() {
        let key = &remaining[i];
        let val = &remaining[i + 1];

        match key.as_keyword_name().as_deref() {
            Some("timeout") => match val.as_int() {
                Some(ms) if ms >= 0 => {
                    result.timeout = Some(Duration::from_millis(ms as u64));
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
            Some("sndbuf") => {
                result.options.sndbuf = Some(extract_positive_int(val, "sndbuf", prim_name)?);
            }
            Some("rcvbuf") => {
                result.options.rcvbuf = Some(extract_positive_int(val, "rcvbuf", prim_name)?);
            }
            Some("nodelay") => {
                result.options.nodelay = Some(extract_bool(val, "nodelay", prim_name)?);
            }
            Some("keepalive") => {
                result.options.keepalive = Some(extract_bool(val, "keepalive", prim_name)?);
            }
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

    Ok(result)
}

fn extract_positive_int(
    val: &Value,
    name: &str,
    prim_name: &str,
) -> Result<i32, (SignalBits, Value)> {
    match val.as_int() {
        Some(n) if n > 0 && n <= i32::MAX as i64 => Ok(n as i32),
        Some(n) => Err((
            SIG_ERROR,
            error_val(
                "value-error",
                format!(
                    "{}: :{} must be a positive integer, got {}",
                    prim_name, name, n
                ),
            ),
        )),
        None => Err((
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "{}: :{} value must be integer, got {}",
                    prim_name,
                    name,
                    val.type_name()
                ),
            ),
        )),
    }
}

fn extract_bool(val: &Value, name: &str, prim_name: &str) -> Result<bool, (SignalBits, Value)> {
    match val.as_bool() {
        Some(b) => Ok(b),
        None => Err((
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "{}: :{} value must be boolean, got {}",
                    prim_name,
                    name,
                    val.type_name()
                ),
            ),
        )),
    }
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

    // --- ConnectKwargs tests ---

    #[test]
    fn test_connect_kwargs_sndbuf() {
        let args = [Value::keyword("sndbuf"), Value::int(1048576)];
        let result = extract_connect_kwargs(&args, 0, "test").unwrap();
        assert_eq!(result.options.sndbuf, Some(1048576));
        assert!(result.timeout.is_none());
    }

    #[test]
    fn test_connect_kwargs_combined() {
        let args = [
            Value::keyword("sndbuf"),
            Value::int(2097152),
            Value::keyword("timeout"),
            Value::int(5000),
        ];
        let result = extract_connect_kwargs(&args, 0, "test").unwrap();
        assert_eq!(result.options.sndbuf, Some(2097152));
        assert_eq!(result.timeout, Some(Duration::from_millis(5000)));
    }

    #[test]
    fn test_connect_kwargs_nodelay() {
        let args = [Value::keyword("nodelay"), Value::TRUE];
        let result = extract_connect_kwargs(&args, 0, "test").unwrap();
        assert_eq!(result.options.nodelay, Some(true));
    }

    #[test]
    fn test_connect_kwargs_negative_sndbuf_errors() {
        let args = [Value::keyword("sndbuf"), Value::int(-1)];
        assert!(extract_connect_kwargs(&args, 0, "test").is_err());
    }

    #[test]
    fn test_connect_kwargs_string_sndbuf_errors() {
        let args = [Value::keyword("sndbuf"), Value::string("foo")];
        assert!(extract_connect_kwargs(&args, 0, "test").is_err());
    }

    #[test]
    fn test_connect_kwargs_unknown_keyword_errors() {
        let args = [Value::keyword("bogus"), Value::int(1)];
        assert!(extract_connect_kwargs(&args, 0, "test").is_err());
    }
}
