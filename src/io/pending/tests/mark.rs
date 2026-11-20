//! audited: 2026-11-20
//! What a cancelled entry answers, beside the sweep of a gone fiber and a teardown.
//!
//! docs/impl/io/cancel.md

use super::*;

/// A cancel answers whatever the fiber that asked has become.
///
/// The trap: an orphan the sweep already asked to stop is the one entry where
/// both readings apply. A take that asked the fiber first would answer with
/// the error that names the gone fiber, and the caller that cancelled would
/// read an answer it did not ask for.
#[test]
fn an_orphan_asked_to_stop_and_then_cancelled_answers_cancelled() {
    let mut pool = BufferPool::new();
    let mut table = PendingTable::new();
    let heap = crate::value::arena::leaked_test_heap();
    // SAFETY: the heap is leaked for the process.
    let h = unsafe { &mut *heap };
    let (fiber, _handle) = fiber_in(h, FiberStatus::Dead);
    table.insert(id(1), sleep_op(&mut pool), Submitter::new(heap, fiber));
    assert_eq!(table.orphaned_to_stop(), vec![id(1)]);

    assert!(table.mark_cancelled(id(1)), "the entry is still in flight");
    assert!(
        table.orphaned_to_stop().is_empty(),
        "a cancelled entry is not asked to stop again",
    );
    match table.take(id(1)) {
        Taken::Unread(op, Unread::Cancelled) => op.retire(0, &[], &mut pool),
        _ => panic!("the cancel's answer is the one a cancelled entry gives"),
    }
}

/// A teardown after a cancel answers nothing for the cancelled entry either.
///
/// The trap: the cancel's answer is built on the heap the entry was filed
/// against, and a teardown may run while that heap tears its regions down.
#[test]
fn a_cancelled_entry_at_rest_answers_nothing() {
    let mut pool = BufferPool::new();
    let mut table = PendingTable::new();
    table.insert(id(1), sleep_op(&mut pool), detached());
    assert!(table.mark_cancelled(id(1)));
    table.cancel_all();
    match table.take(id(1)) {
        Taken::AtRest(op) => op.retire(0, &[], &mut pool),
        _ => panic!("a teardown puts a cancelled entry at rest"),
    }
}

/// A cancel after a teardown reaches nothing that will answer, and leaves the
/// entry at rest.
///
/// The trap: the teardown runs because the heap is going away, and the
/// cancel's answer would be built on that heap.
#[test]
fn a_cancel_after_a_teardown_leaves_the_entry_at_rest() {
    let mut pool = BufferPool::new();
    let mut table = PendingTable::new();
    table.insert(id(1), sleep_op(&mut pool), detached());
    table.cancel_all();
    assert!(
        !table.mark_cancelled(id(1)),
        "an entry at rest will not answer"
    );
    match table.take(id(1)) {
        Taken::AtRest(op) => op.retire(0, &[], &mut pool),
        _ => panic!("a cancel does not wake an entry at rest"),
    }
}

/// Each reason a result goes unread answers with its own error kind: a cancel
/// with `:cancelled`, a gone fiber with `:io-error`.
#[test]
fn each_unread_reason_answers_its_own_kind() {
    let heap = crate::value::arena::leaked_test_heap();
    for (why, kind) in [
        (Unread::Cancelled, "cancelled"),
        (Unread::Orphaned, "io-error"),
    ] {
        let answer = why.answer(id(1), heap);
        assert_eq!(answer.id, id(1), "{why:?}: the answer names its id");
        assert_eq!(answer.error_kind(), Some(Value::keyword(kind)), "{why:?}");
        answer.discard();
    }
}
