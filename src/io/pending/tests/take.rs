//! audited: 2026-09-16
//! What a take answers: live, cancelled, or a fiber that ended without saying.

use super::*;

/// An uncancelled submission's completion is handed its entry to cook.
#[test]
fn a_live_submission_is_taken_live() {
    let mut pool = BufferPool::new();
    let mut table = PendingTable::new();
    table.insert(id(1), sleep_op(&mut pool), detached());
    assert!(matches!(table.take(id(1)), Taken::Live(_)));
    assert!(table.is_empty(), "taking an entry removes it");
}

/// A cancelled submission's completion is told so, and only once: the mark
/// leaves with the entry, so nothing about this id survives to affect a
/// later lookup.
#[test]
fn a_cancelled_submission_is_taken_cancelled_exactly_once() {
    let mut pool = BufferPool::new();
    let mut table = PendingTable::new();
    table.insert(id(1), sleep_op(&mut pool), detached());
    table.mark_cancelled(id(1));
    match table.take(id(1)) {
        Taken::Cancelled(op) => op.retire(0, &mut pool),
        _ => panic!("a marked submission must be reported cancelled"),
    }
    assert!(matches!(table.take(id(1)), Taken::Unknown));
}

/// Cancelling an id that is no longer in flight marks nothing.
///
/// The trap: `io/cancel` races the completion it is trying to prevent, so a
/// cancel routinely arrives for an operation already reaped and delivered.
/// A mark filed then would have no completion coming to clear it.
#[test]
fn cancelling_a_reaped_submission_marks_nothing() {
    let mut pool = BufferPool::new();
    let mut table = PendingTable::new();
    table.insert(id(1), sleep_op(&mut pool), detached());
    assert!(matches!(table.take(id(1)), Taken::Live(_)));

    table.mark_cancelled(id(1));
    assert!(matches!(table.take(id(1)), Taken::Unknown));
}

/// A resubmitted operation is still in flight, so a cancel still reaches
/// it.
///
/// A read that needs another syscall to reach its newline, its count or its
/// EOF leaves the table and comes back (`drain_cqes`). What must not follow
/// is the entry reading as reaped in between — a cancel issued after the
/// restore would then mark nothing, and the next completion would be cooked
/// for a fiber that is gone.
#[test]
fn a_resubmitted_operation_can_still_be_cancelled() {
    let mut pool = BufferPool::new();
    let mut table = PendingTable::new();
    table.insert(id(1), sleep_op(&mut pool), detached());
    let op = match table.take(id(1)) {
        Taken::Live(op) => op,
        _ => panic!("an unmarked entry is live"),
    };
    table.restore(id(1), op);

    table.mark_cancelled(id(1));
    match table.take(id(1)) {
        Taken::Cancelled(op) => op.retire(0, &mut pool),
        _ => panic!("a resubmitted operation must still be cancellable"),
    }
}

/// An entry whose asking fiber has reached a terminal state has no reader,
/// whether or not anybody cancelled it.
///
/// The trap: nothing marks this id. The fiber that asked ran to a terminal
/// state by a path that told no one, so the only evidence left is the fiber
/// itself.
#[test]
fn an_entry_whose_fiber_ended_is_taken_orphaned() {
    let mut pool = BufferPool::new();
    let mut table = PendingTable::new();
    let heap = crate::value::arena::leaked_test_heap();
    // SAFETY: the heap is leaked for the process.
    let h = unsafe { &mut *heap };
    let (fiber, _handle) = fiber_in(h, FiberStatus::Error);

    table.insert(id(1), sleep_op(&mut pool), Submitter::new(heap, fiber));
    match table.take(id(1)) {
        Taken::Orphaned(op) => op.retire(0, &mut pool),
        _ => panic!("an operation whose fiber ended has no reader"),
    }
}

/// The same entry read while its fiber is still running is live.
///
/// The counter-factual for the test above: without it, a check that simply
/// answered "gone" would pass that one and withhold every completion in the
/// process.
#[test]
fn an_entry_whose_fiber_still_runs_is_taken_live() {
    let mut pool = BufferPool::new();
    let mut table = PendingTable::new();
    let heap = crate::value::arena::leaked_test_heap();
    // SAFETY: the heap is leaked for the process.
    let h = unsafe { &mut *heap };
    let (fiber, _handle) = fiber_in(h, FiberStatus::Paused);

    table.insert(id(1), sleep_op(&mut pool), Submitter::new(heap, fiber));
    match table.take(id(1)) {
        Taken::Live(op) => op.retire(0, &mut pool),
        _ => panic!("an operation whose fiber is still parked has a reader"),
    }
}

/// A fiber unwinding after `fiber/abort` is `:paused`, and its operation is
/// left alone.
///
/// The trap: an abort resumes the fiber to unwind, and that unwinding can
/// suspend and be resumed again, so the fiber still has a result to come
/// back for. Ending its operation then is the same red as never ending an
/// orphaned one, from the opposite cause.
#[test]
fn an_unwinding_fiber_keeps_its_operation() {
    let mut pool = BufferPool::new();
    let mut table = PendingTable::new();
    let heap = crate::value::arena::leaked_test_heap();
    // SAFETY: the heap is leaked for the process.
    let h = unsafe { &mut *heap };
    let (fiber, handle) = fiber_in(h, FiberStatus::Paused);

    table.insert(id(1), sleep_op(&mut pool), Submitter::new(heap, fiber));
    assert!(
        table.orphaned_to_stop().is_empty(),
        "a fiber still unwinding has not finished with its operation",
    );

    // It finishes, and only then is the operation ended.
    handle.with_mut(|f| f.status = FiberStatus::Error);
    assert_eq!(table.orphaned_to_stop(), vec![id(1)]);
}

/// A table holding one entry whose asking fiber has ended, and one whose
/// submission names no fiber at all.
fn table_with_an_ended_fiber() -> (
    BufferPool,
    PendingTable,
    *mut crate::value::fiberheap::FiberHeap,
) {
    let mut pool = BufferPool::new();
    let mut table = PendingTable::new();
    let heap = crate::value::arena::leaked_test_heap();
    // SAFETY: the heap is leaked for the process.
    let h = unsafe { &mut *heap };
    let (fiber, _handle) = fiber_in(h, FiberStatus::Dead);

    table.insert(id(1), sleep_op(&mut pool), Submitter::new(heap, fiber));
    // A second entry nobody is waiting on as a fiber, so a sweep reporting
    // everything is distinguishable from one reporting what it should.
    table.insert(id(2), sleep_op(&mut pool), Submitter::detached(heap));
    (pool, table, heap)
}

/// An operation whose fiber has ended is reported for a stop exactly once,
/// and only that operation is.
///
/// The trap: a backend sweeps on every drain, which is every tick of the
/// event loop. Without the record, each tick would ask the same worker
/// again for an operation whose completion is already on its way.
#[test]
fn an_operation_whose_fiber_ended_is_reported_for_a_stop_once() {
    let (_pool, mut table, _heap) = table_with_an_ended_fiber();

    assert_eq!(
        table.orphaned_to_stop(),
        vec![id(1)],
        "only the entry whose fiber ended needs stopping",
    );
    assert!(
        table.orphaned_to_stop().is_empty(),
        "a second sweep must ask nothing: the first ask is still in flight",
    );
}

/// Being asked to stop does not change the answer the operation gets.
///
/// The counter-factual is marking the id cancelled instead, which is the
/// other way to end an operation early: a cancelled id falls silent, and
/// the scheduler is still holding the pairing this completion has to
/// retire.
#[test]
fn an_operation_asked_to_stop_is_still_taken_orphaned() {
    let (mut pool, mut table, _heap) = table_with_an_ended_fiber();
    assert_eq!(table.orphaned_to_stop(), vec![id(1)]);

    match table.take(id(1)) {
        Taken::Orphaned(op) => op.retire(0, &mut pool),
        _ => panic!("an operation asked to stop must still answer"),
    }
}

/// A submission made on behalf of no fiber is never withheld and never
/// swept. `handle-io-forward` submits for a child scheduler, whose reader is
/// a queue rather than a fiber here.
#[test]
fn a_submission_with_no_fiber_is_never_withheld() {
    let (mut pool, mut table, _heap) = table_with_an_ended_fiber();
    assert_eq!(
        table.orphaned_to_stop(),
        vec![id(1)],
        "the detached submission is not swept",
    );
    match table.take(id(2)) {
        Taken::Live(op) => op.retire(0, &mut pool),
        _ => panic!("a submission naming no fiber always has a reader"),
    }
}

/// Teardown withholds every result at once.
#[test]
fn cancel_all_marks_everything_in_flight() {
    let mut pool = BufferPool::new();
    let mut table = PendingTable::new();
    for n in 1..=3 {
        table.insert(id(n), sleep_op(&mut pool), detached());
    }
    table.cancel_all();
    for n in 1..=3 {
        assert!(
            matches!(table.take(id(n)), Taken::Cancelled(_)),
            "submission {n} must be withheld at teardown",
        );
    }
}
