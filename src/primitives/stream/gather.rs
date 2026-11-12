// audited: 2026-11-12
//! `port/write-many`: a sequence of parts sent as one gathered write, each part from where it lies.
//!
//! docs/io/write.md
//! docs/impl/io-bytes.md

use super::extract_port_value;
use crate::io::request::{IoRequest, PortOp};
use crate::primitives::def::RegionEffect;
use crate::primitives::kwarg::extract_bound;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_IO, SIG_OK};
use crate::value::types::Arity;
use crate::value::Value;

/// The parts of an array, an `@array` or a proper list. `None` for any other
/// value, `nil` included.
fn parts_of(parts: Value) -> Option<Vec<Value>> {
    if let Some(elements) = parts.as_array() {
        return Some(elements.to_vec());
    }
    if let Some(read) = parts.array_mut_read() {
        return Some(read.to_vec());
    }
    if parts.is_empty_list() || parts.is_pair() {
        return parts.list_to_vec().ok();
    }
    None
}

/// The bytes in a string, bytes, `@string` or `@bytes`. `None` for any other
/// value.
fn part_size(part: Value) -> Option<usize> {
    part.as_bytes()
        .map(<[u8]>::len)
        .or_else(|| part.as_str().map(str::len))
        .or_else(|| part.bytes_mut_read().map(|b| b.len()))
        .or_else(|| part.string_mut_read().map(|b| b.len()))
}

/// (port/write-many port parts [:timeout s] [:deadline t]) → int
///
/// Every part is checked before anything is written, so a refused call sends
/// no byte. Parts that hold no byte between them are answered at once.
fn prim_stream_write_many(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let port = match extract_port_value(&args[0], "port/write-many", ctx) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let bound = match extract_bound(args, 2, "port/write-many", ctx) {
        Ok(b) => b,
        Err(e) => return e,
    };
    let Some(parts) = parts_of(args[1]) else {
        return type_error!(
            ctx,
            args[1],
            "port/write-many",
            "an array or a list of parts"
        );
    };
    let mut total = 0;
    for part in parts {
        match part_size(part) {
            Some(n) => total += n,
            None => {
                return type_error!(ctx, part, "port/write-many", "a string or bytes as a part")
            }
        }
    }
    if total == 0 {
        return (SIG_OK, Value::int(0));
    }
    (
        SIG_IO,
        IoRequest::bounded(ctx, PortOp::write_many(args[1]).into(), port, bound),
    )
}

primitive! {
    "port/write-many" => prim_stream_write_many {
        signal: Signal::io_yields_errors(),
        arity: Arity::AtLeast(2),
        doc: "Write every byte of every part to port, in order, as one gathered \
              write. parts is an array or a list of strings and bytes. Returns \
              the bytes written, the sum over the parts. An immutable part is \
              sent from where it lies; mutable parts, or parts in an @array, \
              are copied once.",
        params: &["port", "parts"],
        category: "port",
        example: "(port/write-many (port/stdout) [\"hello, \" \"world\\n\"])",
        aliases: &[],
        // Immediate, as `port/write` is and for the same reason: the result is
        // a byte count, and the call stores neither the port nor the parts.
        // Pinned by `port_write_many_declares_immediate_no_arg_clique`.
        effect: RegionEffect::Immediate,
    }
}
