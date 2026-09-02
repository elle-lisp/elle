use super::*;

/// (port/set-options port :timeout ms) → nil
///
/// Set port options. Currently only :timeout is recognized.
/// Pass nil to clear the timeout.
pub(super) fn prim_port_set_options(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let port = match extract_port(&args[0], "port/set-options", ctx) {
        Ok(p) => p,
        Err(e) => return e,
    };

    let remaining = &args[1..];
    if !remaining.len().is_multiple_of(2) {
        return (
            SIG_ERROR,
            ctx.error(
                "arity-error",
                "port/set-options: keyword arguments must be key-value pairs",
            ),
        );
    }

    let mut i = 0;
    while i < remaining.len() {
        let key = &remaining[i];
        let val = &remaining[i + 1];

        match ctx.keyword_spelling(*key).as_deref() {
            Some("timeout") => {
                if val.is_nil() {
                    port.set_timeout_ms(None);
                } else {
                    match val.as_int() {
                        Some(ms) if ms >= 0 => {
                            port.set_timeout_ms(Some(ms as u64));
                        }
                        Some(ms) => {
                            return (
                                SIG_ERROR,
                                ctx.error(
                                    "value-error",
                                    format!(
                                        "port/set-options: :timeout must be non-negative, got {}",
                                        ms
                                    ),
                                ),
                            );
                        }
                        None => {
                            return (
                                SIG_ERROR,
                                ctx.error(
                                    "type-error",
                                    format!(
                                        "port/set-options: :timeout value must be integer or nil, got {}",
                                        val.type_name()
                                    ),
                                ),
                            );
                        }
                    }
                }
            }
            Some(other) => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "value-error",
                        format!("port/set-options: unknown option :{}", other),
                    ),
                );
            }
            None => {
                return type_error!(ctx, key, "port/set-options", "keyword");
            }
        }
        i += 2;
    }

    (SIG_OK, Value::NIL)
}

/// (port/path port) → string or nil
///
/// Returns the path or address the port was opened on:
/// - File port: the file path string (e.g. "data/foo.txt")
/// - TCP listener: the bound address string (e.g. "127.0.0.1:8080")
/// - TCP stream: the peer address string (e.g. "127.0.0.1:54321")
/// - Stdio ports (stdin/stdout/stderr): nil
///
/// Signals :type-error if argument is not a port.
pub(super) fn prim_port_path(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let port = match extract_port(&args[0], "port/path", ctx) {
        Ok(p) => p,
        Err(e) => return e,
    };
    match port.path() {
        Some(p) => (SIG_OK, ctx.string(p)),
        None => (SIG_OK, Value::NIL),
    }
}

/// (port/encoding port) → :text | :binary
///
/// Returns the port's encoding mode as a keyword.  `:text` ports return
/// strings from read operations and treat `port/read-exact`'s count as
/// graphemes (the unit Elle strings are measured in).  `:binary` ports
/// return bytes and treat the count as bytes — what byte-framed
/// protocols (RESP, gRPC, HTTP/2, length-prefixed everything) need.
///
/// Use this to guard protocol-implementing code that requires one or
/// the other.  Example: a RESP reader can assert
/// `(= :binary (port/encoding port))` up front and fail with a clear
/// error instead of silently corrupting bulk-string framing.
pub(super) fn prim_port_encoding(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let port = match extract_port(&args[0], "port/encoding", ctx) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let kw = match port.encoding() {
        crate::port::Encoding::Text => "text",
        crate::port::Encoding::Binary => "binary",
    };
    (SIG_OK, Value::keyword(kw))
}

/// (port/seek port offset)
/// (port/seek port offset :from :start|:current|:end)
///
/// Seek to `offset` in a file port. Discards the per-fd read buffer before
/// seeking (prevents stale buffered data from diverging from the kernel
/// position). Returns the new absolute byte offset as int.
///
/// The `:from` keyword controls the seek origin:
///   :start   — SEEK_SET (default): absolute offset from file start
///   :current — SEEK_CUR: relative to current position
///   :end     — SEEK_END: relative to end of file (offset is usually negative)
///
/// Only valid on file ports. Returns :type-error on other port kinds.
pub(super) fn prim_port_seek(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    // Arity: exactly 2 or exactly 4 (port, offset, :from, :value).
    // 0, 1, 3, or 5+ args are all errors.
    if args.len() == 3 {
        return (
            SIG_ERROR,
            ctx.error("arity-error", "port/seek: :from keyword requires a value"),
        );
    }

    let port = match extract_port(&args[0], "port/seek", ctx) {
        Ok(p) => p,
        Err(e) => return e,
    };

    if port.kind() != PortKind::File {
        return (
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!("port/seek: expected file port, got {:?}", port.kind()),
            ),
        );
    }

    let offset = prim_arg!(ctx, args, 1, as_int, "port/seek", "integer for offset");

    // Parse optional :from keyword-value pair (args[2] and args[3]).
    let whence = if args.len() == 4 {
        match ctx.keyword_spelling(args[2]).as_deref() {
            Some("from") => {}
            Some(other) => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "value-error",
                        format!("port/seek: unknown keyword :{}, expected :from", other),
                    ),
                )
            }
            None => return type_error!(ctx, args[2], "port/seek", "keyword for third argument"),
        }
        match ctx.keyword_spelling(args[3]).as_deref() {
            Some("start") => libc::SEEK_SET,
            Some("current") => libc::SEEK_CUR,
            Some("end") => libc::SEEK_END,
            Some(other) => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "value-error",
                        format!(
                        "port/seek: invalid :from value :{}, expected :start, :current, or :end",
                        other
                    ),
                    ),
                )
            }
            None => return type_error!(ctx, args[3], "port/seek", "keyword for :from value"),
        }
    } else {
        libc::SEEK_SET // default: seek from start
    };

    (
        SIG_IO,
        IoRequest::new(ctx, IoOp::Seek { offset, whence }, args[0]),
    )
}

/// (port/tell port) → int
///
/// Return the current logical read position in a file port.
/// Logical position = kernel file offset - buffered-but-unconsumed bytes.
/// Only valid on file ports. Returns :type-error on other port kinds.
pub(super) fn prim_port_tell(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let port = match extract_port(&args[0], "port/tell", ctx) {
        Ok(p) => p,
        Err(e) => return e,
    };

    if port.kind() != PortKind::File {
        return (
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!("port/tell: expected file port, got {:?}", port.kind()),
            ),
        );
    }

    (SIG_IO, IoRequest::new(ctx, IoOp::Tell, args[0]))
}
