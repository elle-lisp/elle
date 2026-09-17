//! audited: 2026-09-16
//! What a hold retains while its operation is in flight, and when it lets go.

use super::*;

/// A submitted operation's operands outlive the release of the region they
/// were born in.
///
/// The trap: the entry holds `Value`s, and a `Value` is a bare pointer that
/// keeps nothing alive. Nothing else counts a reference held by the pending
/// table, so without the entry's own retain the region goes on the release
/// below and the completion assembles a result out of freed memory.
///
/// The counter-factual: with the retain removed, `region_generation` moves,
/// which is the store recording that this incarnation of the region is gone.
#[test]
fn a_held_operand_survives_the_release_of_its_region() {
    let mut pool = BufferPool::new();
    let mut table = PendingTable::new();
    let (heap, region, watcher) = value_in_fresh_region();
    // SAFETY: the heap is leaked for the process.
    let h = unsafe { &mut *heap };
    let born = h.region_generation(region.get());

    table.insert(
        id(1),
        watch_op(&mut pool, watcher),
        Submitter::detached(heap),
    );
    h.decref_region(region);

    assert_eq!(
        h.region_generation(region.get()),
        born,
        "the submitted operation's hold must outlast its fiber's release",
    );

    // And the hold is what was holding it: taking the entry lets go, and the
    // region is then reclaimed by the reference the release already dropped.
    match table.take(id(1)) {
        Taken::Live(op) => op.retire(0, &mut pool),
        _ => panic!("a detached submission is never withheld"),
    }
    assert_ne!(
        h.region_generation(region.get()),
        born,
        "disposing of the entry must let the region go",
    );
}

/// An operand that lives in an `Owned` region is held through the region
/// whose count reclamation actually listens to.
///
/// The trap: `incref` on an `Owned` region is a no-op by construction — that
/// region has no count left, and its owner's subtree drop frees it however
/// many references point at it. A hold that retained the operand's own
/// region would compile, run, and hold nothing.
///
/// The counter-factual: retain the operand's own region instead of its root,
/// and the owner's release below frees the subtree — `region_generation`
/// moves for the member, and the completion assembles from freed memory.
/// `tests/elle/process-io.lisp` § 10 is the program that gets there: a
/// connection accepted inside a process, adopted into the per-connection
/// `ev/spawn`'s subtree, written to by a `handle-io-forward` submission.
#[test]
fn an_owned_operand_is_held_through_its_reclamation_root() {
    let mut pool = BufferPool::new();
    let mut table = PendingTable::new();
    let (heap, member, watcher) = value_in_fresh_region();
    // SAFETY: the heap is leaked for the process.
    let h = unsafe { &mut *heap };
    // The owner: a Counted region that adopts the operand's, exactly as an
    // activation adopts what it captures.
    let owner = h.new_runtime_region();
    h.adopt_region(owner, member);
    let born = h.region_generation(member.get());

    table.insert(
        id(1),
        watch_op(&mut pool, watcher),
        Submitter::detached(heap),
    );
    // The owner's last reference goes while the operation is in flight. Only
    // a count on the owner can stop the subtree drop taking the member.
    h.decref_region(owner);

    assert_eq!(
        h.region_generation(member.get()),
        born,
        "the operand's owner was released under a submitted operation — a \
         retain on the operand's own region cannot hold an Owned member",
    );

    match table.take(id(1)) {
        Taken::Live(op) => op.retire(0, &mut pool),
        _ => panic!("a detached submission is never withheld"),
    }
    assert_ne!(
        h.region_generation(member.get()),
        born,
        "disposing of the entry must let the subtree go",
    );
}

/// The fiber a submission names outlives the release of the region it was
/// born in.
///
/// The trap: the fiber is the one held value that is read on every drain
/// rather than once at the completion — `asker_finished` dereferences it
/// each time the backend sweeps. A fiber value is a bare pointer like any
/// other, so the region it lives in going while its operation is still in
/// flight makes that sweep a read of freed memory. The check that exists to
/// notice a fiber is gone is the last place that may assume it is there.
///
/// Counter-factual: with the fiber left out of `OperandHold::take`, the
/// release below frees its region, and `tests/integration/fixtures/
/// region-fiber-abort-io-protect-uaf.lisp` faults under `--trace=guardfree`
/// — a fiber aborted mid-`ev/sleep` is exactly this shape.
#[test]
fn a_held_fiber_survives_the_release_of_its_region() {
    let mut pool = BufferPool::new();
    let mut table = PendingTable::new();
    let heap = crate::value::arena::leaked_test_heap();
    // SAFETY: the heap is leaked for the process.
    let h = unsafe { &mut *heap };
    let (fiber, _handle) = fiber_in(h, FiberStatus::Paused);
    let region = crate::value::arena::region_of(h, fiber).expect("the fiber has a region");
    let born = h.region_generation(region.get());

    table.insert(id(1), sleep_op(&mut pool), Submitter::new(heap, fiber));
    h.decref_region(region);

    assert_eq!(
        h.region_generation(region.get()),
        born,
        "the fiber's region went while the operation it asked for was still \
         in flight — every sweep from here reads freed memory",
    );

    // Still readable, which is the whole point of holding it.
    assert!(
        table.orphaned_to_stop().is_empty(),
        "a fiber still parked has not finished with its operation",
    );
    match table.take(id(1)) {
        Taken::Live(op) => op.retire(0, &mut pool),
        _ => panic!("a fiber still parked has a reader"),
    }
    assert_ne!(
        h.region_generation(region.get()),
        born,
        "disposing of the entry must let the fiber's region go",
    );
}

/// A resubmission keeps its hold rather than dropping and retaking it.
///
/// The trap: a read that needs another syscall goes out through `take` and
/// back through `restore`. If the take released, the operands would be
/// unheld between the two — and once the asking fiber has gone, that hold is
/// the last reference, so the port would be freed between two syscalls of
/// one read.
#[test]
fn a_restored_operation_still_holds_its_operands() {
    let mut pool = BufferPool::new();
    let mut table = PendingTable::new();
    let (heap, region, watcher) = value_in_fresh_region();
    // SAFETY: the heap is leaked for the process.
    let h = unsafe { &mut *heap };
    let born = h.region_generation(region.get());

    table.insert(
        id(1),
        watch_op(&mut pool, watcher),
        Submitter::detached(heap),
    );
    h.decref_region(region);
    let op = match table.take(id(1)) {
        Taken::Live(op) => op,
        _ => panic!("a detached submission is never withheld"),
    };
    table.restore(id(1), op);

    assert_eq!(
        h.region_generation(region.get()),
        born,
        "a resubmitted operation must still hold its operands",
    );
    match table.take(id(1)) {
        Taken::Live(op) => op.retire(0, &mut pool),
        _ => panic!("a detached submission is never withheld"),
    }
}

/// Teardown lets go of every hold at once, for operations that will never
/// complete and so will never be disposed of by a completion.
#[test]
fn releasing_the_holds_lets_every_operand_region_go() {
    let mut pool = BufferPool::new();
    let mut table = PendingTable::new();
    let (heap, region, watcher) = value_in_fresh_region();
    // SAFETY: the heap is leaked for the process.
    let h = unsafe { &mut *heap };
    let born = h.region_generation(region.get());

    table.insert(
        id(1),
        watch_op(&mut pool, watcher),
        Submitter::detached(heap),
    );
    h.decref_region(region);
    table.release_holds();

    assert_ne!(
        h.region_generation(region.get()),
        born,
        "teardown must let go of what the entries were holding",
    );
    // Idempotent: the second release names no store and reaches nothing.
    table.release_holds();
}
