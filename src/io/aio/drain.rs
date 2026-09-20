//! audited: 2026-09-20
//! Draining what is ready: the ring's CQEs, the shared hub, and the teardown
//! pass that brings the ring to rest before its buffers are freed.
//!
//! docs/io.md

use super::*;

impl AsyncBackendInner {
    /// Cancel and drain every in-flight io_uring operation so no kernel-owned
    /// buffer outlives the backend — docs/io.md § "Backend teardown" carries
    /// what that is for.
    ///
    /// Two things shape the loop. Operations the stdin and network channels
    /// service post no CQE here, so they never shrink `pending` and the loop
    /// stops on the first pass that does not; the pass count bounds it anyway.
    /// And every entry is marked cancelled before the drain, so each CQE retires
    /// its entry rather than building a result from it — which is also why the
    /// shrink is the whole progress test, no completion being produced here for
    /// a shrink to be read off.
    #[cfg(target_os = "linux")]
    pub(super) fn quiesce_pending(&mut self) {
        if !matches!(self.platform, PlatformBackend::Uring(_)) || self.pending.is_empty() {
            return;
        }
        // Where `drain_cqes` puts what it cooked. Every entry here is marked
        // cancelled, so it stays empty; it exists because the drain is one
        // function with one signature. Emptied through `discard` all the same,
        // because a completion owns what it built
        // (docs/impl/io-inflight.md) and dropping one here would strand that.
        let mut sink: VecDeque<Completion> = VecDeque::new();
        let mut passes = 0u32;
        while !self.pending.is_empty() && passes < 64 {
            passes += 1;
            let before = self.pending.len();
            let ids: Vec<SubmissionId> = self.pending.ids();

            // Nobody is left to read any of these, and the heap their values
            // live on may already be gone — this is the teardown a stranded
            // backend gets (`IoBackend::quiesce`). Mark them so the drain
            // retires each entry rather than building a result from it.
            self.pending.cancel_all();

            // Cancel everything still pending. A cancel for an id the ring
            // doesn't know (a stdin/network op) returns a tagged `-ENOENT` CQE
            // that `drain_cqes` skips, so it is harmless.
            if let PlatformBackend::Uring(ref mut ring) = self.platform {
                for id in &ids {
                    let _ = crate::io::uring::submit_uring_cancel(ring, *id);
                }
            }

            // Wait briefly for the cancellation CQEs, then drain them. A cancel
            // posts its CQE promptly, so 50ms is a ceiling, not the expected
            // latency.
            let origin_heap = self.origin_heap;
            let gen = self.unicode_generation;
            let AsyncBackendInner {
                ref mut platform,
                ref mut pending,
                ref mut buffer_pool,
                ref mut fd_states,
                ..
            } = *self;
            if let PlatformBackend::Uring(ring) = platform {
                let ts = io_uring::types::Timespec::new().sec(0).nsec(50_000_000);
                let args = io_uring::types::SubmitArgs::new().timespec(&ts);
                match ring.submitter().submit_with_args(1, &args) {
                    Ok(_) => {}
                    Err(e) if e.raw_os_error() == Some(libc::ETIME) => {}
                    Err(e) if e.raw_os_error() == Some(libc::EINTR) => {}
                    Err(_) => break,
                }
                // Teardown: the ring is about to close, so the standing eventfd
                // POLL_ADD is left to be cancelled with everything else. We do
                // not service it (no re-arm) — a fired sentinel is just skipped.
                let mut bridge_fired = false;
                crate::io::uring::drain_cqes(
                    ring,
                    pending,
                    buffer_pool,
                    fd_states,
                    &mut sink,
                    origin_heap,
                    gen,
                    &mut bridge_fired,
                );
            }

            Completion::discard_all(sink.drain(..));
            // No shrink ⇒ only channel-serviced ops remain; stop.
            if self.pending.len() >= before {
                break;
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    pub(super) fn quiesce_pending(&mut self) {}

    /// Drain everything ready now into self.completions: the ring's CQEs (uring
    /// platform) and the shared hub (pool + stdin workers), on both platforms.
    ///
    /// The stale sweep runs last, so an operation that already answered is read
    /// off here rather than asked to stop, and anything left whose reader is
    /// gone has its completion on the way before the caller blocks again.
    pub(super) fn drain_ready(&mut self) {
        self.drain_uring_completions();
        self.drain_hub();
        self.stop_orphaned();
    }

    /// Ask every in-flight operation whose asking fiber has ended to stop.
    ///
    /// Such an operation waits for an event outside this process — a peer that
    /// writes, a client that connects — and the fiber that would have received
    /// the result is what went away, so nothing here is left to make that event
    /// happen. See docs/impl/io-inflight.md § "Ending an operation whose fiber is gone"
    /// for why the ask is not a cancel: the id stays unmarked, so the
    /// completion still answers and the scheduler retires the pairing it holds.
    fn stop_orphaned(&mut self) {
        for id in self.pending.orphaned_to_stop() {
            match self.platform {
                #[cfg(target_os = "linux")]
                PlatformBackend::Uring(ref mut ring) => {
                    let _ = crate::io::uring::submit_uring_cancel(ring, id);
                }
                PlatformBackend::ThreadPool => self.hub.stop(id),
            }
        }
    }

    /// Drain the io_uring completion queue into self.completions. A no-op on the
    /// pool platform (no ring); all its work surfaces through `drain_hub`.
    ///
    /// This is the non-blocking drain (poll path, and the pre/post passes of a
    /// blocking wait). If it consumes the standing eventfd bridge `POLL_ADD`
    /// CQE, it clears the eventfd and re-arms the one-shot poll here — otherwise
    /// the next blocking wait would have no armed poll watching the bridge and a
    /// hub worker's wake would be lost.
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
    /// `RawCompletion` into a `Completion` — returning `None` (and so discarding)
    /// a cancelled op whose `pending` entry is already gone.
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
