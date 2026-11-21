// audited: 2026-11-20
//! The passes that bring a backend to rest: the ring drained, and the pool operations that address a region stopped and awaited.
//!
//! docs/impl/io/bytes.md
//! docs/impl/io/inflight.md

use super::*;

impl AsyncBackendInner {
    /// Cancel and drain every in-flight io_uring operation so no kernel-owned
    /// buffer outlives the backend. docs/impl/io/bytes.md says what that is for.
    ///
    /// Two things shape the loop. Operations a hub worker runs post no CQE
    /// here, so they never shrink `pending` and the loop stops on the first pass
    /// that does not; the pass count bounds it anyway. And every entry is put
    /// at rest before the drain, so each CQE retires its entry and answers
    /// nothing. No completion comes out of the drain, so the shrink is the
    /// whole progress test.
    #[cfg(target_os = "linux")]
    pub(super) fn quiesce_pending(&mut self) {
        if !matches!(self.platform, PlatformBackend::Uring(_)) || self.pending.is_empty() {
            return;
        }
        // Where `drain_cqes` puts what it cooked. Every entry here is at rest,
        // so it stays empty; it exists because the drain is one
        // function with one signature. Emptied through `discard` all the same,
        // because a completion owns what it built
        // (docs/impl/io/inflight.md) and dropping one here would strand that.
        let mut sink: VecDeque<Completion> = VecDeque::new();
        let mut passes = 0u32;
        while !self.pending.is_empty() && passes < 64 {
            passes += 1;
            let before = self.pending.len();
            let ids: Vec<SubmissionId> = self.pending.ids();

            // Nobody is left to read any of these, and the heap their values
            // live on may already be gone — this is the teardown a stranded
            // backend gets (`IoBackend::quiesce`). Put them at rest, so the
            // drain retires each entry and builds nothing from it.
            self.pending.cancel_all();

            // Cancel everything still pending. A cancel for an id the ring
            // doesn't know (a hub worker's op) returns a tagged `-ENOENT` CQE
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
                if ring.wait(Some(Duration::from_millis(50))).is_err() {
                    break;
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

    /// Stop every pool operation that carries a stop pipe, and wait for the
    /// completion of each that [`awaited_at_rest`](Self::awaited_at_rest)
    /// names before returning.
    ///
    /// A pool worker reads into the caller's buffer and writes from the
    /// payload's pages as the kernel does on the ring, and the heap frees those
    /// regions right after this returns (docs/impl/io/bytes.md). Only an
    /// operation with a stop pipe addresses a region, and a stop ends it at once,
    /// so the wait is short; it is bounded anyway, as the ring's drain is.
    /// Every entry is put at rest first, so each completion retires its entry
    /// and answers nothing.
    pub(super) fn quiesce_workers(&mut self) {
        let stoppable: Vec<SubmissionId> = self
            .hub
            .stoppable()
            .into_iter()
            .filter(|id| self.pending.contains(*id))
            .collect();
        if stoppable.is_empty() {
            return;
        }
        let awaited = self.awaited_at_rest();
        self.pending.cancel_all();
        for id in &stoppable {
            self.hub.stop(*id);
        }
        let origin_heap = self.origin_heap;
        let gen = self.unicode_generation;
        let mut waits = 0u32;
        while waits < 64 && awaited.iter().any(|id| self.pending.contains(*id)) {
            waits += 1;
            let Some(rc) = self.hub.recv_blocking(Some(Duration::from_millis(50))) else {
                continue;
            };
            let id = SubmissionId::from_raw(match &rc {
                RawCompletion::Pool(pc) => pc.id,
                RawCompletion::Stdin(sc) => sc.id,
            });
            let cooked = cook_raw(
                rc,
                &mut self.pending,
                &mut self.fd_states,
                &mut self.buffer_pool,
                origin_heap,
                gen,
            );
            if let Some(c) = cooked {
                c.discard();
            }
            self.hub.forget_stop(id);
        }
    }

    /// The pool operations a backend coming to rest waits for: each that
    /// carries a stop pipe, but a `File` walk. A walk addresses no region, and
    /// a step on a mount that has stopped answering would hold the teardown up
    /// for as long as the mount does (docs/impl/io/bytes.md).
    pub(super) fn awaited_at_rest(&self) -> Vec<SubmissionId> {
        let stops = self.hub.stoppable();
        self.pending
            .iter()
            .filter(|(id, op)| stops.contains(id) && !matches!(op, PendingOp::File { .. }))
            .map(|(id, _)| *id)
            .collect()
    }
}
