//! audited: 2026-11-17
//! Worker reuse, and the two costs it must not bring with it.
//!
//! docs/impl/io/descriptor.md
//!
//! A finished operation gives its worker back to the crew rather than ending
//! the thread, so these tests ask three things of that crew: that the next
//! operation lands on the worker already there, that an operation which parks
//! never delays one behind it, and that a crew nobody is using shrinks.
//!
//! A zero-length `PoolOp::Sleep` is the operation that must finish. The pool's
//! count of the threads it started, and the order its workers parked in, tell
//! a reused worker from a fresh one. A sleep with no bound, or a `PoolOp::Read`
//! on a pipe nobody writes to, is the operation that must park until a stop
//! reaches it.

use super::super::*;
use crate::io::request::Bound;
use crate::io::SubmissionId;
use std::time::{Duration, Instant};

/// How long a test waits for a completion before calling the crew broken.
const PATIENCE: Duration = Duration::from_secs(5);

/// A sleep's duration is its bound, so a zero-length one finishes at once.
fn instant() -> Bounds {
    Bounds::new(Bound::per_op(Duration::ZERO), None)
}

/// Reap one completion and assert it answers `id`.
fn reap(hub: &mut CompletionHub, id: u64) {
    match hub.recv_blocking(Some(PATIENCE)) {
        Some(RawCompletion::Pool(pc)) => {
            assert_eq!(pc.id, id, "the completion must answer the submission")
        }
        _ => panic!("no completion for submission {id} within {PATIENCE:?}"),
    }
}

/// Run a sleep that ends at once under `id`, and reap it. The caller must have
/// nothing else in flight.
fn run_one(hub: &mut CompletionHub, id: u64) {
    hub.submit(SubmissionId::from_raw(id), PoolOp::Sleep, instant())
        .expect("the pool must accept the submission");
    reap(hub, id);
}

/// The second of two operations runs on the worker the first one left behind.
///
/// The trap this also guards: a worker that counted itself idle only *after*
/// publishing its completion would make this race. A caller learns the
/// operation ended by reaping that completion, and submits the next one the
/// moment it has — so anything the crew must be true by then has to be true
/// before the completion is sent.
#[test]
fn a_second_submission_runs_on_the_first_workers_thread() {
    let mut hub = CompletionHub::new();
    run_one(&mut hub, 1);
    run_one(&mut hub, 2);
    assert_eq!(
        hub.pool.started(),
        1,
        "the worker that finished the first operation must take the second"
    );
}

/// The next job goes to the worker that parked most recently.
///
/// The claim is that the warm worker takes the work and the ones the traffic
/// no longer reaches age out of the keepalive at the bottom of the stack. It
/// is a reasoned order, not a measured one: a run that changed only this
/// order, on the three-core runner where the pool's costs show, moved nothing
/// that could be told from noise.
///
/// The counter-factual: taking from the front of the list — the worker that
/// has slept longest — passes every other test in this file and fails this
/// one, so the order cannot drift unnoticed.
#[test]
fn the_next_job_goes_to_the_worker_that_parked_last() {
    let mut hub = CompletionHub::new();
    // Each sleep has no bound and waits for its stop, so both workers are busy
    // at once and the order they park in is this test's to choose.
    for id in [1, 2].map(SubmissionId::from_raw) {
        let gate = hub.bounds(id, Bound::NONE);
        hub.submit(id, PoolOp::Sleep, gate)
            .expect("the pool must accept a gated submission");
    }
    assert_eq!(
        hub.pool.started(),
        2,
        "two operations that overlap run on two workers"
    );

    for id in [1, 2] {
        hub.stop(SubmissionId::from_raw(id));
        reap(&mut hub, id);
        hub.forget_stop(SubmissionId::from_raw(id));
    }
    assert_eq!(hub.pool.parked(), vec![1, 2], "they park as they finish");

    // The worker that takes the job leaves the list and parks again at its
    // end, so only the most recent one leaves the order as it was.
    run_one(&mut hub, 3);
    assert_eq!(hub.pool.started(), 2, "a parked worker took the job");
    assert_eq!(
        hub.pool.parked(),
        vec![1, 2],
        "the job must go to the most recently parked worker, not the coldest one"
    );
}

/// A worker nobody gives another job to retires, and the submission after that
/// starts a fresh one.
///
/// The counter-factual: a crew that parks its workers and never retires them
/// passes the reuse test above and fails this one — a program that does a
/// little I/O and then stops would hold those stacks for the rest of the
/// process.
#[test]
fn an_idle_worker_retires_and_the_next_submission_starts_another() {
    let keepalive = Duration::from_millis(50);
    let mut hub = CompletionHub::with_keepalive(keepalive);
    run_one(&mut hub, 1);
    // Ten keepalives, because what is being waited for is a loaded machine
    // scheduling the worker's wakeup, not the wait itself.
    std::thread::sleep(keepalive * 10);
    assert!(
        hub.pool.parked().is_empty(),
        "a worker that waited out its keepalive must be gone"
    );
    run_one(&mut hub, 2);
    assert_eq!(hub.pool.started(), 2, "so the next job starts another");
}

/// A zero keepalive is the counter-factual switch: no reuse at all.
///
/// `(*io-keepalive* 0)` is what a program binds to measure what reuse buys, so
/// it has to mean what it says — a worker that retires the moment it has
/// nothing to do, and a thread per operation.
#[test]
fn a_zero_keepalive_gives_every_operation_its_own_thread() {
    let mut hub = CompletionHub::with_keepalive(Duration::ZERO);
    run_one(&mut hub, 1);
    run_one(&mut hub, 2);
    assert_eq!(
        hub.pool.started(),
        2,
        "a worker with no keepalive must not wait for a second operation"
    );
}

/// Operations that park delay no submission behind them.
///
/// The counter-factual: a crew of some fixed size would pass every other test
/// here and fail this one. The eight reads below wait on a pipe nobody writes
/// to and end only when `stop` reaches them, so a crew that hands out a bounded
/// number of workers has none left for the ninth submission — and the fiber
/// that would write to the pipe is exactly the fiber such a bound would be
/// holding up.
#[test]
fn parked_operations_do_not_delay_the_next_submission() {
    const PARKED: u64 = 8;
    const PROMPT: u64 = 100;

    let mut hub = CompletionHub::new();
    // One pipe for all eight reads: they are told to wait on a descriptor that
    // never becomes readable, and one such descriptor serves however many
    // operations wait on it.
    let mut fds = [0 as libc::c_int; 2];
    assert_eq!(
        unsafe { libc::pipe(fds.as_mut_ptr()) },
        0,
        "the test needs a pipe"
    );
    let (read_fd, write_fd) = (fds[0], fds[1]);

    for id in 1..=PARKED {
        let id = SubmissionId::from_raw(id);
        // A stop pipe and no deadline: nothing but `stop` ends these.
        let bounds = hub.bounds(id, Bound::NONE);
        hub.submit(id, PoolOp::read(read_fd, 16), bounds)
            .expect("the pool must accept a parking submission");
    }

    hub.submit(SubmissionId::from_raw(PROMPT), PoolOp::Sleep, instant())
        .expect("the pool must accept a submission behind eight parked ones");
    assert_eq!(
        hub.in_flight(),
        PARKED as usize + 1,
        "nine operations are out"
    );

    let deadline = Instant::now() + PATIENCE;
    loop {
        match hub.recv_blocking(Some(PATIENCE)) {
            Some(RawCompletion::Pool(pc)) if pc.id == PROMPT => break,
            Some(other) => panic!(
                "only the ninth operation can complete while the pipe is silent, got id {}",
                match other {
                    RawCompletion::Pool(pc) => pc.id,
                    RawCompletion::Stdin(sc) => sc.id,
                }
            ),
            None => assert!(
                Instant::now() < deadline,
                "the operation behind eight parked ones never ran"
            ),
        }
    }

    // Wind the parked reads down before the pipe goes: each reports
    // `-ECANCELED` once its stop pipe is written.
    for id in 1..=PARKED {
        hub.stop(SubmissionId::from_raw(id));
    }
    let deadline = Instant::now() + PATIENCE;
    while hub.in_flight() > 0 {
        hub.recv_blocking(Some(PATIENCE));
        assert!(
            Instant::now() < deadline,
            "{} stopped operations never reported",
            hub.in_flight()
        );
    }

    unsafe {
        libc::close(read_fd);
        libc::close(write_fd);
    }
}
