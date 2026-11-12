//! audited: 2026-11-12
//! The SQE a write submits, first and on every resubmission: one write from a
//! part or a copy, or a gathered write of several parts.
//!
//! docs/impl/io-bytes.md

use super::*;
use crate::io::outgoing::{tail_iovecs, Outgoing};

/// Whether a write takes a pooled buffer: for the copy of bytes that must be
/// copied, or for the iovecs of a gathered write of several parts, which must
/// outlive every SQE that names them.
pub(crate) fn write_needs_buffer(data: &Outgoing) -> bool {
    data.in_region()
        .is_none_or(|parts| parts.iter().filter(|p| !p.is_empty()).count() > 1)
}

/// Copy the bytes of a write that cannot be sent where they lie into its
/// pooled buffer, once, at submission.
pub(super) fn copy_if_needed(data: &Outgoing, bh: Option<BufferHandle>, pool: &mut BufferPool) {
    if data.in_region().is_none() {
        data.copy_into(pool.get_mut(bh.expect("a copied write has a pooled buffer")));
    }
}

/// Every byte the write sends: its parts where they lie, or its copy.
pub(super) fn write_len(data: &Outgoing, bh: Option<BufferHandle>, pool: &mut BufferPool) -> usize {
    match data.in_region() {
        Some(parts) => parts.iter().map(|p| p.len()).sum(),
        None => pool.get_mut(bh.expect("a copied write has its copy")).len(),
    }
}

/// An SQE that writes as much of `bytes` as one SQE carries.
fn write_from(id: SubmissionId, fd: RawFd, bytes: &[u8]) -> io_uring::squeue::Entry {
    let len = bytes.len().min(MAX_SQE_BYTES);
    io_uring::opcode::Write::new(io_uring::types::Fd(fd), bytes.as_ptr(), len as u32)
        .offset(u64::MAX)
        .build()
        .user_data(id.as_u64())
}

/// The SQE that writes what is left of a write after its first `filled`
/// bytes, from the same source every time: the copy, the one part, or a
/// gathered write of the parts whose iovecs it lays out in the pooled buffer.
pub(super) fn write_sqe(
    id: SubmissionId,
    fd: RawFd,
    data: &Outgoing,
    filled: usize,
    bh: Option<BufferHandle>,
    pool: &mut BufferPool,
) -> io_uring::squeue::Entry {
    let Some(parts) = data.in_region() else {
        let copy = pool.get_mut(bh.expect("a copied write has its copy"));
        return write_from(id, fd, &copy[filled..]);
    };
    let Some(bh) = bh else {
        let part = parts.iter().copied().find(|p| !p.is_empty()).unwrap_or(&[]);
        return write_from(id, fd, &part[filled.min(part.len())..]);
    };
    let iovecs = pool.get_mut(bh);
    iovecs.clear();
    for iov in tail_iovecs(&parts, filled, MAX_SQE_BYTES) {
        // SAFETY: an iovec is plain data; the kernel reads these bytes back
        // as the array they were.
        iovecs.extend_from_slice(unsafe {
            std::slice::from_raw_parts(
                &iov as *const libc::iovec as *const u8,
                std::mem::size_of::<libc::iovec>(),
            )
        });
    }
    let count = iovecs.len() / std::mem::size_of::<libc::iovec>();
    io_uring::opcode::Writev::new(
        io_uring::types::Fd(fd),
        iovecs.as_ptr() as *const libc::iovec,
        count as u32,
    )
    .offset(u64::MAX)
    .build()
    .user_data(id.as_u64())
}
