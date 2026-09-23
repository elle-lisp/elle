//! audited: 2026-09-23
//! The single CQE drain: each completion resolved through its entry, then
//! retired, resubmitted, or cooked into a `Completion`.
//!
//! src/io/AGENTS.md
//! docs/impl/io-bytes.md

use super::resubmit::{next, Next};
use super::*;

/// Push a resubmitted operation, re-arming the caller's timeout as a linked
/// timeout SQE so the bound applies to this operation as it did to the first.
///
/// Returns the `Timespec` the kernel reads when it processes the SQE. It must
/// stay alive until `ring.submit()` hands the queue over, so the caller holds
/// it — a resubmit loop pushes many SQEs before one submit.
#[must_use = "the timespec must outlive the ring.submit() that consumes the SQE"]
fn push_resubmit(
    ring: &mut io_uring::IoUring,
    id: SubmissionId,
    entry: io_uring::squeue::Entry,
    timeout: Option<Duration>,
) -> Option<Box<io_uring::types::Timespec>> {
    let entry = if timeout.is_some() {
        entry.flags(io_uring::squeue::Flags::IO_LINK)
    } else {
        entry
    };
    unsafe {
        let _ = ring.submission().push(&entry);
    }
    let dur = timeout?;
    let ts = Box::new(
        io_uring::types::Timespec::new()
            .sec(dur.as_secs())
            .nsec(dur.subsec_nanos()),
    );
    let timeout_sqe = io_uring::opcode::LinkTimeout::new(&*ts)
        .build()
        .user_data(id.as_u64() | TIMEOUT_USER_DATA_TAG);
    unsafe {
        let _ = ring.submission().push(&timeout_sqe);
    }
    Some(ts)
}

/// Drain all available CQEs from the completion ring.
///
/// This is the **single** CQE processing path — used by both poll
/// (non-blocking) and wait (after blocking). Each CQE is one of:
/// - the bridge eventfd's standing poll, reported through `eventfd_fired`;
/// - a linked timeout's own CQE (high-bit tag), skipped;
/// - an operation whose entry has no reader, retired rather than cooked;
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
    // SQEs cannot be pushed while the CQ ring is being iterated, so the
    // operations that need another one wait here with the SQE they need.
    let mut again: Vec<(SubmissionId, io_uring::squeue::Entry, TakenOp)> = Vec::new();

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

        // Timeout CQEs have the high bit set — skip them.
        if user_data & TIMEOUT_USER_DATA_TAG != 0 {
            continue;
        }

        let id = SubmissionId::from_raw(user_data);
        // A submission with no reader is retired here rather than cooked.
        // Everything below reads what the operation held — the port, the
        // process handle, the fiber's pre-allocated buffer — and the fiber that
        // owned all three is gone in both of the arms that skip it.
        let mut op = match pending.take(id) {
            // Cancelled: whoever cancelled dropped its record of the submission
            // first, so there is nobody left to tell. The cancel already gave
            // back any remainder the read borrowed.
            Taken::Cancelled(op) => {
                op.retire(result_code, buffer_pool);
                continue;
            }
            // The fiber that asked has ended: nobody dropped this id, so the
            // scheduler still pairs it with that fiber and clears the pairing on
            // a completion. The port may have another reader, so a borrowed
            // remainder goes back to it; then the entry is retired, answered
            // with an error that reads none of it.
            Taken::Orphaned(mut op) => {
                crate::io::landing::give_back_lent(&mut op, fd_states);
                op.retire(result_code, buffer_pool);
                completions.push_back(PendingTable::orphaned_asker_error(id, origin_heap));
                continue;
            }
            Taken::Unknown => continue,
            Taken::Live(op) => op,
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

    // A re-armed `LinkTimeout` hands the kernel a pointer to its `Timespec`,
    // which must stay put until the `ring.submit()` below consumes the SQE.
    // The boxes live here so every resubmission's timespec outlives that call.
    let mut link_timeouts: Vec<Box<io_uring::types::Timespec>> = Vec::new();
    for (id, sqe, op) in again {
        // Bound this operation the way the original submission was bounded. An
        // operation that needs several SQEs — a read to its newline, its count
        // or its EOF, a write to the end of its payload — would otherwise be
        // unbounded from its second SQE on, and a peer that goes quiet would
        // hang an operation that asked for a timeout.
        let timeout = op.timeout();
        pending.restore(id, op);
        link_timeouts.extend(push_resubmit(ring, id, sqe, timeout));
    }

    if !ring.submission().is_empty() {
        let _ = ring.submit();
    }
}
