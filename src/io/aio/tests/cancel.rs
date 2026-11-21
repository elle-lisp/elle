//! audited: 2026-11-20
//! A cancel through the backend: the one `:cancelled` answer it gets, and the false a cancel that reaches nothing returns.
//!
//! docs/impl/io/cancel.md

use super::*;

/// Give a submission time to reach a worker, and then a moment more, so a test
/// that wants the operation already parked in its syscall gets that order.
///
/// Reports whether a worker came out. `workers()` counts a submission from the
/// moment its thread spawns, so a true here says the operation is out rather
/// than that the worker has reached its syscall. The pause is what makes the
/// interesting order the likely one. Always false on the ring, which runs its
/// operations in the kernel and has no worker to wait for.
pub(super) fn wait_for_worker(backend: &AsyncBackend) -> bool {
    for _ in 0..200 {
        if backend.workers() > 0 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    false
}

/// Cancel `id`, and assert that the cancel reached it, that it answers once
/// with a `:cancelled` error, and that its entry and its worker go.
///
/// A bounded number of waits, because the property under test is exactly that
/// this terminates. The pool's `wait` blocks on the hub channel only while
/// `hub.in_flight()` is above zero; the ring's blocks unconditionally
/// (docs/impl/io/platforms.md). So the pool has a state the ring does not: an
/// operation still in `pending` while no worker is out for it. `wait` then
/// returns nothing without blocking, and whoever waits on that operation is
/// never woken again.
///
/// The answer is built from nothing the entry held. Cooking the completion
/// instead would read the port or write the worker's bytes into a buffer the
/// fiber that asked may already have freed.
pub(super) fn cancel_answers(backend: &AsyncBackend, id: SubmissionId, what: &str) {
    assert_eq!(
        backend.cancel(id),
        Ok(true),
        "the cancel reached the {what}"
    );
    let mut answers = Vec::new();
    for _ in 0..100 {
        answers.extend(backend.wait(TICK).unwrap());
        if !answers.is_empty() && !backend.has_pending() && backend.workers() == 0 {
            break;
        }
    }
    answers.extend(backend.poll());
    let kinds: Vec<_> = answers.iter().map(|c| (c.id, c.error_kind())).collect();
    Completion::discard_all(answers);
    assert_eq!(
        kinds,
        vec![(id, Some(Value::keyword("cancelled")))],
        "the cancelled {what} answers once, with :cancelled",
    );
    assert!(
        !backend.has_pending(),
        "the cancelled {what} is still pending with {} worker(s) out",
        backend.workers(),
    );
    assert_eq!(
        backend.workers(),
        0,
        "the cancelled {what} never gave its worker back"
    );
}

/// Cancel a pool operation already out on its worker, and assert what
/// [`cancel_answers`] does.
pub(super) fn assert_cancel_retires(backend: &AsyncBackend, id: SubmissionId, what: &str) {
    assert!(
        wait_for_worker(backend),
        "the pool never took the {what} out to a worker"
    );
    cancel_answers(backend, id, what);
}

/// A cancel of an id nothing is filed under returns false, and nothing answers
/// it, on either backend.
///
/// The trap: a cancel races the completion it would end, so one routinely
/// arrives after the reap. A mark or an answer left for such an id has no
/// operation behind it, and a reader that trusted the true would wait for an
/// answer that never comes.
#[test]
fn a_cancel_that_reaches_nothing_returns_false_on_either_backend() {
    crate::value::arena::with_test_region(|| {
        for (backend, which) in [
            (AsyncBackend::new().unwrap(), "the platform default"),
            (AsyncBackend::new_thread_pool().unwrap(), "the thread pool"),
        ] {
            let sleep = IoRequest::unbounded(
                IoOp::Sleep {
                    duration: Duration::ZERO,
                },
                Value::NIL,
            );
            let id = backend
                .submit(&sleep, crate::io::pending::Submitter::for_test())
                .unwrap();
            let mut got = Vec::new();
            for _ in 0..100 {
                got.extend(backend.wait(TICK).unwrap());
                if !got.is_empty() && !backend.has_pending() {
                    break;
                }
            }
            assert_eq!(completion_ids(got), vec![id], "{which}: the sleep answers");

            assert_eq!(backend.cancel(id), Ok(false), "{which}: a reaped id");
            let never = SubmissionId::from_raw(1 << 40);
            assert_eq!(backend.cancel(never), Ok(false), "{which}: an unminted id");
            assert!(
                completion_ids(backend.wait(TICK).unwrap()).is_empty(),
                "{which}: a cancel that reached nothing answers nothing",
            );
        }
    });
}
