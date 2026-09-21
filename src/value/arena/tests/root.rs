// audited: 2026-09-20
// What a process-root registration does to the count on a value's region, and
// which reference the teardown sweep's decref gives back.
//
// docs/impl/region/rules.md

use super::*;
use crate::value::fiberheap::FiberHeap;

/// A cons in a region of its own, holding the one reference its allocation
/// mints — the shape a host reaches a value through.
fn owned_value(heap: &mut FiberHeap) -> (Value, RuntimeRegion) {
    let (val, rid) =
        alloc_in_fresh_region(heap, HeapObject::Pair(Pair::new(Value::int(1), Value::NIL)));
    assert_eq!(
        region_rc(heap, rid),
        1,
        "a fresh allocation holds exactly one reference"
    );
    (val, rid)
}

/// `RootRef::Take` consumes the reference the caller holds: the count does not
/// move, and the sweep's decref is that same reference coming back.
#[test]
fn a_taken_process_root_consumes_the_callers_reference() {
    let mut heap = FiberHeap::new();
    let (val, rid) = owned_value(&mut heap);

    register_process_root(&mut heap, val, RootRef::Take);
    assert_eq!(
        region_rc(&heap, rid),
        1,
        "taking the caller's reference mints nothing"
    );

    teardown_process_root_regions(&mut heap);
    assert_eq!(
        region_rc(&heap, rid),
        0,
        "the sweep gives the taken reference back"
    );
}

/// `RootRef::Mint` funds the root itself, so the root outlives the release the
/// caller still owes — the release of the value the registered one was reached
/// through (docs/impl/region/rules.md § "The program value is the host's to
/// release").
///
/// The counter-factual is the registration above, made where the caller holds
/// nothing to give: the owner's release frees the region under the root, and
/// the sweep then decrefs a region that is already gone.
#[test]
fn a_minted_process_root_outlives_the_owners_release() {
    let mut heap = FiberHeap::new();
    let (val, rid) = owned_value(&mut heap);

    register_process_root(&mut heap, val, RootRef::Mint);
    assert_eq!(region_rc(&heap, rid), 2, "a minted root raises the count");

    // The owner gives its own reference back — the REPL releasing the tuple a
    // destructured binding came out of. The root is what holds the region now.
    decref_region(&mut heap, Some(rid));
    assert_eq!(
        region_rc(&heap, rid),
        1,
        "the region survives its owner's release"
    );

    teardown_process_root_regions(&mut heap);
    assert_eq!(
        region_rc(&heap, rid),
        0,
        "the sweep gives the minted reference back"
    );
}

/// Two leaves of one destructuring `def` can name one region, and the sweep
/// decrefs a region once per registration. A minted root funds itself, so the
/// count answers every one of those decrefs.
#[test]
fn two_minted_roots_on_one_region_fund_two_decrefs() {
    let mut heap = FiberHeap::new();
    let (val, rid) = owned_value(&mut heap);

    register_process_root(&mut heap, val, RootRef::Mint);
    register_process_root(&mut heap, val, RootRef::Mint);
    assert_eq!(
        region_rc(&heap, rid),
        3,
        "each registration mints a reference of its own"
    );

    decref_region(&mut heap, Some(rid));
    teardown_process_root_regions(&mut heap);
    assert_eq!(
        region_rc(&heap, rid),
        0,
        "two registrations and the owner's release balance exactly"
    );
}

/// An immediate occupies no region, so there is nothing to register and nothing
/// to mint — the type-level form of "only heap values pin a region".
#[test]
fn a_registered_immediate_mints_nothing() {
    let mut heap = FiberHeap::new();
    register_process_root(&mut heap, Value::int(42), RootRef::Mint);
    register_process_root(&mut heap, Value::NIL, RootRef::Take);
    assert_eq!(
        teardown_process_root_regions(&mut heap),
        0,
        "an immediate registered nothing for the sweep to release"
    );
}
