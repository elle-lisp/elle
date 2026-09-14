// audited: 2026-09-14
// What a macro scope's open and close owe each other: the transient region's
// physical id comes back, and comes back once.
//
// docs/impl/region/model.md

use super::*;
use crate::value::fiberheap::FiberHeap;

/// A cons, the smallest thing an expansion can wrap an argument into.
fn wrapped() -> HeapObject {
    HeapObject::Pair(Pair::new(Value::int(1), Value::NIL))
}

#[test]
fn a_scope_that_wraps_nothing_returns_its_transient_id() {
    // The reserved → free exit at the expansion site. A macro called on atoms
    // alone wraps no argument, so the scope's transient region is never
    // materialized: no entry, no page, no count, and no teardown that could ever
    // return its id. The close is the only thing that can.
    //
    // Counter-factual: with the close leaving the receipt alone, the mint below
    // takes a fresh id off `next_physical` instead, and every expansion in the
    // process raises the largest id by one for good.
    let mut heap = FiberHeap::new();
    let scope = begin_macro_scope(&mut heap);
    let transient = scope.arg_region();

    reclaim_macro_scope(&mut heap, scope);

    assert_eq!(
        heap.new_runtime_region(),
        transient,
        "an expansion that wrapped no argument must hand its transient id back",
    );
}

#[test]
fn a_scope_that_wrapped_an_argument_books_its_transient_id_once() {
    // The other half, and the reason the close cannot simply push the id. An
    // expansion that DID wrap an argument leaves a live region, which the
    // reclaim frees — and that teardown already books the id. A close that
    // pushed as well would put the same id in the free list twice, and
    // `new_runtime_region` cannot tell the copies apart: its skip loop rejects
    // an id that is already live, and neither of two unmaterialized mints is.
    //
    // The trap: the first mint below is equal either way, so a test that stops
    // at `assert_eq!` reads green over a double booking. The second mint is what
    // sees it.
    let mut heap = FiberHeap::new();
    let scope = begin_macro_scope(&mut heap);
    let transient = scope.arg_region();
    alloc_in_region(&mut heap, wrapped(), transient);

    reclaim_macro_scope(&mut heap, scope);

    let first = heap.new_runtime_region();
    let second = heap.new_runtime_region();
    assert_eq!(first, transient, "the reclaim's own teardown books the id");
    assert_ne!(
        second, first,
        "the id reached the free list twice — two logical regions on one id",
    );
}

#[test]
fn a_run_of_scopes_that_wrap_nothing_issues_no_new_id() {
    // What the strand costs, stated as the gauge reads it. `region_ids_issued`
    // is `next_physical`, which a mint raises only when the free list is empty,
    // so a loop of scopes that each return their transient holds it flat. One
    // scope runs first, to put the loop's steady state in the free list.
    //
    // The object, region and byte gauges cannot see this: an unmaterialized
    // region holds none of what they count. What it costs is the region table,
    // one `Option<RegionEntry>` slot per stranded id.
    let mut heap = FiberHeap::new();
    let scope = begin_macro_scope(&mut heap);
    reclaim_macro_scope(&mut heap, scope);
    let before = heap.region_ids_issued();

    for _ in 0..10_000 {
        let scope = begin_macro_scope(&mut heap);
        reclaim_macro_scope(&mut heap, scope);
    }

    let after = heap.region_ids_issued();
    assert_eq!(
        after,
        before,
        "10000 expansions that wrapped nothing issued {} physical ids — each \
         one is a transient region that never came back",
        after - before,
    );
}

#[test]
fn a_reissued_transient_carries_an_ordinary_region() {
    // The returned id is an ordinary free id, not a special one: the next scope
    // takes it, wraps an argument into it, and the value resolves back to that
    // region through its page header. A close that returned the id while leaving
    // stale table or generation state behind would trip the stale-deref check
    // here instead of somewhere far away.
    let mut heap = FiberHeap::new();
    let first = begin_macro_scope(&mut heap);
    let transient = first.arg_region();
    reclaim_macro_scope(&mut heap, first);

    let second = begin_macro_scope(&mut heap);
    assert_eq!(second.arg_region(), transient, "the id is reissued");
    let val = alloc_in_region(&mut heap, wrapped(), second.arg_region());
    assert_eq!(region_of(&heap, val), Some(transient));

    reclaim_macro_scope(&mut heap, second);
    assert_eq!(
        region_rc(&heap, transient),
        0,
        "the reclaim balances the wrapped argument's own reference",
    );
}
