//! Unix domain socket primitives.

use crate::io::request::{ConnectAddr, IoOp, IoRequest};
use crate::port::{Direction, Port, PortKind};
use crate::primitives::def::RegionEffect;
use crate::primitives::kwarg::extract_connect_kwargs;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_IO, SIG_OK, SIG_YIELD};
use crate::value::types::Arity;
use crate::value::Value;
use std::os::unix::io::{FromRawFd, OwnedFd};

use super::net::{extract_port_of_kind, extract_string, parse_shutdown_how};

/// (unix/listen path) → listener-port
pub(crate) fn prim_unix_listen(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let path = match extract_string(&args[0], "path", "unix/listen", ctx) {
        Ok(s) => s,
        Err(e) => return e,
    };

    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    if fd < 0 {
        return (
            SIG_ERROR,
            ctx.error(
                "io-error",
                format!("unix/listen: socket: {}", std::io::Error::last_os_error()),
            ),
        );
    }

    // SO_REUSEADDR
    unsafe {
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_REUSEADDR,
            &1i32 as *const i32 as *const libc::c_void,
            std::mem::size_of::<i32>() as libc::socklen_t,
        );
    }

    // Filesystem socket — unlink first to avoid EADDRINUSE
    if !path.starts_with('@') {
        let _ = std::fs::remove_file(&path);
    }

    let (sun, addr_len) = match crate::io::sockaddr::build_unix(&path) {
        Ok(result) => result,
        Err(msg) => {
            unsafe { libc::close(fd) };
            return (
                SIG_ERROR,
                ctx.error("io-error", format!("unix/listen: {}", msg)),
            );
        }
    };

    let ret = unsafe {
        libc::bind(
            fd,
            &sun as *const libc::sockaddr_un as *const libc::sockaddr,
            addr_len,
        )
    };
    if ret < 0 {
        let err = std::io::Error::last_os_error();
        unsafe { libc::close(fd) };
        return (
            SIG_ERROR,
            ctx.error("io-error", format!("unix/listen: bind: {}", err)),
        );
    }

    let ret = unsafe { libc::listen(fd, 128) };
    if ret < 0 {
        let err = std::io::Error::last_os_error();
        unsafe { libc::close(fd) };
        return (
            SIG_ERROR,
            ctx.error("io-error", format!("unix/listen: listen: {}", err)),
        );
    }

    unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
    let owned_fd = unsafe { OwnedFd::from_raw_fd(fd) };
    let p = Port::new_unix_listener(owned_fd, path);
    (SIG_OK, ctx.external("port", p))
}

/// (unix/accept listener [:sndbuf n] [:rcvbuf n] [:keepalive bool] [:timeout ms]) → stream-port
pub(crate) fn prim_unix_accept(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let port_val = match extract_port_of_kind(&args[0], PortKind::UnixListener, "unix/accept", ctx)
    {
        Ok(v) => v,
        Err(e) => return e,
    };
    let kwargs = match extract_connect_kwargs(args, 1, "unix/accept", ctx) {
        Ok(k) => k,
        Err(e) => return e,
    };
    let encoding = kwargs.encoding.unwrap_or(crate::port::Encoding::Binary);
    (
        SIG_YIELD | SIG_IO,
        IoRequest::with_timeout(
            ctx,
            IoOp::Accept {
                options: kwargs.options,
                encoding,
                accept_port: ctx.external(
                    "port",
                    Port::new_unopened(
                        PortKind::UnixStream,
                        Direction::ReadWrite,
                        encoding,
                        String::new(),
                    ),
                ),
            },
            port_val,
            kwargs.timeout,
        ),
    )
}

/// (unix/connect path [:sndbuf n] [:rcvbuf n] [:keepalive bool]
///                    [:encoding :text|:binary] [:timeout ms]) → stream-port
///
/// `:encoding` controls the resulting stream port's mode.  Default is
/// `:binary` (Unix-domain stream sockets are byte streams).  Pass
/// `:text` for line-oriented text protocols carried over Unix sockets.
pub(crate) fn prim_unix_connect(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let path = match extract_string(&args[0], "path", "unix/connect", ctx) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let kwargs = match extract_connect_kwargs(args, 1, "unix/connect", ctx) {
        Ok(k) => k,
        Err(e) => return e,
    };
    let encoding = kwargs.encoding.unwrap_or(crate::port::Encoding::Binary);
    let port_val = ctx.external(
        "port",
        Port::new_unopened(
            PortKind::UnixStream,
            Direction::ReadWrite,
            encoding,
            path.clone(),
        ),
    );
    (
        SIG_YIELD | SIG_IO,
        IoRequest::with_timeout(
            ctx,
            IoOp::Connect {
                addr: ConnectAddr::Unix {
                    path,
                    options: kwargs.options,
                    encoding,
                },
            },
            port_val,
            kwargs.timeout,
        ),
    )
}

/// (unix/shutdown port how) → nil
pub(crate) fn prim_unix_shutdown(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let port_val = match extract_port_of_kind(&args[0], PortKind::UnixStream, "unix/shutdown", ctx)
    {
        Ok(v) => v,
        Err(e) => return e,
    };
    let how = match parse_shutdown_how(&args[1], "unix/shutdown", ctx) {
        Ok(h) => h,
        Err(e) => return e,
    };
    (
        SIG_YIELD | SIG_IO,
        IoRequest::new(ctx, IoOp::Shutdown { how }, port_val),
    )
}

// ---------------------------------------------------------------------------
// PRIMITIVES table
// ---------------------------------------------------------------------------

primitive! {
    "unix/listen" => prim_unix_listen {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Listen on a Unix domain socket. Returns a listener port.",
        params: &["path"],
        category: "unix",
        example: "(unix/listen \"./my.sock\")",
        effect: RegionEffect::Fresh,
    }
    "unix/accept" => prim_unix_accept {
        signal: Signal::io_yields_errors(),
        arity: Arity::AtLeast(1),
        doc: "Accept a connection on a Unix listener. Returns a stream port.",
        params: &["listener"],
        category: "unix",
        example: "(unix/accept listener)",
        // Fresh: the stream port is pre-minted in this call's ctx region
        // (`accept_port: ctx.external(..)`), fd set in place by the completion.
        effect: RegionEffect::Fresh,
    }
    "unix/connect" => prim_unix_connect {
        signal: Signal::io_yields_errors(),
        arity: Arity::AtLeast(1),
        doc: "Connect to a Unix domain socket. Returns a stream port.",
        params: &["path"],
        category: "unix",
        example: "(unix/connect \"./my.sock\")",
        // Fresh: the stream port is pre-minted in this call's ctx region.
        effect: RegionEffect::Fresh,
    }
    "unix/shutdown" => prim_unix_shutdown {
        signal: Signal::io_yields_errors(),
        arity: Arity::Exact(2),
        doc: "Shutdown a Unix stream. how: :read, :write, or :read-write.",
        params: &["port", "how"],
        category: "unix",
        example: "(unix/shutdown conn :write)",
        // Immediate: the completion returns Value::NIL.
        effect: RegionEffect::Immediate,
    }
}

#[cfg(test)]
mod tests;
