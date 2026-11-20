//! audited: 2026-11-20
//! Draining what is ready — the ring's CQEs and the shared hub — and asking the operations of a gone fiber to stop.
//!
//! docs/impl/io/inflight.md
//! docs/impl/io/platforms.md

use super::*;

impl AsyncBackendInner {
    /// Drain everything ready now into self.completions: the ring's CQEs (uring
    /// platform) and the shared hub (pool + stdin workers), on both platforms.
    ///
    /// The stale sweep runs after both drains, so an operation that already
    /// answered is read off here rather than asked to stop. Anything left whose
    /// reader is gone then has its completion on the way before the caller
    /// blocks again. The lines of writes move last, once every write that
    /// finished has left the table (docs/impl/io/write.md).
    pub(super) fn drain_ready(&mut self) {
        self.drain_uring_completions();
        self.drain_hub();
        self.stop_orphaned();
        self.advance_lines();
    }

    /// Ask every in-flight operation whose asking fiber has ended to stop.
    ///
    /// Such an operation waits for an event outside this process, such as a peer
    /// that writes or a client that connects. The fiber that would have received
    /// the result is what went away, so nothing here is left to make that event
    /// happen. The ask is not a cancel (docs/impl/io/inflight.md): the id stays
    /// unmarked, so the completion still answers and the scheduler retires the
    /// pairing it holds.
    ///
    /// The hub is asked first, because a pool worker runs a `File` walk on the
    /// ring's platform too (docs/impl/io/cancel.md).
    fn stop_orphaned(&mut self) {
        for id in self.pending.orphaned_to_stop() {
            if self.hub.stop(id) {
                continue;
            }
            #[cfg(target_os = "linux")]
            if let PlatformBackend::Uring(ref mut ring) = self.platform {
                let _ = crate::io::uring::submit_uring_cancel(ring, id);
            }
        }
    }

    /// Drain the io_uring completion queue into self.completions. A no-op on the
    /// pool platform (no ring); all its work surfaces through `drain_hub`.
    ///
    /// This is the non-blocking drain (poll path, and the pre/post passes of a
    /// blocking wait). When it consumes the standing eventfd bridge `POLL_ADD`
    /// CQE, it clears the eventfd and re-arms the one-shot poll here. Otherwise
    /// the next blocking wait would have no armed poll watching the bridge, and
    /// a hub worker's wake would be lost.
    fn drain_uring_completions(&mut self) {
        // Only the ring arm below consumes these; the pool platform compiles
        // that arm out and would see three unused bindings.
        #[cfg(target_os = "linux")]
        let origin_heap = self.origin_heap;
        #[cfg(target_os = "linux")]
        let gen = self.unicode_generation;
        #[cfg(target_os = "linux")]
        let eventfd = self.hub.eventfd();
        match &mut self.platform {
            #[cfg(target_os = "linux")]
            PlatformBackend::Uring(ring) => {
                let mut eventfd_fired = false;
                crate::io::uring::drain_cqes(
                    ring,
                    &mut self.pending,
                    &mut self.buffer_pool,
                    &mut self.fd_states,
                    &mut self.completions,
                    origin_heap,
                    gen,
                    &mut eventfd_fired,
                );
                if eventfd_fired {
                    if let Some(efd) = eventfd {
                        crate::io::eventfd::drain(efd);
                        // Re-arm of a single SQE into a 256-deep ring is
                        // infallible in practice; assert so a regression is loud
                        // in tests rather than a silent deaf bridge in release.
                        let rearmed = crate::io::uring::arm_eventfd_poll(ring, efd);
                        debug_assert!(
                            rearmed.is_ok(),
                            "eventfd bridge re-arm failed in poll path: {:?}",
                            rearmed
                        );
                    }
                }
            }
            PlatformBackend::ThreadPool => {}
        }
    }

    /// Drain the shared completion hub (thread-pool workers + stdin worker) into
    /// self.completions. This is the single hub drain site: `drain_raw`
    /// decrements `in_flight` once per item, and `cook_raw` turns each
    /// `RawCompletion` into a `Completion`. It returns `None` for an op nobody
    /// reads: a cancelled one, which it retires, or one with no entry.
    fn drain_hub(&mut self) {
        let origin_heap = self.origin_heap;
        let gen = self.unicode_generation;
        let AsyncBackendInner {
            ref mut hub,
            ref mut pending,
            ref mut fd_states,
            ref mut buffer_pool,
            ref mut completions,
            ..
        } = *self;
        for rc in hub.drain_raw() {
            let id = SubmissionId::from_raw(match &rc {
                crate::io::threadpool::RawCompletion::Pool(pc) => pc.id,
                crate::io::threadpool::RawCompletion::Stdin(sc) => sc.id,
            });
            let cooked = cook_raw(rc, pending, fd_states, buffer_pool, origin_heap, gen);
            hub.forget_stop(id);
            if let Some(c) = cooked {
                completions.push_back(c);
            }
        }
    }
}
