//! Where a read's answer ends in the bytes it owns, and how that answer is
//! handed back.
//!
//! A read is answered from two places. The port may already hold enough — the
//! remainder a previous read took past what it answered with — in which case the
//! submission answers on the spot and no backend runs. Otherwise the completion
//! joins that remainder to the bytes the read produced and answers from the
//! join. Both cut the answer out of a `&[u8]` by the same rules, so the rules
//! live here rather than once at each site, where they could drift apart and
//! frame the same stream two ways.

use crate::port::Encoding;
use crate::value::Value;

/// Where the line ends in `all`, and where the port's remainder starts.
///
/// A newline is the boundary: it is dropped, a `\r` before it goes with it, and
/// the bytes after it belong to the next read on this port. With no newline the
/// whole of `all` is the answer — a partial last line, which is what a stream
/// that ended mid-line leaves.
pub(crate) fn line_end(all: &[u8]) -> (usize, usize) {
    match all.iter().position(|&b| b == b'\n') {
        Some(pos) => {
            let end = if pos > 0 && all[pos - 1] == b'\r' {
                pos - 1
            } else {
                pos
            };
            (end, pos + 1)
        }
        None => (all.len(), all.len()),
    }
}

/// Where `count` units end in `all`, or `None` when `all` does not hold that
/// many.
///
/// The unit is the port's own: bytes on a binary port, grapheme clusters on a
/// text one. `read-exact` is all-or-nothing, so `None` is the answer a caller
/// gets as nil rather than as a short result.
pub(crate) fn exact_end(
    all: &[u8],
    count: usize,
    encoding: Encoding,
    gen: crate::segment::Generation,
) -> Option<usize> {
    match encoding {
        Encoding::Text => crate::io::nth_grapheme_byte_end(all, count, gen),
        Encoding::Binary => (all.len() >= count).then_some(count),
    }
}

/// Hand `bytes` back as the value a read answers with.
///
/// The fiber's own buffer is preferred: it was born in the caller's region, and
/// a text port's result is that same allocation transmuted in place rather than
/// a copy. It cannot always be used. A grapheme cluster has no upper bound in
/// bytes, and neither has a line, so a read can answer with more bytes than any
/// count could have reserved; that result is born on the requesting instance's
/// heap instead, exactly as `read-all`'s is. Clamping to the buffer instead
/// would drop the bytes past it, and they are bytes the port has already taken
/// from the kernel — nothing would be left to read them again.
pub(crate) fn read_result(
    buffer: &Value,
    bytes: Vec<u8>,
    encoding: Encoding,
    origin_heap: *mut crate::value::fiberheap::FiberHeap,
) -> Result<Value, Value> {
    // SAFETY: the buffer is the requesting fiber's pre-allocated LBytes and that
    // fiber is parked until this answer reaches it.
    let (dst, cap) = unsafe { crate::io::request::writeable_buffer_ptr(buffer) };
    let value = if bytes.len() <= cap {
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len());
            crate::io::request::truncate_buffer(buffer, bytes.len());
        }
        *buffer
    } else {
        let heap = unsafe { &mut *crate::io::completion_heap_ptr(origin_heap) };
        let ctx = crate::primitives::ctx::Alloc::new(heap);
        ctx.bytes(bytes)
    };
    if encoding == Encoding::Text {
        // SAFETY: `value` is an LBytes this call owns — either the parked
        // fiber's buffer or an allocation made just above.
        unsafe { crate::io::request::bytes_to_string_in_place(value, origin_heap) }
    } else {
        Ok(value)
    }
}

#[cfg(test)]
mod tests;
