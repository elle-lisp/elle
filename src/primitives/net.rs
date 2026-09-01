//! Network primitives — TCP and UDP.
//!
//! Unix domain socket primitives are in `unix.rs`.
//!
//! Listener/bind primitives are synchronous (no SIG_IO) because they
//! complete immediately. Accept/connect/send/recv/shutdown yield SIG_IO
//! for scheduler dispatch.

use crate::io::request::{ConnectAddr, IoOp, IoRequest, PortOp};
use crate::port::{Direction, Port, PortKind};
use crate::primitives::ctx::NativeCtx;
use crate::primitives::def::RegionEffect;
use crate::primitives::kwarg::{extract_connect_kwargs, extract_keyword_timeout};
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_IO, SIG_OK};
use crate::value::types::Arity;
use crate::value::Value;

use std::os::unix::io::{FromRawFd, OwnedFd};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub(crate) fn extract_port_of_kind(
    value: &Value,
    expected: PortKind,
    prim_name: &str,
    ctx: &mut NativeCtx,
) -> Result<Value, (SignalBits, Value)> {
    let port = value.as_external::<Port>().ok_or_else(|| {
        (
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!("{}: expected port, got {}", prim_name, value.type_name()),
            ),
        )
    })?;
    if port.kind() != expected {
        return Err((
            SIG_ERROR,
            ctx.error(
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
    ctx: &mut NativeCtx,
) -> Result<String, (SignalBits, Value)> {
    value.with_string(|s| s.to_string()).ok_or_else(|| {
        (
            SIG_ERROR,
            ctx.error(
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

fn extract_port_num(
    value: &Value,
    prim_name: &str,
    ctx: &mut NativeCtx,
) -> Result<u16, (SignalBits, Value)> {
    match value.as_int() {
        Some(n) if (0..=65535).contains(&n) => Ok(n as u16),
        Some(n) => Err((
            SIG_ERROR,
            ctx.error(
                "value-error",
                format!("{}: port must be 0-65535, got {}", prim_name, n),
            ),
        )),
        None => Err((
            SIG_ERROR,
            ctx.error(
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
    ctx: &mut NativeCtx,
) -> Result<i32, (SignalBits, Value)> {
    match ctx.keyword_spelling(*value).as_deref() {
        Some("read") => Ok(libc::SHUT_RD),
        Some("write") => Ok(libc::SHUT_WR),
        Some("read-write") => Ok(libc::SHUT_RDWR),
        Some(other) => Err((
            SIG_ERROR,
            ctx.error(
                "value-error",
                format!(
                    "{}: expected :read, :write, or :read-write, got :{}",
                    prim_name, other
                ),
            ),
        )),
        None => Err((
            SIG_ERROR,
            ctx.error(
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
    ctx: &mut NativeCtx,
) -> Result<(OwnedFd, String), (SignalBits, Value)> {
    use std::net::ToSocketAddrs;

    let addr_str = format!("{}:{}", addr, port);
    let resolved = addr_str
        .to_socket_addrs()
        .map_err(|e| {
            (
                SIG_ERROR,
                ctx.error("io-error", format!("{}: {}", prim_name, e)),
            )
        })?
        .next()
        .ok_or_else(|| {
            (
                SIG_ERROR,
                ctx.error(
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
            ctx.error(
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
    let (sa_bytes, sa_len) = crate::io::sockaddr::build_inet(&resolved);
    let bind_result = unsafe { libc::bind(fd, sa_bytes.as_ptr() as *const libc::sockaddr, sa_len) };

    if bind_result < 0 {
        let err = std::io::Error::last_os_error();
        unsafe { libc::close(fd) };
        return Err((
            SIG_ERROR,
            ctx.error("io-error", format!("{}: bind: {}", prim_name, err)),
        ));
    }

    if listen {
        let ret = unsafe { libc::listen(fd, 128) };
        if ret < 0 {
            let err = std::io::Error::last_os_error();
            unsafe { libc::close(fd) };
            return Err((
                SIG_ERROR,
                ctx.error("io-error", format!("{}: listen: {}", prim_name, err)),
            ));
        }
    }

    // Get actual bound address (for port 0)
    let bound_addr = crate::io::sockaddr::local_address(fd);

    unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
    let owned_fd = unsafe { OwnedFd::from_raw_fd(fd) };
    Ok((owned_fd, bound_addr))
}

mod tcp;
mod udp;
use tcp::*;
use udp::*;

primitive! {
    "tcp/listen" => prim_tcp_listen {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Bind and listen on a TCP address. Returns a listener port.",
        params: &["addr", "port"],
        category: "tcp",
        example: "(tcp/listen \"127.0.0.1\" 8080)",
        effect: RegionEffect::Fresh,
    }
    "tcp/accept" => prim_tcp_accept {
        signal: Signal::io_yields_errors(),
        arity: Arity::AtLeast(1),
        doc: "Accept a connection on a TCP listener. Returns a stream port.",
        params: &["listener"],
        category: "tcp",
        example: "(tcp/accept listener)",
        // Fresh: the stream port is pre-minted in this call's ctx region
        // (`accept_port = ctx.external(..)`); the completion sets its fd in place
        // and returns the same `*accept_port`. Yields → oracle-exempt.
        effect: RegionEffect::Fresh,
    }
    "tcp/connect-ip" => prim_tcp_connect_ip {
        signal: Signal::io_yields_errors(),
        arity: Arity::AtLeast(2),
        doc: "Connect to a TCP endpoint by IP literal (IPv4 or IPv6). Hostnames are rejected — the stdlib tcp/connect wrapper resolves names and calls this per address. Returns a stream port.",
        params: &["ip", "port"],
        category: "tcp",
        example: "(tcp/connect-ip \"127.0.0.1\" 8080)",
        // Fresh: the stream port is pre-minted in this call's ctx region
        // (`port_val = ctx.external(..)`); the completion sets its fd in place.
        effect: RegionEffect::Fresh,
    }
    "tcp/shutdown" => prim_tcp_shutdown {
        signal: Signal::io_yields_errors(),
        arity: Arity::Exact(2),
        doc: "Shutdown a TCP stream. how: :read, :write, or :read-write.",
        params: &["port", "how"],
        category: "tcp",
        example: "(tcp/shutdown conn :write)",
        // Immediate: the completion returns Value::NIL. Yields → oracle-exempt.
        effect: RegionEffect::Immediate,
    }
    "udp/bind" => prim_udp_bind {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Bind a UDP socket. Returns a UDP port.",
        params: &["addr", "port"],
        category: "udp",
        example: "(udp/bind \"0.0.0.0\" 9000)",
        effect: RegionEffect::Fresh,
    }
    "udp/send-to" => prim_udp_send_to {
        signal: Signal::io_yields_errors(),
        arity: Arity::AtLeast(4),
        doc: "Send data to a remote address via UDP. Returns bytes sent.",
        params: &["socket", "data", "addr", "port"],
        category: "udp",
        example: "(udp/send-to sock \"hello\" \"127.0.0.1\" 9000)",
        // The io completion returns `Value::int(result_code)` (bytes sent), so
        // the result is always an immediate. `Immediate` records no may-store
        // edges — `udp/send-to` takes THREE heap args (socket + data + addr
        // string) but stores none into another (the `data` it ships rides into
        // the kernel; `addr` is copied out to a Rust String), so the `Mixed`
        // clique only leaked. The `data` value is held in the IoRequest across
        // the yield, but it stays pinned by the suspended caller frame (whose
        // `DecrefValueRegion` is suspended too), so dropping the clique cannot
        // free it early. Pinned by `udp_send_to_declares_immediate_no_arg_clique`
        // (no clique) and region-udp-send-effect.lisp (resumed int). The
        // result side is oracle-exempt (always yields).
        effect: RegionEffect::Immediate,
    }
    "udp/recv-from" => prim_udp_recv_from {
        signal: Signal::io_yields_errors(),
        arity: Arity::AtLeast(2),
        doc: "Receive data from a UDP socket. Returns {:data :addr :port}.",
        params: &["socket", "count"],
        category: "udp",
        example: "(udp/recv-from sock 1024)",
        // Fresh: the `{:data :addr :port}` struct and its buffers are pre-minted
        // in this call's ctx region; the completion fills them in place and
        // returns the same `*result` — nothing is born on the scheduler heap.
        effect: RegionEffect::Fresh,
    }
    "sys/resolve" => prim_sys_resolve {
        signal: Signal::io_yields_errors(),
        arity: Arity::Exact(1),
        doc: "Resolve a hostname to IP addresses via the system resolver (getaddrinfo). Returns an array of IP address strings.",
        params: &["hostname"],
        category: "sys",
        example: "(sys/resolve \"localhost\")",
        // Opaque: stores nothing (hostname copied to a Rust String), but the IP
        // array is minted at completion on the origin heap (portless Resolve),
        // neither this call's region nor an arg's. No clique, non-fresh result.
        effect: RegionEffect::Opaque,
    }
    "sys/ip?" => prim_sys_ip_p {
        arity: Arity::Exact(1),
        doc: "True if the argument is a string holding an IPv4 or IPv6 address literal (e.g. \"127.0.0.1\", \"::1\"). Hostnames, bracketed or port-suffixed addresses, and non-strings are false. Synchronous — does no resolution; tcp/connect uses it to skip sys/resolve for IP literals.",
        params: &["value"],
        category: "predicate",
        example: "(sys/ip? \"127.0.0.1\") #=> true",
        effect: RegionEffect::Immediate,
    }
}

// Tests migrated to tests/elle/prim-net.lisp
