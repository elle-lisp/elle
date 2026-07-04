//! `CompletionHub` accounting tests.
//!
//! The hub is the one channel every background worker feeds. These tests pin
//! the combined `in_flight` invariant the single-channel design rests on: +1
//! per worker submit, −1 once per `RawCompletion` reaped at the drain site, and
//! nothing else touches the counter (so a cancel — which only removes the
//! pending entry — cannot double-decrement). They use `PoolOp::Sleep { nanos:
//! 0 }` and `PoolOp::Task` because those need no file descriptor, so the hub is
//! exercised on any platform without a real I/O resource.

use super::super::*;
use crate::io::SubmissionId;
use std::time::Duration;

#[test]
fn hub_in_flight_increments_on_submit_decrements_on_reap() {
    let mut hub = CompletionHub::new();
    assert_eq!(hub.in_flight(), 0, "fresh hub has no in-flight work");

    hub.submit(SubmissionId::from_raw(1), PoolOp::Sleep { nanos: 0 })
        .unwrap();
    assert_eq!(hub.in_flight(), 1, "submit raises the combined counter");

    // The worker runs a zero-length sleep and reports back. recv_blocking is
    // the sole reap site and must lower the counter exactly once.
    let rc = hub
        .recv_blocking(Some(Duration::from_secs(5)))
        .expect("worker completion must arrive");
    match rc {
        RawCompletion::Pool(pc) => assert_eq!(pc.id, 1),
        RawCompletion::Stdin(_) => panic!("expected a Pool completion"),
    }
    assert_eq!(
        hub.in_flight(),
        0,
        "reaping the completion clears the counter"
    );
}

#[test]
fn hub_task_result_round_trips_as_pool_completion() {
    let mut hub = CompletionHub::new();
    hub.submit(
        SubmissionId::from_raw(42),
        PoolOp::Task(Box::new(|| (7, b"hello".to_vec()))),
    )
    .unwrap();

    let rc = hub
        .recv_blocking(Some(Duration::from_secs(5)))
        .expect("task completion must arrive");
    match rc {
        RawCompletion::Pool(pc) => {
            assert_eq!(pc.id, 42);
            assert_eq!(pc.result_code, 7);
            assert_eq!(pc.data, b"hello");
        }
        RawCompletion::Stdin(_) => panic!("expected a Pool completion"),
    }
    assert_eq!(hub.in_flight(), 0);
}

#[test]
fn hub_drains_a_burst_without_leaking_in_flight() {
    let mut hub = CompletionHub::new();
    for id in 1..=3 {
        hub.submit(SubmissionId::from_raw(id), PoolOp::Sleep { nanos: 0 })
            .unwrap();
    }
    assert_eq!(hub.in_flight(), 3);

    // Collect all three completions. drain_raw and recv_blocking share the one
    // decrement site, so however the burst is split between them the counter
    // must reach zero — and never underflow (saturating).
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut seen = 0usize;
    while seen < 3 {
        if let Some(_rc) = hub.recv_blocking(Some(Duration::from_millis(200))) {
            seen += 1;
        }
        seen += hub.drain_raw().len();
        assert!(
            std::time::Instant::now() < deadline,
            "burst did not drain within 5s (saw {seen}/3)"
        );
    }
    assert_eq!(seen, 3, "exactly the three submitted completions arrive");
    assert_eq!(
        hub.in_flight(),
        0,
        "draining the whole burst clears the counter"
    );
}

/// A reaped completion the caller discards (the cancellation shape: the
/// pending entry is gone, so the cook fn returns `None`) still decrements the
/// counter exactly once. The hub has no cancel path of its own, so there is no
/// second decrement to race — the invariant holds by construction. This test
/// stands in for that: reaping without cooking leaves `in_flight` at zero.
#[test]
fn hub_reap_decrements_even_when_result_is_discarded() {
    let mut hub = CompletionHub::new();
    hub.submit(SubmissionId::from_raw(9), PoolOp::Sleep { nanos: 0 })
        .unwrap();
    assert_eq!(hub.in_flight(), 1);

    // Reap but drop the RawCompletion on the floor (as a cancelled op's cook
    // would, returning None) — the counter still falls to zero.
    let _discarded = hub.recv_blocking(Some(Duration::from_secs(5)));
    assert_eq!(hub.in_flight(), 0);
}
