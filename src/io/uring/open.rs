//! audited: 2026-11-17
//! An open on the ring: its `IORING_OP_OPENAT`, the timer that bounds it, and what its CQE settles.
//!
//! docs/impl/io/platforms.md
//! docs/impl/io/bounds.md
//! docs/impl/io/cancel.md

use super::ops::cancel_entry;
use super::submit::{expiry_user_data, submit_with_expiry};
use super::*;
use crate::io::aio::EXPIRY_CANCELS;

/// What a blocking open in an io-wq thread answers when a cancel interrupts
/// it. The kernel's restart code has no libc constant.
const ERESTARTSYS: i32 = 512;

/// Submit an open of `path` with `flags` and `mode`, bounded by `timeout`.
///
/// The path goes into `buf_handle` with its NUL, where the kernel reads it
/// until the CQE arrives. A write-only open goes in forced async, so an io-wq
/// thread waits there for a fifo's reader. Every other open asks with
/// `O_NONBLOCK`, which [`settle`] clears on the descriptor it hands back.
#[allow(clippy::too_many_arguments)]
pub(crate) fn submit_uring_open(
    ring: &mut io_uring::IoUring,
    id: SubmissionId,
    path: &std::ffi::CStr,
    flags: i32,
    mode: u32,
    timeout: Option<Duration>,
    buffer_pool: &mut BufferPool,
    buf_handle: BufferHandle,
) -> Result<(), String> {
    let buf = buffer_pool.get_mut(buf_handle);
    buf.clear();
    buf.extend_from_slice(path.to_bytes_with_nul());
    let write_only = flags & libc::O_ACCMODE == libc::O_WRONLY;
    let asked = if write_only {
        flags
    } else {
        flags | libc::O_NONBLOCK
    };
    let entry =
        io_uring::opcode::OpenAt::new(io_uring::types::Fd(libc::AT_FDCWD), buf.as_ptr().cast())
            .flags(asked)
            .mode(mode)
            .build()
            .user_data(id.as_u64());
    let entry = if write_only {
        entry.flags(io_uring::squeue::Flags::ASYNC)
    } else {
        entry
    };
    // SAFETY: the path lives in the pooled buffer until the CQE arrives.
    unsafe { submit_with_expiry(ring, id, entry, timeout) }
}

/// The result a live open's CQE answers with.
///
/// A live open is interrupted only by the cancel its own timer asked for, so
/// an interrupted wait answers `ECANCELED`, which the completion reads as the
/// timeout. A descriptor leaves in blocking mode: `port/open` never asks for
/// `O_NONBLOCK`, so the flag on it is the one [`submit_uring_open`] added.
pub(super) fn settle(result_code: i32) -> i32 {
    if result_code == -ERESTARTSYS || result_code == -libc::EINTR {
        return -libc::ECANCELED;
    }
    if result_code < 0 {
        return result_code;
    }
    // SAFETY: the kernel handed this descriptor to this process, and nothing
    // else holds it yet.
    unsafe {
        let got = libc::fcntl(result_code, libc::F_GETFL);
        if got >= 0
            && (got & libc::O_NONBLOCK == 0
                || libc::fcntl(result_code, libc::F_SETFL, got & !libc::O_NONBLOCK) == 0)
        {
            return result_code;
        }
        let errno = std::io::Error::last_os_error()
            .raw_os_error()
            .unwrap_or(libc::EIO);
        libc::close(result_code);
        -errno
    }
}

/// What one drain owes the ring's opens once its CQ loop ends. SQEs cannot be
/// pushed while the loop iterates the CQ ring, so the drain collects them here.
#[derive(Default)]
pub(super) struct OpenTimers {
    /// Opens whose timer expired, to cancel if they are still in flight.
    expired: Vec<SubmissionId>,
    /// Opens that answered, whose timer comes out of the ring.
    answered: Vec<SubmissionId>,
}

impl OpenTimers {
    /// Read a tagged CQE. A timer that expired with `ETIME` asks for its open
    /// to be cancelled; a removed timer answers `ECANCELED` and asks nothing.
    pub(super) fn tagged(&mut self, user_data: u64, result_code: i32) {
        if user_data & EXPIRY_CANCELS != 0 && result_code == -libc::ETIME {
            let id = user_data & !(TIMEOUT_USER_DATA_TAG | EXPIRY_CANCELS);
            self.expired.push(SubmissionId::from_raw(id));
        }
    }

    /// Note an operation whose own CQE arrived, however it is taken. A bounded
    /// open's timer then comes out, so no timer outlives what it bounds.
    pub(super) fn answered(&mut self, id: SubmissionId, op: &PendingOp) {
        if matches!(op, PendingOp::Open { .. }) && op.bound().is_bounded() {
            self.answered.push(id);
        }
    }

    /// Push a cancel for each expired open still in flight, and a removal for
    /// each answered open's timer. A removal that finds its timer gone, and a
    /// cancel that finds nothing to cancel, answer tagged CQEs the drain skips.
    pub(super) fn push(self, ring: &mut io_uring::IoUring, pending: &PendingTable) {
        let cancels = self
            .expired
            .into_iter()
            .filter(|id| pending.contains(*id))
            .map(cancel_entry);
        let removals = self.answered.into_iter().map(|id| {
            io_uring::opcode::TimeoutRemove::new(expiry_user_data(id))
                .build()
                .user_data(id.as_u64() | TIMEOUT_USER_DATA_TAG)
        });
        for entry in cancels.chain(removals) {
            push_or_flush(ring, &entry);
        }
    }
}

/// Push `entry`, submitting the queue first when it is full. A lost cancel
/// would leave an open waiting past its bound. Neither entry points at memory.
fn push_or_flush(ring: &mut io_uring::IoUring, entry: &io_uring::squeue::Entry) {
    // SAFETY: the entry addresses no memory of ours.
    if unsafe { ring.submission().push(entry) }.is_ok() {
        return;
    }
    let _ = ring.submit();
    // SAFETY: as above.
    let pushed = unsafe { ring.submission().push(entry) };
    debug_assert!(pushed.is_ok(), "the ring's queue is full after a submit");
}
