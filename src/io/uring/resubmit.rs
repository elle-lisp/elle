//! audited: 2026-09-23
//! What a live operation does after a CQE: complete, or go out again for the
//! rest of its bytes.
//!
//! src/io/AGENTS.md
//! docs/impl/io-bytes.md
//!
//! A read goes out again until it has its answer, a `read-all` until EOF, and
//! a write until its payload is gone. Each resubmission reads into the caller's
//! buffer, the accumulation, or the payload exactly where the first SQE did.

use super::stream::{read_all_into, read_into, write_from, write_source};
use super::*;
use crate::io::grapheme_count_in_valid_prefix;
use crate::port::Encoding;

/// What an operation does after a CQE.
pub(super) enum Next {
    /// Submit this SQE and keep the entry.
    Again(io_uring::squeue::Entry),
    /// Cook a completion from this result, these bytes, and this pooled
    /// buffer, which the completion releases.
    Complete {
        result_code: i32,
        data: Vec<u8>,
        buf_handle: Option<BufferHandle>,
    },
}

/// The shapes that differ in what they do after a CQE.
enum Shape {
    /// `read`, `read-line`, `read-exact`: into the caller's buffer.
    Buffered,
    ReadAll,
    Write,
    RecvFrom,
    /// A watch or signal read: a batch in the pooled buffer.
    Batch,
    Other,
}

/// Decide what `op` does after a CQE carrying `result_code`.
pub(super) fn next(
    id: SubmissionId,
    op: &mut PendingOp,
    result_code: i32,
    buffer_pool: &mut BufferPool,
    fd_states: &mut HashMap<PortKey, FdState>,
    gen: crate::segment::Generation,
) -> Next {
    let buf_handle = op.buffer_handle();
    let shape = match &*op {
        PendingOp::Port { op: port_op, .. } => match port_op {
            PortOp::ReadLine { .. } | PortOp::Read { .. } | PortOp::ReadExact { .. } => {
                Shape::Buffered
            }
            PortOp::ReadAll => Shape::ReadAll,
            PortOp::Write { .. } => Shape::Write,
            PortOp::RecvFrom { .. } => Shape::RecvFrom,
            PortOp::Flush
            | PortOp::Accept { .. }
            | PortOp::SendTo { .. }
            | PortOp::Shutdown { .. } => Shape::Other,
        },
        PendingOp::WatchNext { .. } | PendingOp::SigNext { .. } => Shape::Batch,
        _ => Shape::Other,
    };
    let done = |result_code, data| Next::Complete {
        result_code,
        data,
        buf_handle,
    };
    match shape {
        Shape::Buffered if result_code > 0 => {
            after_read(id, op, result_code as usize, fd_states, gen)
        }
        Shape::ReadAll => after_read_all(id, op, result_code, buffer_pool),
        Shape::Write if result_code >= 0 => after_write(id, op, result_code as usize, buffer_pool),
        Shape::RecvFrom if result_code > 0 => {
            let scratch = buffer_pool.get_mut(buf_handle.expect("RecvFrom has scratch"));
            done(result_code, sender_address(scratch))
        }
        Shape::Batch if result_code > 0 => {
            let buf = buffer_pool.get_mut(buf_handle.expect("a batch read has a buffer"));
            done(result_code, buf[..result_code as usize].to_vec())
        }
        _ => done(result_code, Vec::new()),
    }
}

/// A buffered read got `got` more bytes: complete with its answer, or read on
/// into the tail of the caller's buffer.
///
/// The buffer holds a borrowed remainder and every byte this read has taken,
/// so "enough yet?" is asked of the buffer where they lie. Only a remainder too
/// large to lend, or bytes spilled from a full buffer, still sit in the port
/// (`unlent`), and then the question is asked of both.
fn after_read(
    id: SubmissionId,
    op: &mut PendingOp,
    got: usize,
    fd_states: &mut HashMap<PortKey, FdState>,
    gen: crate::segment::Generation,
) -> Next {
    let PendingOp::Port {
        op: port_op,
        port_key,
        port,
        lent,
        filled,
        ..
    } = op
    else {
        unreachable!("after_read: not a port read");
    };
    let buffer = match port_op {
        PortOp::ReadLine { buffer }
        | PortOp::Read { buffer, .. }
        | PortOp::ReadExact { buffer, .. } => *buffer,
        _ => unreachable!("after_read: not a buffered read"),
    };
    let bytes = buffer.as_bytes().expect("a read's buffer is bytes");
    let cap = bytes.len();
    let k = (*filled + got).min(cap);
    let fd = port_key.raw_fd();
    let the_port = port.as_external::<Port>();
    let text = the_port.is_some_and(|p| p.encoding() == Encoding::Text);
    let stream =
        the_port.is_some_and(|p| matches!(p.kind(), PortKind::TcpStream | PortKind::UnixStream));
    let unlent: &[u8] = fd_states.get(port_key).map_or(&[], |s| &s.buffer);

    // `port/read` answers "up to n" on a stream socket, so only a file reads
    // on past a short read. A text `read-exact` counts clusters, which a
    // reservation cannot bound, so it keeps no room for what the port holds.
    let (need_more, clusters) = match port_op {
        PortOp::ReadLine { .. } => (!bytes[*filled..k].contains(&b'\n'), false),
        PortOp::Read { count, .. } => (!stream && unlent.len() + k < *count, false),
        PortOp::ReadExact { count, .. } if text => {
            let counted = if unlent.is_empty() {
                grapheme_count_in_valid_prefix(&bytes[..k], gen)
            } else {
                let mut all = unlent.to_vec();
                all.extend_from_slice(&bytes[..k]);
                grapheme_count_in_valid_prefix(&all, gen)
            };
            (counted < *count, true)
        }
        PortOp::ReadExact { count, .. } => (unlent.len() + k < *count, false),
        _ => unreachable!("after_read: not a buffered read"),
    };
    let complete = |result_code| Next::Complete {
        result_code,
        data: Vec::new(),
        buf_handle: None,
    };
    if !need_more {
        return complete(got as i32);
    }
    let reserve = if clusters { 0 } else { unlent.len() };
    let room = cap.saturating_sub(k).saturating_sub(reserve);
    if room > 0 {
        *filled = k;
        return Next::Again(read_into(id, fd, &buffer, k, room.min(MAX_READ_CHUNK)));
    }
    if clusters {
        // The buffer is full and the clusters are wider than it reserved. Its
        // bytes join the port's remainder, where a cancel leaves them for the
        // next read, and the buffer serves as a chunk from here on; the
        // completion builds the answer outside it (docs/impl/io-bytes.md
        // § "When the answer outgrows the buffer").
        crate::io::types::fd_state_mut(fd_states, port_key)
            .buffer
            .extend_from_slice(&bytes[..k]);
        lent.clear();
        *filled = 0;
        return Next::Again(read_into(id, fd, &buffer, 0, cap.min(MAX_READ_CHUNK)));
    }
    // A line longer than its buffer answers with the full buffer; reading on
    // gives the next piece. Abandoning the operation here instead would leave
    // the fiber that asked parked with no completion coming.
    *filled = k;
    complete(0)
}

/// A `read-all` CQE: account the bytes the kernel put in the accumulation and
/// read on, or hand the accumulation over at EOF.
fn after_read_all(
    id: SubmissionId,
    op: &mut PendingOp,
    result_code: i32,
    buffer_pool: &mut BufferPool,
) -> Next {
    let PendingOp::Port {
        port_key,
        buffer_handle,
        ..
    } = op
    else {
        unreachable!("after_read_all: not a port operation");
    };
    let bh = buffer_handle.expect("ReadAll has an accumulation");
    if result_code < 0 {
        return Next::Complete {
            result_code,
            data: Vec::new(),
            buf_handle: Some(bh),
        };
    }
    if result_code > 0 {
        let acc = buffer_pool.get_mut(bh);
        // SAFETY: the kernel wrote this many bytes into the room past `len`
        // that the SQE named.
        unsafe { acc.set_len(acc.len() + result_code as usize) };
        return Next::Again(read_all_into(id, port_key.raw_fd(), acc));
    }
    // The stream ended. The accumulation is the answer's bytes; releasing it
    // hands them over, so nothing is left for the completion to release.
    *buffer_handle = None;
    Next::Complete {
        result_code: 0,
        data: buffer_pool.release(bh),
        buf_handle: None,
    }
}

/// A write moved `got` more bytes: complete once the payload is gone, or write
/// the tail from the same source.
///
/// One write(2) transfers only what fits in the fd's send buffer, which on a
/// socket is routinely a fraction of a large payload, and `port/write` writes
/// every byte before it returns (docs/io.md). The completion counts `filled +
/// got`.
fn after_write(
    id: SubmissionId,
    op: &mut PendingOp,
    got: usize,
    buffer_pool: &mut BufferPool,
) -> Next {
    let PendingOp::Port {
        op: PortOp::Write { data },
        port_key,
        buffer_handle,
        filled,
        ..
    } = op
    else {
        unreachable!("after_write: not a write");
    };
    let (data, bh, fd) = (*data, *buffer_handle, port_key.raw_fd());
    let source = write_source(&data, bh, buffer_pool);
    let complete = |result_code| Next::Complete {
        result_code,
        data: Vec::new(),
        buf_handle: bh,
    };
    if *filled + got >= source.len() {
        return complete(got as i32);
    }
    if got == 0 {
        // The kernel accepted nothing of a non-empty tail. Resubmitting would
        // spin, and reporting the partial count would read as a completed
        // write to a caller that trusts the contract — so fail the operation.
        return complete(-libc::EIO);
    }
    *filled += got;
    Next::Again(write_from(id, fd, &source[*filled..]))
}

/// The sender of a datagram `RecvMsg` received, encoded as `addr_len` (4 bytes,
/// little-endian) then the `sockaddr_storage`.
///
/// The scratch layout is `[msghdr | iovec | sockaddr_storage]`. The payload is
/// not in it: the iovec points straight at the result's `:data` buffer, so the
/// datagram was received into the value the fiber resumes with. The completion
/// truncates `:data` to the result code and fills `:addr`/`:port` from this.
fn sender_address(scratch: &[u8]) -> Vec<u8> {
    let msghdr_size = std::mem::size_of::<libc::msghdr>();
    let iovec_size = std::mem::size_of::<libc::iovec>();
    let sockaddr_size = std::mem::size_of::<libc::sockaddr_storage>();
    // The kernel updated `msg_namelen` to the address's real length.
    // SAFETY: the scratch starts with the msghdr the submission laid out.
    let addr_len = unsafe { (*(scratch.as_ptr() as *const libc::msghdr)).msg_namelen };
    let sa_start = msghdr_size + iovec_size;
    let mut encoded = Vec::with_capacity(4 + sockaddr_size);
    encoded.extend_from_slice(&addr_len.to_le_bytes());
    encoded.extend_from_slice(&scratch[sa_start..sa_start + sockaddr_size]);
    encoded
}
