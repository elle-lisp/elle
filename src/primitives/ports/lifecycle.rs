use super::*;

/// Map an Elle mode keyword name to POSIX open(2) flags and direction.
///
/// All flags include O_CLOEXEC for atomic close-on-exec at openat() time,
/// avoiding the race window between openat() and a post-hoc fcntl().
fn mode_to_flags(mode: &str) -> Option<(i32, Direction)> {
    match mode {
        "read" => Some((libc::O_RDONLY | libc::O_CLOEXEC, Direction::Read)),
        "write" => Some((
            libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC | libc::O_CLOEXEC,
            Direction::Write,
        )),
        "append" => Some((
            libc::O_WRONLY | libc::O_CREAT | libc::O_APPEND | libc::O_CLOEXEC,
            Direction::Write,
        )),
        "read-write" => Some((
            libc::O_RDWR | libc::O_CREAT | libc::O_CLOEXEC,
            Direction::ReadWrite,
        )),
        _ => None,
    }
}

/// Helper: open a file with the given encoding.
///
/// Shared implementation for `port/open` and `port/open-bytes`.
/// Yields `SIG_YIELD | SIG_IO` with an `IoRequest` containing `IoOp::Open`.
/// Argument validation (path type, mode keyword, timeout) happens here before yielding.
fn open_file(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
    encoding: Encoding,
    prim_name: &str,
) -> (SignalBits, Value) {
    let path = match args[0].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    format!(
                        "{}: expected string for path, got {}",
                        prim_name,
                        args[0].type_name()
                    ),
                ),
            );
        }
    };

    let mode_name_owned = match args[1].as_keyword_name() {
        Some(name) => name,
        None => {
            return (
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    format!(
                        "{}: expected keyword for mode, got {}",
                        prim_name,
                        args[1].type_name()
                    ),
                ),
            );
        }
    };

    let (flags, direction) = match mode_to_flags(&mode_name_owned) {
        Some(pair) => pair,
        None => {
            return (
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    format!(
                        "{}: unknown mode :{}, expected :read, :write, :append, or :read-write",
                        prim_name, mode_name_owned
                    ),
                ),
            );
        }
    };

    let timeout = match extract_keyword_timeout(args, 2, prim_name, ctx) {
        Ok(t) => t,
        Err(e) => return e,
    };

    let port_val = ctx.external(
        "port",
        Port::new_unopened(PortKind::File, direction, encoding, path.clone()),
    );
    (
        SIG_YIELD | SIG_IO,
        IoRequest::with_timeout(
            ctx,
            IoOp::Open {
                path,
                flags,
                mode: 0o666,
                direction,
                encoding,
            },
            port_val,
            timeout,
        ),
    )
}

/// (port/open path mode) → port
///
/// Open a file with text (UTF-8) encoding.
pub(super) fn prim_port_open(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    open_file(ctx, args, Encoding::Text, "port/open")
}

/// (port/open-bytes path mode) → port
///
/// Open a file with binary encoding.
pub(super) fn prim_port_open_bytes(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    open_file(ctx, args, Encoding::Binary, "port/open-bytes")
}

/// (port/close port) → nil
///
/// Close a port. Idempotent — closing an already-closed port is a no-op.
///
/// For ports with an fd (file, network, pipe), yields SIG_IO so the
/// async scheduler can cancel pending io_uring operations before the
/// fd is dropped. For stdio ports (no owned fd) and already-closed
/// ports, completes synchronously.
pub(super) fn prim_port_close(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let port = match extract_port(&args[0], "port/close", ctx) {
        Ok(p) => p,
        Err(e) => return e,
    };
    // Already closed: no-op.
    if port.is_closed() {
        return (SIG_OK, Value::NIL);
    }
    // Stdout/stderr don't own their fd and have no async resource to
    // tear down — close synchronously. Stdin DOES have an async
    // resource: a dedicated worker thread parked in `libc::read(0, …)`.
    // Even though the port doesn't own fd 0, the close must reach the
    // AsyncBackend so the worker is signalled out of its blocking read
    // and the fiber waiting on the in-flight read is resumed with a
    // `stdin closed` error. See `docs/io.md` and the close branch in
    // `AsyncBackend::submit` for the full path.
    if !port.has_fd() && !matches!(port.kind(), crate::port::PortKind::Stdin) {
        port.close();
        return (SIG_OK, Value::NIL);
    }
    // Stdin and ports with an owned fd: yield to the I/O scheduler so
    // it can cancel pending operations / shut down the stdin worker
    // before the fd is dropped (or, for stdin, the port is marked
    // closed and any in-flight read is woken).
    (
        SIG_YIELD | SIG_IO,
        IoRequest::new(ctx, IoOp::Close, args[0]),
    )
}

/// (port/stdin) → port
pub(super) fn prim_port_stdin(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    (SIG_OK, ctx.external("port", Port::stdin()))
}

/// (port/stdout) → port
pub(super) fn prim_port_stdout(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    (SIG_OK, ctx.external("port", Port::stdout()))
}

/// (port/stderr) → port
pub(super) fn prim_port_stderr(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    (SIG_OK, ctx.external("port", Port::stderr()))
}

/// (port? value) → boolean
pub(super) fn prim_is_port(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    (
        SIG_OK,
        Value::bool(args[0].external_type_name() == Some("port")),
    )
}

/// (port/open? port) → boolean
///
/// Returns true if the port is open, false if closed.
/// Signals :type-error if argument is not a port.
pub(super) fn prim_is_port_open(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let port = match extract_port(&args[0], "port/open?", ctx) {
        Ok(p) => p,
        Err(e) => return e,
    };
    (SIG_OK, Value::bool(!port.is_closed()))
}
