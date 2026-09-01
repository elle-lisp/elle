//! Keyword argument extraction helpers for primitives.
//!
//! Provides `extract_keyword_timeout` for parsing optional `:timeout ms`
//! keyword arguments from primitive arg slices.

use crate::io::request::SocketOptions;
use crate::port::Encoding;
use crate::primitives::ctx::NativeCtx;
use crate::value::fiber::{SignalBits, SIG_ERROR};
use crate::value::Value;
use std::time::Duration;

/// Parsed keyword arguments for connect primitives.
pub(crate) struct ConnectKwargs {
    pub timeout: Option<Duration>,
    pub options: SocketOptions,
    /// Port encoding for the resulting stream.  `None` => caller's default
    /// (binary for raw socket primitives).  Explicit `:text` opts into
    /// grapheme-mode reads / `port/read-exact` graphemes / etc., for
    /// line-oriented text protocols (SMTP, IRC, plain HTTP/1.x).
    pub encoding: Option<Encoding>,
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
    ctx: &mut NativeCtx,
) -> Result<Option<Duration>, (SignalBits, Value)> {
    if args.len() <= start {
        return Ok(None);
    }

    let remaining = &args[start..];
    if !remaining.len().is_multiple_of(2) {
        return Err((
            SIG_ERROR,
            ctx.error(
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

        match ctx.keyword_spelling(*key).as_deref() {
            Some("timeout") => match val.as_int() {
                Some(ms) if ms >= 0 => {
                    timeout = Some(Duration::from_millis(ms as u64));
                }
                Some(ms) => {
                    return Err((
                        SIG_ERROR,
                        ctx.error(
                            "value-error",
                            format!("{}: :timeout must be non-negative, got {}", prim_name, ms),
                        ),
                    ));
                }
                None => {
                    return Err((
                        SIG_ERROR,
                        ctx.error(
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
                    ctx.error(
                        "value-error",
                        format!("{}: unknown keyword :{}", prim_name, other),
                    ),
                ));
            }
            None => {
                return Err((
                    SIG_ERROR,
                    ctx.error(
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
    ctx: &mut NativeCtx,
) -> Result<ConnectKwargs, (SignalBits, Value)> {
    let mut result = ConnectKwargs {
        timeout: None,
        options: SocketOptions::default(),
        encoding: None,
    };

    if args.len() <= start {
        return Ok(result);
    }

    let remaining = &args[start..];
    if !remaining.len().is_multiple_of(2) {
        return Err((
            SIG_ERROR,
            ctx.error(
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

        match ctx.keyword_spelling(*key).as_deref() {
            Some("timeout") => match val.as_int() {
                Some(ms) if ms >= 0 => {
                    result.timeout = Some(Duration::from_millis(ms as u64));
                }
                Some(ms) => {
                    return Err((
                        SIG_ERROR,
                        ctx.error(
                            "value-error",
                            format!("{}: :timeout must be non-negative, got {}", prim_name, ms),
                        ),
                    ));
                }
                None => {
                    return Err((
                        SIG_ERROR,
                        ctx.error(
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
                result.options.sndbuf = Some(extract_positive_int(val, "sndbuf", prim_name, ctx)?);
            }
            Some("rcvbuf") => {
                result.options.rcvbuf = Some(extract_positive_int(val, "rcvbuf", prim_name, ctx)?);
            }
            Some("nodelay") => {
                result.options.nodelay = Some(extract_bool(val, "nodelay", prim_name, ctx)?);
            }
            Some("keepalive") => {
                result.options.keepalive = Some(extract_bool(val, "keepalive", prim_name, ctx)?);
            }
            Some("encoding") => {
                let enc = match ctx.keyword_spelling(*val).as_deref() {
                    Some("text") => Encoding::Text,
                    Some("binary") => Encoding::Binary,
                    Some(other) => {
                        return Err((
                            SIG_ERROR,
                            ctx.error(
                                "value-error",
                                format!(
                                    "{}: :encoding must be :text or :binary, got :{}",
                                    prim_name, other
                                ),
                            ),
                        ));
                    }
                    None => {
                        return Err((
                            SIG_ERROR,
                            ctx.error(
                                "type-error",
                                format!(
                                    "{}: :encoding value must be keyword (:text or :binary), got {}",
                                    prim_name,
                                    val.type_name()
                                ),
                            ),
                        ));
                    }
                };
                result.encoding = Some(enc);
            }
            Some(other) => {
                return Err((
                    SIG_ERROR,
                    ctx.error(
                        "value-error",
                        format!("{}: unknown keyword :{}", prim_name, other),
                    ),
                ));
            }
            None => {
                return Err((
                    SIG_ERROR,
                    ctx.error(
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
    ctx: &mut NativeCtx,
) -> Result<i32, (SignalBits, Value)> {
    match val.as_int() {
        Some(n) if n > 0 && n <= i32::MAX as i64 => Ok(n as i32),
        Some(n) => Err((
            SIG_ERROR,
            ctx.error(
                "value-error",
                format!(
                    "{}: :{} must be a positive integer, got {}",
                    prim_name, name, n
                ),
            ),
        )),
        None => Err((
            SIG_ERROR,
            ctx.error(
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

fn extract_bool(
    val: &Value,
    name: &str,
    prim_name: &str,
    ctx: &mut NativeCtx,
) -> Result<bool, (SignalBits, Value)> {
    match val.as_bool() {
        Some(b) => Ok(b),
        None => Err((
            SIG_ERROR,
            ctx.error(
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

// Tests migrated to tests/elle/prim-kwarg.lisp
