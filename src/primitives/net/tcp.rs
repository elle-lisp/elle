use super::*;

// ---------------------------------------------------------------------------
// TCP primitives
// ---------------------------------------------------------------------------

/// (tcp/listen addr port) → listener-port
pub(super) fn prim_tcp_listen(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let addr = match extract_string(&args[0], "addr", "tcp/listen", ctx) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let port = match extract_port_num(&args[1], "tcp/listen", ctx) {
        Ok(p) => p,
        Err(e) => return e,
    };

    match bind_socket(&addr, port, libc::SOCK_STREAM, true, "tcp/listen", ctx) {
        Ok((fd, bound_addr)) => {
            let p = Port::new_tcp_listener(fd, bound_addr);
            (SIG_OK, ctx.external("port", p))
        }
        Err(e) => e,
    }
}

/// (tcp/accept listener [:sndbuf n] [:rcvbuf n] [:nodelay bool] [:keepalive bool]
///                       [:encoding :text|:binary] [:timeout ms]) → stream-port
///
/// `:encoding` controls the resulting stream port's mode.  Default is
/// `:binary` (POSIX-style: a TCP connection is a byte stream).  Pass
/// `:text` for line-oriented text protocols (SMTP, IRC, plain HTTP/1.x)
/// — then `port/read` and `port/read-exact` return strings and treat
/// `n` as graphemes (the unit Elle strings are measured in).
pub(super) fn prim_tcp_accept(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let port_val = match extract_port_of_kind(&args[0], PortKind::TcpListener, "tcp/accept", ctx) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let kwargs = match extract_connect_kwargs(args, 1, "tcp/accept", ctx) {
        Ok(k) => k,
        Err(e) => return e,
    };
    let encoding = kwargs.encoding.unwrap_or(crate::port::Encoding::Binary);
    let accept_port = ctx.external(
        "port",
        Port::new_unopened(
            PortKind::TcpStream,
            Direction::ReadWrite,
            encoding,
            String::new(),
        ),
    );
    (
        SIG_YIELD | SIG_IO,
        IoRequest::with_timeout(
            ctx,
            IoOp::Accept {
                options: kwargs.options,
                encoding,
                accept_port,
            },
            port_val,
            kwargs.timeout,
        ),
    )
}

/// (tcp/connect-ip ip port [:sndbuf n] [:rcvbuf n] [:nodelay bool] [:keepalive bool]
///                          [:encoding :text|:binary] [:timeout ms]) → stream-port
///
/// The IP-only connect primitive: `ip` must parse as an IPv4 or IPv6 literal
/// (e.g. `"127.0.0.1"`, `"::1"`). A hostname is rejected synchronously — name
/// resolution is the stdlib `tcp/connect` wrapper's job, which calls `sys/resolve`
/// then this primitive for each returned address. Keeping the primitive IP-only
/// lets the backend hand the address straight to the kernel with no blocking
/// getaddrinfo fallback.
///
/// `:encoding` controls the resulting stream port's mode.  Default is
/// `:binary` (POSIX-style: a TCP connection is a byte stream).  Pass
/// `:text` for line-oriented text protocols (SMTP, IRC, plain HTTP/1.x).
pub(super) fn prim_tcp_connect_ip(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let addr = match extract_string(&args[0], "ip", "tcp/connect-ip", ctx) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let port = match extract_port_num(&args[1], "tcp/connect-ip", ctx) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let ip = match addr.parse::<std::net::IpAddr>() {
        Ok(ip) => ip,
        Err(_) => {
            return (
                SIG_ERROR,
                ctx.error(
                    "value-error",
                    format!(
                        "tcp/connect-ip: expected an IP address, got {:?}; \
                         use tcp/connect for hostnames",
                        addr
                    ),
                ),
            )
        }
    };
    let kwargs = match extract_connect_kwargs(args, 2, "tcp/connect-ip", ctx) {
        Ok(k) => k,
        Err(e) => return e,
    };
    let peer = crate::io::sockaddr::format_host_port(&ip.to_string(), port);
    let encoding = kwargs.encoding.unwrap_or(crate::port::Encoding::Binary);
    let port_val = ctx.external(
        "port",
        Port::new_unopened(PortKind::TcpStream, Direction::ReadWrite, encoding, peer),
    );
    (
        SIG_YIELD | SIG_IO,
        IoRequest::with_timeout(
            ctx,
            IoOp::Connect {
                addr: ConnectAddr::Tcp {
                    addr: ip,
                    port,
                    options: kwargs.options,
                    encoding,
                },
            },
            port_val,
            kwargs.timeout,
        ),
    )
}

/// (sys/ip? value) → boolean
///
/// True iff `value` is a string that parses as an IPv4 or IPv6 address literal
/// (e.g. `"127.0.0.1"`, `"::1"`). Total and synchronous: a non-string, or a
/// string that is not a bare IP (a hostname, or a bracketed/port-suffixed
/// address), is `false` — never an error. `tcp/connect` branches on it to skip a
/// redundant `sys/resolve` pool round-trip when the host is already an IP.
pub(super) fn prim_sys_ip_p(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let is_ip = args[0]
        .with_string(|s| s.parse::<std::net::IpAddr>().is_ok())
        .unwrap_or(false);
    (SIG_OK, Value::bool(is_ip))
}

/// (tcp/shutdown port how) → nil
pub(super) fn prim_tcp_shutdown(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let port_val = match extract_port_of_kind(&args[0], PortKind::TcpStream, "tcp/shutdown", ctx) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let how = match parse_shutdown_how(&args[1], "tcp/shutdown", ctx) {
        Ok(h) => h,
        Err(e) => return e,
    };
    (
        SIG_YIELD | SIG_IO,
        IoRequest::new(ctx, IoOp::Shutdown { how }, port_val),
    )
}
