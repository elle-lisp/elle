use super::*;

impl AsyncBackend {
    /// Cancel a pending I/O operation by submission ID.
    ///
    /// For io_uring: submits IORING_OP_ASYNC_CANCEL. The original SQE will
    /// generate a CQE with result = -ECANCELED; the cancel SQE's CQE is
    /// tagged and skipped by drain_cqes.
    ///
    /// For the thread pool: just remove the pending entry. The worker still
    /// runs to completion and its `RawCompletion` decrements the hub's
    /// `in_flight` once at the drain site; the cooked completion is then
    /// discarded because the pending entry is gone. We must NOT also decrement
    /// `in_flight` here — that would underflow the combined count.
    pub(crate) fn cancel(&self, id: SubmissionId) -> Result<(), String> {
        let mut inner = self.inner.borrow_mut();
        match inner.platform {
            #[cfg(target_os = "linux")]
            PlatformBackend::Uring(ref mut ring) => {
                crate::io::uring::submit_uring_cancel(ring, id)?;
            }
            PlatformBackend::ThreadPool => {
                inner.pending.remove(&id);
            }
        }
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
                    // One blocking wait, no cap. Ring ops post their own CQEs;
                    // hub work (getaddrinfo, `Task`, stdin) that posts no ring
                    // CQE wakes this wait through the standing eventfd POLL_ADD —
                    // a worker raises the eventfd after publishing, its poll
                    // fires a CQE, and `wait_uring` clears the eventfd and
                    // re-arms. The hub channel is then drained by `drain_ready`
                    // below. No cap means a genuinely lost wakeup hangs rather
                    // than being downgraded to a bounded stall — the property
                    // that makes the scheduler reasoned-about.
                    crate::io::uring::wait_uring(
                        ring,
                        timeout,
                        pending,
                        buffer_pool,
                        fd_states,
                        completions,
                        origin_heap,
                        hub.eventfd(),
                    )?;
                }
                PlatformBackend::ThreadPool => {
                    // One channel, all sources. A crossbeam `recv()`
                    // registers-before-sleeps on the sole hub channel, so a
                    // worker's publish can never be missed while the scheduler is
                    // asleep — the lost-wakeup fix by construction. No caps: a
                    // genuinely lost wakeup would hang here rather than be
                    // downgraded to a bounded stall, which is exactly the
                    // property that makes the scheduler reasoned-about.
                    if hub.in_flight() > 0 {
                        let waited = match timeout {
                            None => hub.recv_blocking(None),
                            Some(0) => None, // poll mode — already drained above
                            Some(ms) => hub.recv_blocking(Some(Duration::from_millis(ms))),
                        };
                        if let Some(rc) = waited {
                            if let Some(c) =
                                cook_raw(rc, pending, fd_states, buffer_pool, origin_heap)
                            {
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

    pub(crate) fn extract_write_bytes(data: &Value) -> Vec<u8> {
        if let Some(s) = data.with_string(|s| s.as_bytes().to_vec()) {
            s
        } else if let Some(b) = data.as_bytes() {
            b.to_vec()
        } else if let Some(b) = data.as_bytes_mut() {
            b.borrow().clone()
        } else if let Some(b) = data.as_string_mut() {
            b.borrow().clone()
        } else {
            format!("{}", data).into_bytes()
        }
    }

    /// Check if there are pending operations.
    /// Used by the async scheduler to determine when to exit the event loop.
    #[allow(dead_code)]
    pub(crate) fn has_pending(&self) -> bool {
        let inner = self.inner.borrow();
        !inner.pending.is_empty()
    }
}
