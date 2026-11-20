//! audited: 2026-11-20
//! The single CQE drain: each completion resolved through its entry, then
//! retired, resubmitted, or cooked into a `Completion`.
//!
//! docs/impl/io/platforms.md
//! docs/impl/io/bytes.md
//! docs/impl/io/bounds.md

use super::open::{settle, OpenTimers};
use super::resubmit::{next, Next};
use super::*;

/// Push a resubmitted operation, re-arming what the caller's bound has left as
/// a linked timeout SQE, so the bound applies to this operation as it did to
/// the first.
///
/// Returns the timer, whose timespec the kernel reads when `ring.submit()`
/// hands the queue over, so the caller holds it until then — a resubmit loop
/// pushes many SQEs before one submit.
fn push_resubmit(
    ring: &mut io_uring::IoUring,
    id: SubmissionId,
    entry: io_uring::squeue::Entry,
    timeout: Option<Duration>,
) -> Option<LinkTimer> {
    let entry = if timeout.is_some() {
        entry.flags(io_uring::squeue::Flags::IO_LINK)
    } else {
        entry
    };
    unsafe {
        let _ = ring.submission().push(&entry);
    }
    let timer = LinkTimer::new(timeout?);
    unsafe {
        let _ = ring.submission().push(&timer.entry(id));
    }
    Some(timer)
}

/// Drain all available CQEs from the completion ring.
///
/// This is the **single** CQE processing path — used by both poll
/// (non-blocking) and wait (after blocking). Each CQE is one of:
/// - the bridge eventfd's standing poll, reported through `eventfd_fired`;
/// - a timer's own CQE (high-bit tag), skipped unless an open's timer expired,
///   which cancels the open;
/// - an operation whose result nobody reads, retired rather than cooked, and
///   answered with an error that reads nothing it held;
/// - an operation at rest after a teardown, retired and answered with nothing;
/// - an operation that needs another SQE — a read short of its answer, a
///   `read-all` short of EOF, a write short of its payload — resubmitted;
/// - an operation that is done, cooked into a `Completion`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn drain_cqes(
    ring: &mut io_uring::IoUring,
    pending: &mut PendingTable,
    buffer_pool: &mut BufferPool,
    fd_states: &mut HashMap<PortKey, FdState>,
    completions: &mut VecDeque<Completion>,
    // The requesting instance's heap; completion values are born on it
    // (`crate::io::completion_heap_ptr`).
    origin_heap: *mut crate::value::fiberheap::FiberHeap,
    // The owning VM's Unicode generation; a text `read-exact` decides "enough
    // clusters yet?" with it.
    gen: crate::segment::Generation,
    // Set to true if the standing eventfd bridge `POLL_ADD` fired (a hub worker
    // raised the eventfd). The caller clears the eventfd and re-arms the poll —
    // the sentinel CQE has no `pending` entry and no buffer to process.
    eventfd_fired: &mut bool,
) {
    // SQEs cannot be pushed while the CQ ring is being iterated. So the
    // operations that need another one wait here with the SQE they need, and
    // the opens' timers with what they ask of the ring.
    let mut again: Vec<(SubmissionId, io_uring::squeue::Entry, TakenOp)> = Vec::new();
    let mut open_timers = OpenTimers::default();

    for cqe in ring.completion() {
        let user_data = cqe.user_data();
        let result_code = cqe.result();

        // Bridge eventfd POLL_ADD CQE: a hub worker raised the eventfd to wake
        // this wait. Record the edge and skip — it carries no pending op. Matched
        // before the timeout tag (its high bit is clear) and the pending lookup.
        if user_data == EVENTFD_USER_DATA {
            *eventfd_fired = true;
            continue;
        }

        // Timeout CQEs have the high bit set. Only an open's expired timer asks
        // for anything; the rest are skipped.
        if user_data & TIMEOUT_USER_DATA_TAG != 0 {
            open_timers.tagged(user_data, result_code);
            continue;
        }

        let id = SubmissionId::from_raw(user_data);
        let taken = pending.take(id);
        if let Some(op) = taken.op() {
            open_timers.answered(id, op);
        }
        // A submission with no reader is retired here rather than cooked.
        // Everything below reads what the operation held — the port, the
        // process handle, the fiber's pre-allocated buffer — and the fiber that
        // owned all three may be gone. Its answer reads none of them.
        let mut op =
            match taken.live_or_retire(id, result_code, &[], buffer_pool, fd_states, origin_heap) {
                Ok(op) => op,
                Err(answer) => {
                    completions.extend(answer);
                    continue;
                }
            };

        // Connect: on failure, close the pre-created socket.
        if let PendingOp::Connect {
            ref mut connect_fd, ..
        } = &mut *op
        {
            if let Some(fd) = *connect_fd {
                if result_code < 0 {
                    unsafe { libc::close(fd) };
                    *connect_fd = None;
                }
            }
        }
        let result_code = match &*op {
            PendingOp::Open { .. } => settle(result_code),
            _ => result_code,
        };

        match next(id, &mut op, result_code, buffer_pool, fd_states, gen) {
            Next::Again(sqe) => again.push((id, sqe, op)),
            Next::Complete {
                result_code,
                data,
                buf_handle,
            } => completions.push_back(process_raw_completion(
                id,
                result_code,
                data,
                &op,
                fd_states,
                buffer_pool,
                buf_handle,
                origin_heap,
                gen,
            )),
        }
    }

    // Every re-armed timer lives here until the `ring.submit()` below reads it.
    let mut link_timeouts: Vec<LinkTimer> = Vec::new();
    for (id, sqe, op) in again {
        // Bound this operation the way the original submission was bounded.
        // Some operations need several SQEs: a read to its newline, its count
        // or its EOF, a write to the end of its payload. Unbounded from its
        // second SQE on, such an operation would hang on a peer that goes
        // quiet, though it asked for a timeout.
        let timeout = op.bound().next_wait();
        pending.restore(id, op);
        link_timeouts.extend(push_resubmit(ring, id, sqe, timeout));
    }
    open_timers.push(ring, pending);

    if !ring.submission().is_empty() {
        let _ = ring.submit();
    }
}
