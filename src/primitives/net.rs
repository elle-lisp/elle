//! Network primitives — TCP and UDP.
//!
//! Unix domain socket primitives are in `unix.rs`.
//!
//! Listener/bind primitives are synchronous (no SIG_IO) because they
//! complete immediately. Accept/connect/send/recv/shutdown yield SIG_IO
//! for scheduler dispatch.

use crate::io::request::{ConnectAddr, IoOp, IoRequest};
use crate::port::{Port, PortKind};
use crate::primitives::def::PrimitiveDef;
use crate::primitives::kwarg::extract_keyword_timeout;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_IO, SIG_OK, SIG_YIELD};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

use std::os::unix::io::{FromRawFd, OwnedFd};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub(crate) fn extract_port_of_kind(
    value: &Value,
    expected: PortKind,
    prim_name: &str,
) -> Result<Value, (SignalBits, Value)> {
    let port = value.as_external::<Port>().ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{}: expected port, got {}", prim_name, value.type_name()),
            ),
        )
    })?;
    if port.kind() != expected {
        return Err((
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "{}: expected {:?} port, got {:?}",
                    prim_name,
                    expected,
                    port.kind()
                ),
            ),
        ));
    }
    Ok(*value)
}

pub(crate) fn extract_string(
    value: &Value,
    param: &str,
    prim_name: &str,
) -> Result<String, (SignalBits, Value)> {
    value.with_string(|s| s.to_string()).ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "{}: expected string for {}, got {}",
                    prim_name,
                    param,
                    value.type_name()
                ),
            ),
        )
    })
}

fn extract_port_num(value: &Value, prim_name: &str) -> Result<u16, (SignalBits, Value)> {
    match value.as_int() {
        Some(n) if (0..=65535).contains(&n) => Ok(n as u16),
        Some(n) => Err((
            SIG_ERROR,
            error_val(
                "value-error",
                format!("{}: port must be 0-65535, got {}", prim_name, n),
            ),
        )),
        None => Err((
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "{}: expected integer for port, got {}",
                    prim_name,
                    value.type_name()
                ),
            ),
        )),
    }
}

pub(crate) fn parse_shutdown_how(
    value: &Value,
    prim_name: &str,
) -> Result<i32, (SignalBits, Value)> {
    match value.as_keyword_name() {
        Some("read") => Ok(libc::SHUT_RD),
        Some("write") => Ok(libc::SHUT_WR),
        Some("read-write") => Ok(libc::SHUT_RDWR),
        Some(other) => Err((
            SIG_ERROR,
            error_val(
                "value-error",
                format!(
                    "{}: expected :read, :write, or :read-write, got :{}",
                    prim_name, other
                ),
            ),
        )),
        None => Err((
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "{}: expected keyword for how, got {}",
                    prim_name,
                    value.type_name()
                ),
            ),
        )),
    }
}

/// Create a socket, set SO_REUSEADDR, bind, and optionally listen.
/// Returns the OwnedFd and the actual bound address string.
fn bind_socket(
    addr: &str,
    port: u16,
    sock_type: libc::c_int,
    listen: bool,
    prim_name: &str,
) -> Result<(OwnedFd, String), (SignalBits, Value)> {
    use std::net::ToSocketAddrs;

    let addr_str = format!("{}:{}", addr, port);
    let resolved = addr_str
        .to_socket_addrs()
        .map_err(|e| {
            (
                SIG_ERROR,
                error_val("io-error", format!("{}: {}", prim_name, e)),
            )
        })?
        .next()
        .ok_or_else(|| {
            (
                SIG_ERROR,
                error_val(
                    "io-error",
                    format!("{}: could not resolve {}", prim_name, addr_str),
                ),
            )
        })?;

    let family = match resolved {
        std::net::SocketAddr::V4(_) => libc::AF_INET,
        std::net::SocketAddr::V6(_) => libc::AF_INET6,
    };

    let fd = unsafe { libc::socket(family, sock_type, 0) };
    if fd < 0 {
        return Err((
            SIG_ERROR,
            error_val(
                "io-error",
                format!("{}: socket: {}", prim_name, std::io::Error::last_os_error()),
            ),
        ));
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

    // Bind
    let bind_result = match resolved {
        std::net::SocketAddr::V4(ref v4) => {
            let sin = libc::sockaddr_in {
                sin_family: libc::AF_INET as libc::sa_family_t,
                sin_port: v4.port().to_be(),
                sin_addr: libc::in_addr {
                    s_addr: u32::from_ne_bytes(v4.ip().octets()),
                },
                sin_zero: [0; 8],
            };
            unsafe {
                libc::bind(
                    fd,
                    &sin as *const libc::sockaddr_in as *const libc::sockaddr,
                    std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
                )
            }
        }
        std::net::SocketAddr::V6(ref v6) => {
            let sin6 = libc::sockaddr_in6 {
                sin6_family: libc::AF_INET6 as libc::sa_family_t,
                sin6_port: v6.port().to_be(),
                sin6_flowinfo: v6.flowinfo(),
                sin6_addr: libc::in6_addr {
                    s6_addr: v6.ip().octets(),
                },
                sin6_scope_id: v6.scope_id(),
            };
            unsafe {
                libc::bind(
                    fd,
                    &sin6 as *const libc::sockaddr_in6 as *const libc::sockaddr,
                    std::mem::size_of::<libc::sockaddr_in6>() as libc::socklen_t,
                )
            }
        }
    };

    if bind_result < 0 {
        let err = std::io::Error::last_os_error();
        unsafe { libc::close(fd) };
        return Err((
            SIG_ERROR,
            error_val("io-error", format!("{}: bind: {}", prim_name, err)),
        ));
    }

    if listen {
        let ret = unsafe { libc::listen(fd, 128) };
        if ret < 0 {
            let err = std::io::Error::last_os_error();
            unsafe { libc::close(fd) };
            return Err((
                SIG_ERROR,
                error_val("io-error", format!("{}: listen: {}", prim_name, err)),
            ));
        }
    }

    // Get actual bound address (for port 0)
    let mut sa_storage: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
    let mut sa_len = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
    unsafe {
        libc::getsockname(
            fd,
            &mut sa_storage as *mut libc::sockaddr_storage as *mut libc::sockaddr,
            &mut sa_len,
        );
    }

    let bound_addr = match sa_storage.ss_family as libc::c_int {
        libc::AF_INET => {
            let sin = unsafe {
                &*(&sa_storage as *const libc::sockaddr_storage as *const libc::sockaddr_in)
            };
            let ip = std::net::Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
            let p = u16::from_be(sin.sin_port);
            format!("{}:{}", ip, p)
        }
        libc::AF_INET6 => {
            let sin6 = unsafe {
                &*(&sa_storage as *const libc::sockaddr_storage as *const libc::sockaddr_in6)
            };
            let ip = std::net::Ipv6Addr::from(sin6.sin6_addr.s6_addr);
            let p = u16::from_be(sin6.sin6_port);
            format!("[{}]:{}", ip, p)
        }
        _ => addr_str,
    };

    unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
    let owned_fd = unsafe { OwnedFd::from_raw_fd(fd) };
    Ok((owned_fd, bound_addr))
}

// ---------------------------------------------------------------------------
// TCP primitives
// ---------------------------------------------------------------------------

/// (tcp/listen addr port) → listener-port
fn prim_tcp_listen(args: &[Value]) -> (SignalBits, Value) {
    let addr = match extract_string(&args[0], "addr", "tcp/listen") {
        Ok(s) => s,
        Err(e) => return e,
    };
    let port = match extract_port_num(&args[1], "tcp/listen") {
        Ok(p) => p,
        Err(e) => return e,
    };

    match bind_socket(&addr, port, libc::SOCK_STREAM, true, "tcp/listen") {
        Ok((fd, bound_addr)) => {
            let p = Port::new_tcp_listener(fd, bound_addr);
            (SIG_OK, Value::external("port", p))
        }
        Err(e) => e,
    }
}

/// (tcp/accept listener [:timeout ms]) → stream-port
fn prim_tcp_accept(args: &[Value]) -> (SignalBits, Value) {
    let port_val = match extract_port_of_kind(&args[0], PortKind::TcpListener, "tcp/accept") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let timeout = match extract_keyword_timeout(args, 1, "tcp/accept") {
        Ok(t) => t,
        Err(e) => return e,
    };
    (
        SIG_YIELD | SIG_IO,
        IoRequest::with_timeout(IoOp::Accept, port_val, timeout),
    )
}

/// (tcp/connect addr port [:timeout ms]) → stream-port
fn prim_tcp_connect(args: &[Value]) -> (SignalBits, Value) {
    let addr = match extract_string(&args[0], "addr", "tcp/connect") {
        Ok(s) => s,
        Err(e) => return e,
    };
    let port = match extract_port_num(&args[1], "tcp/connect") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let timeout = match extract_keyword_timeout(args, 2, "tcp/connect") {
        Ok(t) => t,
        Err(e) => return e,
    };
    (
        SIG_YIELD | SIG_IO,
        IoRequest::with_timeout(
            IoOp::Connect {
                addr: ConnectAddr::Tcp { addr, port },
            },
            Value::NIL,
            timeout,
        ),
    )
}

/// (tcp/shutdown port how) → nil
fn prim_tcp_shutdown(args: &[Value]) -> (SignalBits, Value) {
    let port_val = match extract_port_of_kind(&args[0], PortKind::TcpStream, "tcp/shutdown") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let how = match parse_shutdown_how(&args[1], "tcp/shutdown") {
        Ok(h) => h,
        Err(e) => return e,
    };
    (
        SIG_YIELD | SIG_IO,
        IoRequest::new(IoOp::Shutdown { how }, port_val),
    )
}

// ---------------------------------------------------------------------------
// UDP primitives
// ---------------------------------------------------------------------------

/// (udp/bind addr port) → udp-port
fn prim_udp_bind(args: &[Value]) -> (SignalBits, Value) {
    let addr = match extract_string(&args[0], "addr", "udp/bind") {
        Ok(s) => s,
        Err(e) => return e,
    };
    let port = match extract_port_num(&args[1], "udp/bind") {
        Ok(p) => p,
        Err(e) => return e,
    };

    match bind_socket(&addr, port, libc::SOCK_DGRAM, false, "udp/bind") {
        Ok((fd, bound_addr)) => {
            let p = Port::new_udp_socket(fd, bound_addr);
            (SIG_OK, Value::external("port", p))
        }
        Err(e) => e,
    }
}

/// (udp/send-to socket data addr port [:timeout ms]) → bytes-sent
fn prim_udp_send_to(args: &[Value]) -> (SignalBits, Value) {
    let socket_val = match extract_port_of_kind(&args[0], PortKind::UdpSocket, "udp/send-to") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let data = args[1];
    let addr = match extract_string(&args[2], "addr", "udp/send-to") {
        Ok(s) => s,
        Err(e) => return e,
    };
    let port_num = match extract_port_num(&args[3], "udp/send-to") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let timeout = match extract_keyword_timeout(args, 4, "udp/send-to") {
        Ok(t) => t,
        Err(e) => return e,
    };
    (
        SIG_YIELD | SIG_IO,
        IoRequest::with_timeout(
            IoOp::SendTo {
                addr,
                port_num,
                data,
            },
            socket_val,
            timeout,
        ),
    )
}

/// (udp/recv-from socket count [:timeout ms]) → {:data bytes :addr string :port int}
fn prim_udp_recv_from(args: &[Value]) -> (SignalBits, Value) {
    let socket_val = match extract_port_of_kind(&args[0], PortKind::UdpSocket, "udp/recv-from") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let count = match args[1].as_int() {
        Some(n) if n > 0 => n as usize,
        Some(n) => {
            return (
                SIG_ERROR,
                error_val(
                    "value-error",
                    format!("udp/recv-from: count must be positive, got {}", n),
                ),
            )
        }
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "udp/recv-from: expected integer for count, got {}",
                        args[1].type_name()
                    ),
                ),
            )
        }
    };
    let timeout = match extract_keyword_timeout(args, 2, "udp/recv-from") {
        Ok(t) => t,
        Err(e) => return e,
    };
    (
        SIG_YIELD | SIG_IO,
        IoRequest::with_timeout(IoOp::RecvFrom { count }, socket_val, timeout),
    )
}

// ---------------------------------------------------------------------------
// PRIMITIVES table
// ---------------------------------------------------------------------------

pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    // TCP
    PrimitiveDef {
        name: "tcp/listen",
        func: prim_tcp_listen,
        arity: Arity::Exact(2),
        effect: Signal::errors(),
        doc: "Bind and listen on a TCP address. Returns a listener port.",
        params: &["addr", "port"],
        category: "tcp",
        example: "(tcp/listen \"127.0.0.1\" 8080)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "tcp/accept",
        func: prim_tcp_accept,
        arity: Arity::AtLeast(1),
        effect: Signal::errors(),
        doc: "Accept a connection on a TCP listener. Returns a stream port.",
        params: &["listener"],
        category: "tcp",
        example: "(tcp/accept listener)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "tcp/connect",
        func: prim_tcp_connect,
        arity: Arity::AtLeast(2),
        effect: Signal::errors(),
        doc: "Connect to a TCP address. Returns a stream port.",
        params: &["addr", "port"],
        category: "tcp",
        example: "(tcp/connect \"127.0.0.1\" 8080)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "tcp/shutdown",
        func: prim_tcp_shutdown,
        arity: Arity::Exact(2),
        effect: Signal::errors(),
        doc: "Shutdown a TCP stream. how: :read, :write, or :read-write.",
        params: &["port", "how"],
        category: "tcp",
        example: "(tcp/shutdown conn :write)",
        aliases: &[],
    },
    // UDP
    PrimitiveDef {
        name: "udp/bind",
        func: prim_udp_bind,
        arity: Arity::Exact(2),
        effect: Signal::errors(),
        doc: "Bind a UDP socket. Returns a UDP port.",
        params: &["addr", "port"],
        category: "udp",
        example: "(udp/bind \"0.0.0.0\" 9000)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "udp/send-to",
        func: prim_udp_send_to,
        arity: Arity::AtLeast(4),
        effect: Signal::errors(),
        doc: "Send data to a remote address via UDP. Returns bytes sent.",
        params: &["socket", "data", "addr", "port"],
        category: "udp",
        example: "(udp/send-to sock \"hello\" \"127.0.0.1\" 9000)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "udp/recv-from",
        func: prim_udp_recv_from,
        arity: Arity::AtLeast(2),
        effect: Signal::errors(),
        doc: "Receive data from a UDP socket. Returns {:data :addr :port}.",
        params: &["socket", "count"],
        category: "udp",
        example: "(udp/recv-from sock 1024)",
        aliases: &[],
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::fiber::{SIG_IO, SIG_YIELD};

    #[test]
    fn test_tcp_listen_returns_ok() {
        let (bits, val) = prim_tcp_listen(&[Value::string("127.0.0.1"), Value::int(0)]);
        assert_eq!(bits, SIG_OK);
        let port = val.as_external::<Port>().unwrap();
        assert_eq!(port.kind(), PortKind::TcpListener);
        port.close();
    }

    #[test]
    fn test_tcp_listen_port_zero() {
        let (bits, val) = prim_tcp_listen(&[Value::string("127.0.0.1"), Value::int(0)]);
        assert_eq!(bits, SIG_OK);
        let port = val.as_external::<Port>().unwrap();
        // Verify OS assigned a real port (path contains it)
        let path = port.path().unwrap();
        assert!(path.contains(':'), "expected addr:port, got {}", path);
        let port_str = path.split(':').next_back().unwrap();
        let port_num: u16 = port_str.parse().unwrap();
        assert!(port_num > 0, "expected non-zero port, got {}", port_num);
        port.close();
    }

    #[test]
    fn test_tcp_listen_bad_addr_errors() {
        let (bits, _) = prim_tcp_listen(&[Value::string("not-a-valid-addr"), Value::int(0)]);
        assert_eq!(bits, SIG_ERROR);
    }

    #[test]
    fn test_tcp_listen_bad_port_errors() {
        let (bits, _) = prim_tcp_listen(&[Value::string("127.0.0.1"), Value::int(99999)]);
        assert_eq!(bits, SIG_ERROR);
    }

    #[test]
    fn test_tcp_listen_non_string_addr_errors() {
        let (bits, _) = prim_tcp_listen(&[Value::int(42), Value::int(0)]);
        assert_eq!(bits, SIG_ERROR);
    }

    #[test]
    fn test_tcp_accept_returns_sig_io() {
        let (_, listener) = prim_tcp_listen(&[Value::string("127.0.0.1"), Value::int(0)]);
        let (bits, val) = prim_tcp_accept(&[listener]);
        assert_eq!(bits, SIG_YIELD | SIG_IO);
        assert_eq!(val.external_type_name(), Some("io-request"));
        // Clean up
        listener.as_external::<Port>().unwrap().close();
    }

    #[test]
    fn test_tcp_accept_non_listener_errors() {
        // Create a TcpStream port (not a listener)
        let file = std::fs::File::open("/dev/null").unwrap();
        let fd: std::os::unix::io::OwnedFd = file.into();
        let stream_port = Value::external("port", Port::new_tcp_stream(fd, "x".into()));
        let (bits, _) = prim_tcp_accept(&[stream_port]);
        assert_eq!(bits, SIG_ERROR);
    }

    #[test]
    fn test_tcp_accept_non_port_errors() {
        let (bits, _) = prim_tcp_accept(&[Value::int(42)]);
        assert_eq!(bits, SIG_ERROR);
    }

    #[test]
    fn test_tcp_connect_returns_sig_io() {
        let (bits, val) = prim_tcp_connect(&[Value::string("127.0.0.1"), Value::int(8080)]);
        assert_eq!(bits, SIG_YIELD | SIG_IO);
        assert_eq!(val.external_type_name(), Some("io-request"));
    }

    #[test]
    fn test_tcp_connect_bad_port_errors() {
        let (bits, _) = prim_tcp_connect(&[Value::string("127.0.0.1"), Value::int(99999)]);
        assert_eq!(bits, SIG_ERROR);
    }

    #[test]
    fn test_tcp_shutdown_returns_sig_io() {
        let file = std::fs::File::open("/dev/null").unwrap();
        let fd: std::os::unix::io::OwnedFd = file.into();
        let stream_port = Value::external("port", Port::new_tcp_stream(fd, "x".into()));
        let (bits, _) = prim_tcp_shutdown(&[stream_port, Value::keyword("write")]);
        assert_eq!(bits, SIG_YIELD | SIG_IO);
    }

    #[test]
    fn test_tcp_shutdown_bad_how_errors() {
        let file = std::fs::File::open("/dev/null").unwrap();
        let fd: std::os::unix::io::OwnedFd = file.into();
        let stream_port = Value::external("port", Port::new_tcp_stream(fd, "x".into()));
        let (bits, _) = prim_tcp_shutdown(&[stream_port, Value::keyword("foo")]);
        assert_eq!(bits, SIG_ERROR);
    }

    #[test]
    fn test_udp_bind_returns_ok() {
        let (bits, val) = prim_udp_bind(&[Value::string("127.0.0.1"), Value::int(0)]);
        assert_eq!(bits, SIG_OK);
        let port = val.as_external::<Port>().unwrap();
        assert_eq!(port.kind(), PortKind::UdpSocket);
        port.close();
    }

    #[test]
    fn test_udp_send_to_returns_sig_io() {
        let (_, socket) = prim_udp_bind(&[Value::string("127.0.0.1"), Value::int(0)]);
        let (bits, _) = prim_udp_send_to(&[
            socket,
            Value::string("hello"),
            Value::string("127.0.0.1"),
            Value::int(9999),
        ]);
        assert_eq!(bits, SIG_YIELD | SIG_IO);
        socket.as_external::<Port>().unwrap().close();
    }

    #[test]
    fn test_udp_recv_from_returns_sig_io() {
        let (_, socket) = prim_udp_bind(&[Value::string("127.0.0.1"), Value::int(0)]);
        let (bits, _) = prim_udp_recv_from(&[socket, Value::int(1024)]);
        assert_eq!(bits, SIG_YIELD | SIG_IO);
        socket.as_external::<Port>().unwrap().close();
    }
}
