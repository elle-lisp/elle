//! audited: 2026-09-23
//! The ring's blocking wait: one `io_uring_enter` for ring CQEs and the hub's
//! bridge alike, then the drain.
//!
//! src/io/AGENTS.md

use super::*;

#[allow(clippy::too_many_arguments)]
pub(crate) fn wait_uring(
    ring: &mut io_uring::IoUring,
    timeout: Option<u64>,
    pending: &mut PendingTable,
    buffer_pool: &mut BufferPool,
    fd_states: &mut HashMap<PortKey, FdState>,
    completions: &mut VecDeque<Completion>,
    // The requesting instance's heap; completion values are born on it
    // (`crate::io::completion_heap_ptr`).
    origin_heap: *mut crate::value::fiberheap::FiberHeap,
    // The owning VM's Unicode generation, forwarded to drain_cqes.
    gen: crate::segment::Generation,
    // The bridge eventfd (`Some` on the uring platform). When its standing
    // `POLL_ADD` is what woke this wait, the counter is cleared and the poll
    // re-armed before returning so the next wait stays wakeable. The hub
    // channel that carries the actual completion is drained by the caller.
    eventfd: Option<RawFd>,
) -> Result<(), String> {
    let mut eventfd_fired = false;
    // Block until at least one CQE is available (or timeout).
    match timeout {
        Some(0) => {} // poll only — no wait
        Some(ms) => {
            let ts = io_uring::types::Timespec::new()
                .sec(ms / 1000)
                .nsec(((ms % 1000) * 1_000_000) as u32);
            let args = io_uring::types::SubmitArgs::new().timespec(&ts);
            loop {
                match ring.submitter().submit_with_args(1, &args) {
                    Ok(_) => break,
                    Err(e) if e.raw_os_error() == Some(libc::EINTR) => {
                        // Interrupted by a signal (e.g. SIGCHLD from a subprocess
                        // in a concurrent test). Retry — the timeout is still active.
                        continue;
                    }
                    Err(e) if e.raw_os_error() == Some(libc::ETIME) => {
                        // Timeout expired with no completions — that's valid.
                        break;
                    }
                    Err(e) => {
                        return Err(format!("io/wait: io_uring wait failed: {}", e));
                    }
                }
            }
        }
        None => loop {
            match ring.submit_and_wait(1) {
                Ok(_) => break,
                Err(e) if e.raw_os_error() == Some(libc::EINTR) => continue,
                Err(e) => {
                    return Err(format!("io/wait: io_uring wait failed: {}", e));
                }
            }
        },
    }

    drain_cqes(
        ring,
        pending,
        buffer_pool,
        fd_states,
        completions,
        origin_heap,
        gen,
        &mut eventfd_fired,
    );

    // If drain_cqes resubmitted ops (ReadAll/ReadLine) and produced no
    // completions, loop to wait for the resubmitted read's CQE. Only do
    // this for blocking waits (no timeout) — callers with timeouts
    // should return and retry via the outer event loop.
    //
    // Stop if the eventfd bridge fired: the wake came from a hub worker whose
    // completion sits in the channel (drained by the caller), not from a ring
    // op. A pending *pool* op posts no ring CQE, so looping on `submit_and_wait`
    // here — with the one-shot `POLL_ADD` already consumed — would block forever.
    if timeout.is_none() {
        while completions.is_empty() && !pending.is_empty() && !eventfd_fired {
            loop {
                match ring.submit_and_wait(1) {
                    Ok(_) => break,
                    Err(e) if e.raw_os_error() == Some(libc::EINTR) => continue,
                    Err(e) => {
                        return Err(format!("io/wait: io_uring wait failed: {}", e));
                    }
                }
            }
            drain_cqes(
                ring,
                pending,
                buffer_pool,
                fd_states,
                completions,
                origin_heap,
                gen,
                &mut eventfd_fired,
            );
        }
    }

    // The bridge eventfd's `POLL_ADD` is one-shot. If it fired, clear the
    // counter (so the re-armed poll blocks instead of completing on a stale
    // count) and re-arm so the next wait is wakeable. A re-arm failure must
    // propagate — a deaf bridge would hang a later wait, and surfacing it here
    // fails a test instead.
    if eventfd_fired {
        if let Some(efd) = eventfd {
            crate::io::eventfd::drain(efd);
            arm_eventfd_poll(ring, efd)?;
        }
    }
    Ok(())
}
