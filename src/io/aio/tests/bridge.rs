//! audited: 2026-09-30
//! Eventfd-bridge tests for the io_uring platform.
//!
//! src/io/AGENTS.md
//!
//! On the uring platform the scheduler's single blocking wait is one
//! `io_uring_enter`. Work that cannot lift to the ring (a `Task` closure, a
//! `Resolve`, stdin) runs on the thread-pool hub and posts NO ring CQE; for its
//! completion to wake that one wait, a pool worker must raise a bridge eventfd
//! whose standing `POLL_ADD` produces a CQE. These tests pin that the bridge
//! actually wakes the wait — without it, a hub completion is invisible to the
//! ring and only surfaces on a later, separately-woken tick.

use super::*;

/// A `Task` (pure hub work, no ring CQE) submitted on the uring backend must
/// wake the single `io_uring_enter` wait and be returned by one `wait()` call.
///
/// The closure sleeps 250 ms. The counter-factual is a wait capped at 100 ms
/// to rescue a missed wakeup: it returns EMPTY and strands the completion
/// until a later tick, while the bridged wait blocks on the ring until the
/// eventfd fires at ~250 ms and returns the completion.
///
/// `wait(PATIENCE)` is bounded rather than `wait(None)`: a deaf bridge then
/// surfaces as an empty return after 5 s rather than an infinite hang that
/// would wedge the whole `cargo test` run.
#[test]
fn a_pool_task_wakes_the_rings_single_wait() {
    crate::value::arena::with_test_region(|| {
        let backend = AsyncBackend::new().unwrap();

        let req = IoRequest::unbounded(
            IoOp::Task(crate::io::request::TaskFn::new(Box::new(|| {
                std::thread::sleep(std::time::Duration::from_millis(250));
                (0, Vec::new())
            }))),
            crate::value::Value::NIL,
        );
        let id = backend
            .submit(&req, crate::io::pending::Submitter::for_test())
            .unwrap();

        let completions = backend.wait(PATIENCE).unwrap();
        assert_eq!(
            completions.len(),
            1,
            "wait() must return the pool Task completion; a sub-250ms cap strands it"
        );
        assert_eq!(completions[0].id, id);
        assert!(
            completions[0].result.is_ok(),
            "task completion: {:?}",
            completions[0].result
        );
        Completion::discard_all(completions);
    });
}
