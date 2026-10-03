// audited: 2026-09-30
//! The keyword arguments the I/O primitives take: the bounds every wait takes, and the socket options a connect accepts.
//!
//! docs/io/timeout.md
//! docs/io.md
//!
//! tests/lang/prim-kwarg.lisp and tests/lang/port-deadline.lisp pin them.

use crate::io::request::{Bound, SocketOptions};
use crate::port::Encoding;
use crate::primitives::ctx::NativeCtx;
use crate::primitives::time::instant_at;
use crate::value::fiber::{SignalBits, SIG_ERROR};
use crate::value::Value;
use std::time::{Duration, Instant};

/// What a primitive answers when it refuses an argument.
type Refusal = (SignalBits, Value);

/// Parsed keyword arguments for connect primitives.
pub(crate) struct ConnectKwargs {
    pub bound: Bound,
    pub options: SocketOptions,
    /// Port encoding for the resulting stream.  `None` => caller's default
    /// (binary for raw socket primitives).  Explicit `:text` opts into
    /// grapheme-mode reads / `port/read-exact` graphemes / etc., for
    /// line-oriented text protocols (SMTP, IRC, plain HTTP/1.x).
    pub encoding: Option<Encoding>,
}

/// What a primitive answers when an argument is not a span of seconds.
type SecondsRefusal = (&'static str, String);

/// A span of seconds an argument names, as an int or a float.
///
/// Every primitive that takes a duration refuses the same values: not a number,
/// negative, infinite, or longer than the clock can count from now
/// (docs/io/timeout.md). The error names `what`, the caller, so a refusal
/// reads as that primitive's.
pub(crate) fn seconds(value: &Value, what: &str) -> Result<Duration, SecondsRefusal> {
    let secs = if let Some(n) = value.as_int() {
        n as f64
    } else if let Some(f) = value.as_float() {
        f
    } else {
        return Err((
            "type-error",
            format!("{}: expected a number of seconds", what),
        ));
    };
    if secs < 0.0 || !secs.is_finite() {
        return Err((
            "argument-error",
            format!("{}: seconds must be finite and non-negative", what),
        ));
    }
    Duration::try_from_secs_f64(secs)
        .ok()
        .filter(|span| Instant::now().checked_add(*span).is_some())
        .ok_or_else(|| {
            (
                "argument-error",
                format!("{}: seconds must be within what the clock can count", what),
            )
        })
}

/// A `:timeout` value: a span of seconds, or `None` for `nil`.
pub(crate) fn timeout_arg(
    val: Value,
    prim_name: &str,
    ctx: &mut NativeCtx,
) -> Result<Option<Duration>, Refusal> {
    if val.is_nil() {
        return Ok(None);
    }
    seconds(&val, &format!("{} :timeout", prim_name))
        .map(Some)
        .map_err(|(kind, msg)| (SIG_ERROR, ctx.error(kind, msg)))
}

/// A `:deadline` value: a `(clock/monotonic)` reading, or `nil` for none.
fn deadline_arg(
    val: Value,
    prim_name: &str,
    ctx: &mut NativeCtx,
) -> Result<Option<Instant>, Refusal> {
    if val.is_nil() {
        return Ok(None);
    }
    let reading = if let Some(n) = val.as_int() {
        n as f64
    } else if let Some(f) = val.as_float() {
        f
    } else {
        return Err((
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!(
                    "{} :deadline: expected a (clock/monotonic) reading, got {}",
                    prim_name,
                    val.type_name()
                ),
            ),
        ));
    };
    if reading.is_nan() {
        return Err((
            SIG_ERROR,
            ctx.error(
                "argument-error",
                format!("{} :deadline: NaN is not a reading", prim_name),
            ),
        ));
    }
    match instant_at(reading) {
        Some(until) => Ok(Some(until)),
        None => Err((
            SIG_ERROR,
            ctx.error(
                "argument-error",
                format!(
                    "{} :deadline: a reading must be within what the clock can count",
                    prim_name
                ),
            ),
        )),
    }
}

/// Walk the keyword-value pairs from `args[start]` on, handing each keyword's
/// spelling and its value to `take`. `take` answers whether it knows the
/// keyword; one it does not know is refused here, as is an odd count or a key
/// that is not a keyword.
pub(crate) fn each_keyword(
    args: &[Value],
    start: usize,
    prim_name: &str,
    ctx: &mut NativeCtx,
    mut take: impl FnMut(&str, Value, &mut NativeCtx) -> Result<bool, Refusal>,
) -> Result<(), Refusal> {
    let remaining = args.get(start..).unwrap_or(&[]);
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
    for &[key, val] in remaining.as_chunks::<2>().0 {
        let Some(spelling) = ctx.keyword_spelling(key) else {
            return Err((
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    format!("{}: expected keyword, got {}", prim_name, key.type_name()),
                ),
            ));
        };
        if !take(&spelling, val, ctx)? {
            return Err((
                SIG_ERROR,
                ctx.error(
                    "value-error",
                    format!("{}: unknown keyword :{}", prim_name, spelling),
                ),
            ));
        }
    }
    Ok(())
}

/// The `:timeout` and `:deadline` a call names, gathered as its keywords go by.
#[derive(Default)]
struct BoundArgs {
    timeout: Option<Duration>,
    until: Option<Instant>,
}

impl BoundArgs {
    /// Take `key` when it names a bound. Answers whether it did.
    fn take(
        &mut self,
        key: &str,
        val: Value,
        prim_name: &str,
        ctx: &mut NativeCtx,
    ) -> Result<bool, Refusal> {
        match key {
            "timeout" => self.timeout = timeout_arg(val, prim_name, ctx)?,
            "deadline" => self.until = deadline_arg(val, prim_name, ctx)?,
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn bound(self) -> Bound {
        Bound::new(self.timeout, self.until)
    }
}

/// The bound named by the keyword arguments from `args[start]` on: `:timeout`
/// and `:deadline`, and nothing else.
pub(crate) fn extract_bound(
    args: &[Value],
    start: usize,
    prim_name: &str,
    ctx: &mut NativeCtx,
) -> Result<Bound, Refusal> {
    let mut bounds = BoundArgs::default();
    each_keyword(args, start, prim_name, ctx, |key, val, ctx| {
        bounds.take(key, val, prim_name, ctx)
    })?;
    Ok(bounds.bound())
}

/// Extract connect keyword arguments: `:timeout`, `:deadline`, `:sndbuf`,
/// `:rcvbuf`, `:nodelay`, `:keepalive` and `:encoding`.
pub(crate) fn extract_connect_kwargs(
    args: &[Value],
    start: usize,
    prim_name: &str,
    ctx: &mut NativeCtx,
) -> Result<ConnectKwargs, Refusal> {
    let mut bounds = BoundArgs::default();
    let mut options = SocketOptions::default();
    let mut encoding = None;
    each_keyword(args, start, prim_name, ctx, |key, val, ctx| {
        match key {
            "sndbuf" => options.sndbuf = Some(extract_positive_int(val, key, prim_name, ctx)?),
            "rcvbuf" => options.rcvbuf = Some(extract_positive_int(val, key, prim_name, ctx)?),
            "nodelay" => options.nodelay = Some(extract_bool(val, key, prim_name, ctx)?),
            "keepalive" => options.keepalive = Some(extract_bool(val, key, prim_name, ctx)?),
            "encoding" => encoding = Some(extract_encoding(val, prim_name, ctx)?),
            _ => return bounds.take(key, val, prim_name, ctx),
        }
        Ok(true)
    })?;
    Ok(ConnectKwargs {
        bound: bounds.bound(),
        options,
        encoding,
    })
}

fn extract_encoding(val: Value, prim_name: &str, ctx: &mut NativeCtx) -> Result<Encoding, Refusal> {
    match ctx.keyword_spelling(val).as_deref() {
        Some("text") => Ok(Encoding::Text),
        Some("binary") => Ok(Encoding::Binary),
        Some(other) => Err((
            SIG_ERROR,
            ctx.error(
                "value-error",
                format!(
                    "{}: :encoding must be :text or :binary, got :{}",
                    prim_name, other
                ),
            ),
        )),
        None => Err((
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!(
                    "{}: :encoding value must be keyword (:text or :binary), got {}",
                    prim_name,
                    val.type_name()
                ),
            ),
        )),
    }
}

fn extract_positive_int(
    val: Value,
    name: &str,
    prim_name: &str,
    ctx: &mut NativeCtx,
) -> Result<i32, Refusal> {
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
    val: Value,
    name: &str,
    prim_name: &str,
    ctx: &mut NativeCtx,
) -> Result<bool, Refusal> {
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
