//! audited: 2026-11-17
//! Eventfd-bridge tests for the io_uring platform.
//!
//! docs/impl/io/platforms.md
//!
//! On the ring the scheduler's one blocking wait is one `io_uring_enter`. Work
//! that has no ring form (a `Resolve`, a `File` operation, stdin) runs on the
//! hub and posts no ring CQE. A pool worker raises the bridge eventfd, whose
//! standing `POLL_ADD` posts the CQE that wakes the wait. Without the bridge, a
//! hub completion surfaces only on a later tick that something else woke.

use super::*;

/// A pool completion (hub work, no ring CQE) on the uring backend must wake the
/// single `io_uring_enter` wait and be returned by one `wait()` call.
///
/// The sleep is handed to the hub directly, on whichever platform, so it posts
/// no ring CQE even where the ring runs every other sleep. It lasts 250 ms. The
/// counter-factual is a wait capped at 100 ms to rescue a missed wakeup: it
/// returns EMPTY and strands the completion until a later tick, while the
/// bridged wait blocks on the ring until the eventfd fires at ~250 ms and
/// returns the completion.
///
/// `wait(PATIENCE)` is bounded rather than `wait(None)`: a deaf bridge then
/// surfaces as an empty return after 5 s rather than an infinite hang that
/// would wedge the whole `cargo test` run.
#[test]
fn a_pool_completion_wakes_the_rings_single_wait() {
    crate::value::arena::with_test_region(|| {
        let backend = AsyncBackend::new().unwrap();
        let lasts = Bound::per_op(Duration::from_millis(250));
        let id = backend
            .submit_op(
                0,
                |d| {
                    let bounds = d.hub.bounds(d.id, lasts);
                    d.hub.submit(d.id, PoolOp::Sleep, bounds)
                },
                |buffer, ()| PendingOp::Sleep {
                    buffer_handle: buffer,
                },
            )
            .unwrap();

        let completions = backend.wait(PATIENCE).unwrap();
        assert_eq!(
            completions.len(),
            1,
            "wait() must return the pool completion; a sub-250ms cap strands it"
        );
        assert_eq!(completions[0].id, id);
        assert!(
            completions[0].result.is_ok(),
            "pool sleep completion: {:?}",
            completions[0].result
        );
        Completion::discard_all(completions);
    });
}
