//! Eventfd-bridge tests for the io_uring platform.
//!
//! On the uring platform the scheduler's single blocking wait is one
//! `io_uring_enter`. Work that cannot lift to the ring (a `Task` closure, a
//! `Resolve`, stdin) runs on the thread-pool hub and posts NO ring CQE; for its
//! completion to wake that one wait, a pool worker must raise a bridge eventfd
//! whose standing `POLL_ADD` produces a CQE. These tests pin that the bridge
//! actually wakes the wait — without it, a hub completion is invisible to the
//! ring and only surfaces on a later, separately-woken tick (or, pre-bridge,
//! after a wakeup-rescue cap).

use super::*;

/// A `Task` (pure hub work, no ring CQE) submitted on the uring backend must
/// wake the single `io_uring_enter` wait and be returned by one `wait()` call.
///
/// The closure sleeps 250 ms — longer than the pre-bridge 100 ms wakeup-rescue
/// cap — so a capped wait would return EMPTY (stranding the completion until a
/// later tick) while the bridged wait blocks on the ring until the eventfd
/// fires at ~250 ms and returns the completion.
///
/// `wait(5000)` uses a bounded timeout, not `-1`: a deaf bridge then surfaces
/// as an empty return after 5 s rather than an infinite hang that would wedge
/// the whole `cargo test` run.
#[test]
fn uring_pool_task_wakes_the_single_wait_past_old_cap() {
    crate::value::arena::with_test_region(|| {
        let backend = AsyncBackend::new().unwrap();

        let req = IoRequest {
            op: IoOp::Task(crate::io::request::TaskFn::new(Box::new(|| {
                std::thread::sleep(std::time::Duration::from_millis(250));
                (0, Vec::new())
            }))),
            port: crate::value::Value::NIL,
            timeout: None,
        };
        let id = backend
            .submit(&req, crate::io::pending::Submitter::for_test())
            .unwrap();

        let completions = backend.wait(5000).unwrap();
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
    });
}
