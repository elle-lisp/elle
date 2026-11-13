//! audited: 2026-11-11
//! The ring's blocking wait: one `io_uring_enter` for ring CQEs and the hub's
//! bridge alike, then the drain.
//!
//! docs/impl/io/platforms.md

use super::*;

#[allow(clippy::too_many_arguments)]
pub(crate) fn wait_uring(
    ring: &mut Ring,
    timeout: Option<Duration>,
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
    // A zero timeout polls: the drain below takes what is there, and nothing
    // sleeps.
    if timeout != Some(Duration::ZERO) {
        ring.wait(timeout)?;
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
    // op. A pending *pool* op posts no ring CQE, so looping on the wait here —
    // with the one-shot `POLL_ADD` already consumed — would block forever.
    if timeout.is_none() {
        while completions.is_empty() && !pending.is_empty() && !eventfd_fired {
            ring.wait(None)?;
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
