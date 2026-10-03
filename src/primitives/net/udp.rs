// audited: 2026-09-30
//! The UDP primitives: bind, send a datagram, receive one, and resolve a hostname.
//!
//! docs/io.md
//! docs/io/timeout.md

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

/// (udp/send-to socket data addr port [:timeout s] [:deadline t]) → bytes-sent
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
    let bound = match extract_bound(args, 4, "udp/send-to", ctx) {
        Ok(b) => b,
        Err(e) => return e,
    };
    (
        SIG_IO,
        IoRequest::bounded(
            ctx,
            PortOp::SendTo {
                addr,
                port_num,
                data,
            }
            .into(),
            socket_val,
            bound,
        ),
    )
}

/// (udp/recv-from socket count [:timeout s] [:deadline t]) → {:data bytes :addr string :port int}
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
    let bound = match extract_bound(args, 2, "udp/recv-from", ctx) {
        Ok(b) => b,
        Err(e) => return e,
    };
    // Pre-allocate the result struct on THIS (the requesting) fiber's heap, the
    // same discipline as `port/read`'s buffer. The completion fills these
    // buffers in place (kernel writes the payload straight into `:data`) instead
    // of instantiating fresh values on the scheduler's heap — otherwise the
    // region-backed payload is freed before `fiber/resume` hands it back and the
    // datagram arrives zeroed. `:addr` is an LBytes buffer the completion fills
    // then transmutes to a string in place.
    let result = {
        use crate::value::heap::TableKey;
        let mut fields = std::collections::BTreeMap::new();
        fields.insert(TableKey::keyword("data"), ctx.bytes(vec![0u8; count]));
        // INET6_ADDRSTRLEN is 46; 64 gives slack and the completion truncates to
        // the real length before transmuting the buffer to a string.
        fields.insert(TableKey::keyword("addr"), ctx.bytes(vec![0u8; 64]));
        fields.insert(TableKey::keyword("port"), Value::int(0));
        ctx.struct_from(fields)
    };
    (
        SIG_IO,
        IoRequest::bounded(
            ctx,
            PortOp::RecvFrom { count, result }.into(),
            socket_val,
            bound,
        ),
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
    (SIG_IO, IoRequest::portless(ctx, IoOp::Resolve { hostname }))
}
