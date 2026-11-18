//! audited: 2026-11-17
//! Handing SQEs to the kernel, with a linked timeout, a timer of their own, or neither.
//!
//! docs/impl/io/bounds.md
//!
//! Every `submit_uring_*` operation ends the same way, and the ending is a
//! protocol rather than a formality: an operation with a deadline goes in
//! `IO_LINK`ed to a `LinkTimeout` SQE that follows it, and the timer's own CQE
//! is tagged so the completion drain can tell timer from operation. Getting any
//! part of that wrong produces a completion nobody claims. An open is the
//! exception, and takes a timer linked to nothing.

use super::TIMEOUT_USER_DATA_TAG;
use crate::io::aio::EXPIRY_CANCELS;
use crate::io::SubmissionId;
use std::time::Duration;

/// A linked timeout, and the `Timespec` its SQE points at.
///
/// The kernel reads the timespec during the `io_uring_enter` that submits the
/// SQE, not when the SQE is pushed. So whoever pushes [`LinkTimer::entry`]
/// holds the `LinkTimer` until that submit returns. The timespec is boxed, so
/// the timer may move — into a list of re-armed timers, say — without moving
/// what the SQE points at.
#[must_use = "the timespec must outlive the ring.submit() that reads the SQE"]
pub(crate) struct LinkTimer {
    ts: Box<io_uring::types::Timespec>,
}

impl LinkTimer {
    /// A timer that fires `dur` after the kernel arms it.
    pub(crate) fn new(dur: Duration) -> LinkTimer {
        LinkTimer {
            ts: Box::new(
                io_uring::types::Timespec::new()
                    .sec(dur.as_secs())
                    .nsec(dur.subsec_nanos()),
            ),
        }
    }

    /// The `LinkTimeout` SQE for the operation `id`, which goes on the queue
    /// right behind that operation's `IO_LINK`ed SQE. Its CQE carries the
    /// timeout tag, so `drain_cqes` skips it. The SQE points into `self`.
    pub(crate) fn entry(&self, id: SubmissionId) -> io_uring::squeue::Entry {
        io_uring::opcode::LinkTimeout::new(&*self.ts)
            .build()
            .user_data(id.as_u64() | TIMEOUT_USER_DATA_TAG)
    }

    /// A `Timeout` SQE for the operation `id`, linked to nothing. Its CQE
    /// carries the timeout tag and `EXPIRY_CANCELS`, and `drain_cqes` answers
    /// an expiry with a cancel of `id`. The SQE points into `self`.
    pub(crate) fn expiry(&self, id: SubmissionId) -> io_uring::squeue::Entry {
        io_uring::opcode::Timeout::new(&*self.ts)
            .build()
            .user_data(expiry_user_data(id))
    }
}

/// The `user_data` of the timer [`LinkTimer::expiry`] arms for `id`, which an
/// `IORING_OP_TIMEOUT_REMOVE` names to take that timer out.
pub(crate) fn expiry_user_data(id: SubmissionId) -> u64 {
    id.as_u64() | TIMEOUT_USER_DATA_TAG | EXPIRY_CANCELS
}

/// Push `entry` and, with a timeout, a timer of its own, then submit the ring.
///
/// The timer is linked to nothing, so it ends `entry` only through the cancel
/// the drain issues when it expires. A linked timeout cannot end an operation
/// that waits in an io-wq thread (docs/impl/io/bounds.md).
///
/// # Safety
///
/// As for [`submit_linked`].
pub(crate) unsafe fn submit_with_expiry(
    ring: &mut io_uring::IoUring,
    id: SubmissionId,
    entry: io_uring::squeue::Entry,
    timeout: Option<Duration>,
) -> Result<(), String> {
    // Held to the end of the function: `ring.submit()` below is what reads it.
    let timer = timeout.map(LinkTimer::new);
    push(ring, &entry)?;
    if let Some(timer) = &timer {
        push(ring, &timer.expiry(id))?;
    }
    ring.submit()
        .map_err(|e| format!("io/submit: io_uring submit failed: {e}"))?;
    Ok(())
}

/// Push `entry`, arm its timeout if it has one, and submit the ring.
///
/// With a timeout, `entry` goes in flagged `IO_LINK` and a `LinkTimeout` SQE
/// follows it. When the timer fires first the kernel cancels the linked
/// operation, whose CQE then carries `-ECANCELED` — that is what the
/// completion handlers read as a timeout. The timer's own CQE carries
/// `id | TIMEOUT_USER_DATA_TAG`, which `drain_cqes` recognizes and drops, so
/// one operation still yields one completion.
///
/// Without a timeout, `entry` goes in unflagged and alone.
///
/// # Safety
///
/// `entry` may point at caller-owned memory — a path buffer, a `sockaddr`, a
/// read or write buffer. The kernel reads that memory during the
/// `io_uring_enter(2)` this function performs, so it must stay valid and
/// unmoved until this function returns. The caller keeps such buffers in the
/// buffer pool until the CQE arrives.
pub(crate) unsafe fn submit_linked(
    ring: &mut io_uring::IoUring,
    id: SubmissionId,
    entry: io_uring::squeue::Entry,
    timeout: Option<Duration>,
) -> Result<(), String> {
    // Held to the end of the function: `ring.submit()` below is what reads it.
    let timer = timeout.map(LinkTimer::new);
    let entry = if timer.is_some() {
        entry.flags(io_uring::squeue::Flags::IO_LINK)
    } else {
        entry
    };
    push(ring, &entry)?;
    if let Some(timer) = &timer {
        push(ring, &timer.entry(id))?;
    }

    ring.submit()
        .map_err(|e| format!("io/submit: io_uring submit failed: {e}"))?;
    Ok(())
}

/// Push one SQE onto the submission queue.
///
/// # Safety
///
/// Same obligation as [`submit_linked`]: any memory `entry` points at must
/// outlive the submission.
unsafe fn push(
    ring: &mut io_uring::IoUring,
    entry: &io_uring::squeue::Entry,
) -> Result<(), String> {
    ring.submission()
        .push(entry)
        .map_err(|_| "io/submit: io_uring submission queue full".to_string())
}
