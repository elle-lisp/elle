use super::*;

// ---------------------------------------------------------------------------
// UDP primitives
// ---------------------------------------------------------------------------

/// (udp/bind addr port) → udp-port
pub(super) fn prim_udp_bind(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let addr = match extract_string(&args[0], "addr", "udp/bind", ctx) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let port = match extract_port_num(&args[1], "udp/bind", ctx) {
        Ok(p) => p,
        Err(e) => return e,
    };

    match bind_socket(&addr, port, libc::SOCK_DGRAM, false, "udp/bind", ctx) {
        Ok((fd, bound_addr)) => {
            let p = Port::new_udp_socket(fd, bound_addr);
            (SIG_OK, ctx.external("port", p))
        }
        Err(e) => e,
    }
}

/// (udp/send-to socket data addr port [:timeout ms]) → bytes-sent
pub(super) fn prim_udp_send_to(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let socket_val = match extract_port_of_kind(&args[0], PortKind::UdpSocket, "udp/send-to", ctx) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let data = args[1];
    let addr = match extract_string(&args[2], "addr", "udp/send-to", ctx) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let port_num = match extract_port_num(&args[3], "udp/send-to", ctx) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let timeout = match extract_keyword_timeout(args, 4, "udp/send-to", ctx) {
        Ok(t) => t,
        Err(e) => return e,
    };
    (
        SIG_YIELD | SIG_IO,
        IoRequest::with_timeout(
            ctx,
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
pub(super) fn prim_udp_recv_from(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let socket_val = match extract_port_of_kind(&args[0], PortKind::UdpSocket, "udp/recv-from", ctx)
    {
        Ok(v) => v,
        Err(e) => return e,
    };
    let count = match args[1].as_int() {
        Some(n) if n > 0 => n as usize,
        Some(n) => {
            return (
                SIG_ERROR,
                ctx.error(
                    "value-error",
                    format!("udp/recv-from: count must be positive, got {}", n),
                ),
            )
        }
        None => return type_error!(ctx, args[1], "udp/recv-from", "integer for count"),
    };
    let timeout = match extract_keyword_timeout(args, 2, "udp/recv-from", ctx) {
        Ok(t) => t,
        Err(e) => return e,
    };
    // Pre-allocate the result struct on THIS (the requesting) fiber's heap, the
    // same discipline as `port/read`'s buffer. The completion fills these
    // buffers in place (kernel writes the payload straight into `:data`) instead
    // of instantiating fresh values on the scheduler's heap — otherwise the
    // region-backed payload is freed before `fiber/resume` hands it back and the
    // datagram arrives zeroed (the arena-lifetime bug). `:addr` is an LBytes
    // buffer the completion fills then transmutes to a string in place.
    let result = {
        use crate::value::heap::TableKey;
        let mut fields = std::collections::BTreeMap::new();
        fields.insert(
            TableKey::Keyword("data".into()),
            ctx.bytes(vec![0u8; count]),
        );
        // INET6_ADDRSTRLEN is 46; 64 gives slack and the completion truncates to
        // the real length before transmuting the buffer to a string.
        fields.insert(TableKey::Keyword("addr".into()), ctx.bytes(vec![0u8; 64]));
        fields.insert(TableKey::Keyword("port".into()), Value::int(0));
        ctx.struct_from(fields)
    };
    (
        SIG_YIELD | SIG_IO,
        IoRequest::with_timeout(ctx, IoOp::RecvFrom { count, result }, socket_val, timeout),
    )
}
/// (sys/resolve hostname) → array of IP address strings
pub(super) fn prim_sys_resolve(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let hostname = match extract_string(&args[0], "hostname", "sys/resolve", ctx) {
        Ok(s) => s,
        Err(e) => return e,
    };
    (
        SIG_YIELD | SIG_IO,
        IoRequest::portless(ctx, IoOp::Resolve { hostname }),
    )
}
