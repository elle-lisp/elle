//! audited: 2026-09-23
//! Where a read's answer ends in the bytes it owns, and how that answer is
//! handed back.
//!
//! src/io/AGENTS.md
//! docs/impl/io-bytes.md
//!
//! A read is answered from two places. The port may already hold enough — the
//! remainder a previous read took past what it answered with — in which case the
//! submission answers on the spot and no backend runs. Otherwise the completion
//! answers from the bytes the read landed, with any remainder in front. Both cut
//! the answer out of a `&[u8]` by the same rules, so the rules live here rather
//! than once at each site, where they could drift apart and frame the same
//! stream two ways.

use crate::io::request::PortOp;
use crate::port::Encoding;
use crate::value::Value;

/// What a buffered read asks for, in the terms its answer is cut by.
#[derive(Clone, Copy)]
pub(crate) enum Ask {
    /// One line: `port/read-line`.
    Line,
    /// Up to this many bytes: `port/read`.
    UpTo(usize),
    /// Exactly this many units: `port/read-exact`.
    Exact(usize),
}

impl Ask {
    /// The ask a buffered read makes, or `None` for any other operation.
    pub(crate) fn of(op: &PortOp) -> Option<Ask> {
        match op {
            PortOp::ReadLine { .. } => Some(Ask::Line),
            PortOp::Read { count, .. } => Some(Ask::UpTo(*count)),
            PortOp::ReadExact { count, .. } => Some(Ask::Exact(*count)),
            _ => None,
        }
    }

    /// The encoding the answer carries: a line is text whatever the port is
    /// measured in.
    pub(crate) fn answer_encoding(self, port: Encoding) -> Encoding {
        match self {
            Ask::Line => Encoding::Text,
            Ask::UpTo(_) | Ask::Exact(_) => port,
        }
    }
}

/// Where the answer to `ask` ends in `all`, and where the port's remainder
/// starts. `None` is a request `all` cannot answer: nothing at all for a line
/// or an up-to read, too few units for an exact one.
///
/// `port/read` answers with up to `count` bytes — whatever arrived — so only an
/// empty stream leaves it nothing to say. `read-exact` is all-or-nothing: a
/// stream that ended before the count yields nil, and the partial goes with it,
/// so a caller can tell "got n" from "ended early".
pub(crate) fn span(
    ask: Ask,
    all: &[u8],
    encoding: Encoding,
    gen: crate::segment::Generation,
) -> Option<(usize, usize)> {
    match ask {
        Ask::Line => (!all.is_empty()).then(|| line_end(all)),
        Ask::UpTo(count) => (!all.is_empty()).then(|| {
            let end = all.len().min(count);
            (end, end)
        }),
        Ask::Exact(count) => exact_end(all, count, encoding, gen).map(|end| (end, end)),
    }
}

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

/// Answer with the first `len` bytes of the caller's buffer, where the read
/// already landed them: truncate the buffer there, and on a text port transmute
/// it to a string in place. No byte moves.
pub(crate) fn answer_in_buffer(
    buffer: &Value,
    len: usize,
    encoding: Encoding,
    birth: &mut crate::io::Birthplace,
) -> Result<Value, Value> {
    // SAFETY: the buffer is the requesting fiber's pre-allocated LBytes, that
    // fiber is parked until this answer reaches it, and `len` is within it.
    unsafe { crate::io::request::truncate_buffer(buffer, len) };
    as_answer(*buffer, encoding, birth)
}

/// Answer with `bytes`, which did not land in the caller's buffer: a remainder
/// joined to what the read produced, or bytes a stdin read hands over.
///
/// The fiber's own buffer is preferred: it was born in the caller's region, and
/// a text port's result is that same allocation transmuted in place rather than
/// a copy. It cannot always be used. A grapheme cluster has no upper bound in
/// bytes, and neither has a line, so a read can answer with more bytes than any
/// count could have reserved; that result is built at the completion's
/// `Birthplace` instead, exactly as `read-all`'s is. Clamping to the buffer
/// instead would drop the bytes past it, and they are bytes the port has already
/// taken from the kernel — nothing would be left to read them again.
///
/// Which of the two a read takes is what decides whether the birthplace coins
/// anything: the buffer path allocates nothing there, so a read that fits owes
/// the handover nothing.
pub(crate) fn answer_from(
    buffer: &Value,
    bytes: &[u8],
    encoding: Encoding,
    birth: &mut crate::io::Birthplace,
) -> Result<Value, Value> {
    // SAFETY: as in `answer_in_buffer`.
    let (dst, cap) = unsafe { crate::io::request::writeable_buffer_ptr(buffer) };
    let value = if bytes.len() <= cap {
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len());
            crate::io::request::truncate_buffer(buffer, bytes.len());
        }
        *buffer
    } else {
        birth.alloc().joined_bytes(&[bytes])
    };
    as_answer(value, encoding, birth)
}

/// `value`, an `LBytes` this answer owns, as the value a read answers with.
fn as_answer(
    value: Value,
    encoding: Encoding,
    birth: &mut crate::io::Birthplace,
) -> Result<Value, Value> {
    if encoding == Encoding::Text {
        // SAFETY: `value` is an LBytes this call owns — either the parked
        // fiber's buffer or an allocation made at the birthplace.
        unsafe { crate::io::request::bytes_to_string_in_place(value, birth) }
    } else {
        Ok(value)
    }
}

#[cfg(test)]
mod tests;
