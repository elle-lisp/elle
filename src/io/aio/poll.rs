//! audited: 2026-09-23
//! `AsyncBackend`'s cancel, poll and wait — what the scheduler drives the
//! backend with, on either platform.
//!
//! src/io/AGENTS.md

use super::*;

impl AsyncBackend {
    /// Cancel a pending I/O operation by submission ID.
    ///
    /// Two halves, and both platforms have both. **Asking the operation to
    /// stop** is platform-specific: io_uring takes `IORING_OP_ASYNC_CANCEL`
    /// (its own CQE is high-bit tagged and skipped; the operation's own CQE
    /// arrives with `-ECANCELED`), while a pool worker is asked through its stop
    /// pipe (docs/impl/io-inflight.md § "The stop pipe"). **Marking the id** is shared:
    /// the operation's completion, whenever it arrives, retires the entry
    /// instead of building a result nobody would read.
    ///
    /// The `pending` entry stays either way. The worker's `RawCompletion` still
    /// decrements the hub's `in_flight` at the drain site and still releases the
    /// descriptor the operation named; removing the entry here would strand
    /// both. We must NOT decrement `in_flight` here — the drain site does it,
    /// and doing it twice would underflow the combined count.
    pub(crate) fn cancel(&self, id: SubmissionId) -> Result<(), String> {
        let mut inner = self.inner.borrow_mut();
        match inner.platform {
            #[cfg(target_os = "linux")]
            PlatformBackend::Uring(ref mut ring) => {
                crate::io::uring::submit_uring_cancel(ring, id)?;
            }
            PlatformBackend::ThreadPool => inner.hub.stop(id),
        }
        // A read that borrowed its port's remainder gives it back now rather
        // than when its completion arrives: the next read on the port is often
        // submitted first, and it must find those bytes where the stream has
        // them (docs/impl/io-bytes.md). Before the mark, which lets go of the
        // port the give-back reads.
        if let Some((key, port, lent)) = inner.pending.take_lent(id) {
            crate::io::landing::give_back(&mut inner.fd_states, &key, &port, lent);
        }
        inner.pending.mark_cancelled(id);
        Ok(())
    }

    /// Non-blocking poll for completions.
    pub(crate) fn poll(&self) -> Vec<Completion> {
        let mut inner = self.inner.borrow_mut();
        inner.drain_ready();
        inner.completions.drain(..).collect()
    }

    /// Blocking wait for completions.
    /// `timeout_ms`: negative = wait forever, 0 = poll, positive = wait up to N ms.
    pub(crate) fn wait(&self, timeout_ms: i64) -> Result<Vec<Completion>, String> {
        let mut inner = self.inner.borrow_mut();
        // The requesting instance's heap (constant per backend); every completion
        // value the harvest builds is born on it. Captured as a `Copy` pointer so
        // it survives the field destructure below.
        let origin_heap = inner.origin_heap;
        let gen = inner.unicode_generation;

        // First drain anything already ready (ring CQEs + the hub).
        inner.drain_ready();
        if !inner.completions.is_empty() {
            return Ok(inner.completions.drain(..).collect());
        }

        // Nothing buffered — block on the platform's waitable.
        let timeout = if timeout_ms < 0 {
            None
        } else {
            Some(timeout_ms as u64)
        };

        // Destructure for independent borrows of each field.
        {
            let AsyncBackendInner {
                ref mut platform,
                ref mut hub,
                ref mut pending,
                ref mut buffer_pool,
                ref mut fd_states,
                ref mut completions,
                ..
            } = *inner;

            match platform {
                #[cfg(target_os = "linux")]
                PlatformBackend::Uring(ring) => {
                    // Ring ops post their own CQEs; hub work (getaddrinfo,
                    // `Task`, stdin) posts none and wakes this wait through the
                    // standing eventfd POLL_ADD, which `wait_uring` clears and
                    // re-arms. `drain_ready` below takes the hub channel.
                    //
                    // The timeout passed here is the caller's, never a rescue
                    // cap — src/io/AGENTS.md invariant 8.
                    crate::io::uring::wait_uring(
                        ring,
                        timeout,
                        pending,
                        buffer_pool,
                        fd_states,
                        completions,
                        origin_heap,
                        gen,
                        hub.eventfd(),
                    )?;
                }
                PlatformBackend::ThreadPool => {
                    // One channel, all sources. A crossbeam `recv()`
                    // registers before it sleeps, so a worker's publish cannot
                    // be missed while the scheduler is asleep. Same rule as the
                    // ring arm above: the timeout is the caller's, never a
                    // rescue cap — src/io/AGENTS.md invariant 8.
                    if hub.in_flight() > 0 {
                        let waited = match timeout {
                            None => hub.recv_blocking(None),
                            Some(0) => None, // poll mode — already drained above
                            Some(ms) => hub.recv_blocking(Some(Duration::from_millis(ms))),
                        };
                        if let Some(rc) = waited {
                            let id = SubmissionId::from_raw(match &rc {
                                crate::io::threadpool::RawCompletion::Pool(pc) => pc.id,
                                crate::io::threadpool::RawCompletion::Stdin(sc) => sc.id,
                            });
                            let cooked =
                                cook_raw(rc, pending, fd_states, buffer_pool, origin_heap, gen);
                            hub.forget_stop(id);
                            if let Some(c) = cooked {
                                completions.push_back(c);
                            }
                        }
                    }
                }
            }
        }

        // Catch any hub stragglers (and re-drain the ring) that landed while we
        // blocked.
        inner.drain_ready();
        Ok(inner.completions.drain(..).collect())
    }

    /// Check if there are pending operations.
    /// Used by the async scheduler to determine when to exit the event loop.
    #[allow(dead_code)]
    pub(crate) fn has_pending(&self) -> bool {
        let inner = self.inner.borrow();
        !inner.pending.is_empty()
    }

    /// Background worker operations submitted but not yet reaped — the OS
    /// threads this backend currently has out. An operation that is cancelled,
    /// or whose port is closed under it, is meant to give its worker back;
    /// this is what says whether it did. Zero on io_uring, which runs its
    /// operations in the kernel rather than on threads.
    pub(crate) fn workers(&self) -> usize {
        self.inner.borrow().hub.in_flight()
    }
}
