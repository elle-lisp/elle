//! Unix domain socket primitives.

use crate::io::request::{ConnectAddr, IoOp, IoRequest};
use crate::port::{Port, PortKind};
use crate::primitives::def::PrimitiveDef;
use crate::primitives::kwarg::extract_connect_kwargs;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_IO, SIG_OK, SIG_YIELD};
use crate::value::types::Arity;
use crate::value::{error_val, Value};
use std::os::unix::io::{FromRawFd, OwnedFd};

use super::net::{extract_port_of_kind, extract_string, parse_shutdown_how};

/// (unix/listen path) → listener-port
pub(crate) fn prim_unix_listen(args: &[Value]) -> (SignalBits, Value) {
    let path = match extract_string(&args[0], "path", "unix/listen") {
        Ok(s) => s,
        Err(e) => return e,
    };

    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    if fd < 0 {
        return (
            SIG_ERROR,
            error_val(
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
                error_val("io-error", format!("unix/listen: {}", msg)),
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
            error_val("io-error", format!("unix/listen: bind: {}", err)),
        );
    }

    let ret = unsafe { libc::listen(fd, 128) };
    if ret < 0 {
        let err = std::io::Error::last_os_error();
        unsafe { libc::close(fd) };
        return (
            SIG_ERROR,
            error_val("io-error", format!("unix/listen: listen: {}", err)),
        );
    }

    unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
    let owned_fd = unsafe { OwnedFd::from_raw_fd(fd) };
    let p = Port::new_unix_listener(owned_fd, path);
    (SIG_OK, Value::external("port", p))
}

/// (unix/accept listener [:sndbuf n] [:rcvbuf n] [:keepalive bool] [:timeout ms]) → stream-port
pub(crate) fn prim_unix_accept(args: &[Value]) -> (SignalBits, Value) {
    let port_val = match extract_port_of_kind(&args[0], PortKind::UnixListener, "unix/accept") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let kwargs = match extract_connect_kwargs(args, 1, "unix/accept") {
        Ok(k) => k,
        Err(e) => return e,
    };
    (
        SIG_YIELD | SIG_IO,
        IoRequest::with_timeout(
            IoOp::Accept {
                options: kwargs.options,
            },
            port_val,
            kwargs.timeout,
        ),
    )
}

/// (unix/connect path [:sndbuf n] [:rcvbuf n] [:keepalive bool] [:timeout ms]) → stream-port
pub(crate) fn prim_unix_connect(args: &[Value]) -> (SignalBits, Value) {
    let path = match extract_string(&args[0], "path", "unix/connect") {
        Ok(s) => s,
        Err(e) => return e,
    };
    let kwargs = match extract_connect_kwargs(args, 1, "unix/connect") {
        Ok(k) => k,
        Err(e) => return e,
    };
    (
        SIG_YIELD | SIG_IO,
        IoRequest::with_timeout(
            IoOp::Connect {
                addr: ConnectAddr::Unix {
                    path,
                    options: kwargs.options,
                },
            },
            Value::NIL,
            kwargs.timeout,
        ),
    )
}

/// (unix/shutdown port how) → nil
pub(crate) fn prim_unix_shutdown(args: &[Value]) -> (SignalBits, Value) {
    let port_val = match extract_port_of_kind(&args[0], PortKind::UnixStream, "unix/shutdown") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let how = match parse_shutdown_how(&args[1], "unix/shutdown") {
        Ok(h) => h,
        Err(e) => return e,
    };
    (
        SIG_YIELD | SIG_IO,
        IoRequest::new(IoOp::Shutdown { how }, port_val),
    )
}

// ---------------------------------------------------------------------------
// PRIMITIVES table
// ---------------------------------------------------------------------------

pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "unix/listen",
        func: prim_unix_listen,
        arity: Arity::Exact(1),
        signal: Signal::errors(),
        doc: "Listen on a Unix domain socket. Returns a listener port.",
        params: &["path"],
        category: "unix",
        example: "(unix/listen \"/tmp/my.sock\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "unix/accept",
        func: prim_unix_accept,
        arity: Arity::AtLeast(1),
        signal: Signal {
            bits: SIG_ERROR.union(SIG_YIELD).union(SIG_IO),
            propagates: 0,
        },
        doc: "Accept a connection on a Unix listener. Returns a stream port.",
        params: &["listener"],
        category: "unix",
        example: "(unix/accept listener)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "unix/connect",
        func: prim_unix_connect,
        arity: Arity::AtLeast(1),
        signal: Signal {
            bits: SIG_ERROR.union(SIG_YIELD).union(SIG_IO),
            propagates: 0,
        },
        doc: "Connect to a Unix domain socket. Returns a stream port.",
        params: &["path"],
        category: "unix",
        example: "(unix/connect \"/tmp/my.sock\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "unix/shutdown",
        func: prim_unix_shutdown,
        arity: Arity::Exact(2),
        signal: Signal {
            bits: SIG_ERROR.union(SIG_YIELD).union(SIG_IO),
            propagates: 0,
        },
        doc: "Shutdown a Unix stream. how: :read, :write, or :read-write.",
        params: &["port", "how"],
        category: "unix",
        example: "(unix/shutdown conn :write)",
        aliases: &[],
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::fiber::{SIG_IO, SIG_YIELD};

    #[test]
    fn test_unix_listen_returns_ok() {
        let path = format!("/tmp/elle-test-unix-listen-{}.sock", std::process::id());
        let (bits, val) = prim_unix_listen(&[Value::string(&*path)]);
        assert_eq!(bits, SIG_OK);
        let port = val.as_external::<Port>().unwrap();
        assert_eq!(port.kind(), PortKind::UnixListener);
        port.close();
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_unix_accept_returns_sig_io() {
        let path = format!("/tmp/elle-test-unix-accept-{}.sock", std::process::id());
        let (_, listener) = prim_unix_listen(&[Value::string(&*path)]);
        let (bits, _) = prim_unix_accept(&[listener]);
        assert_eq!(bits, SIG_YIELD | SIG_IO);
        listener.as_external::<Port>().unwrap().close();
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_unix_connect_returns_sig_io() {
        let (bits, _) = prim_unix_connect(&[Value::string("/tmp/nonexistent.sock")]);
        assert_eq!(bits, SIG_YIELD | SIG_IO);
    }

    #[test]
    fn test_unix_shutdown_returns_sig_io() {
        let file = std::fs::File::open("/dev/null").unwrap();
        let fd: std::os::unix::io::OwnedFd = file.into();
        let stream_port = Value::external("port", Port::new_unix_stream(fd, "x".into()));
        let (bits, _) = prim_unix_shutdown(&[stream_port, Value::keyword("read-write")]);
        assert_eq!(bits, SIG_YIELD | SIG_IO);
    }
}
