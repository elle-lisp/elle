//! Handing SQEs to the kernel, with or without a linked timeout.
//!
//! Every `submit_uring_*` operation ends the same way, and the ending is a
//! protocol rather than a formality: an operation with a deadline goes in
//! `IO_LINK`ed to a `LinkTimeout` SQE that follows it, and the timer's own CQE
//! is tagged so the completion drain can tell timer from operation. Getting any
//! part of that wrong produces a completion nobody claims.

use super::TIMEOUT_USER_DATA_TAG;
use crate::io::SubmissionId;
use std::time::Duration;

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
    let entry = if timeout.is_some() {
        entry.flags(io_uring::squeue::Flags::IO_LINK)
    } else {
        entry
    };
    push(ring, &entry)?;

    if let Some(dur) = timeout {
        let ts = io_uring::types::Timespec::new()
            .sec(dur.as_secs())
            .nsec(dur.subsec_nanos());
        let timer = io_uring::opcode::LinkTimeout::new(&ts)
            .build()
            .user_data(id.as_u64() | TIMEOUT_USER_DATA_TAG);
        push(ring, &timer)?;
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
