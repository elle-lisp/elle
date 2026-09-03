//! What one activation's dues record owes, and what it refuses to owe twice.

use super::*;

/// The ledger never dereferences a region — it only records and compares ids —
/// so any distinct real id serves. The trap: `RuntimeRegion::new` refuses the
/// reserved low ids, so a test id must start above them.
fn region(id: u32) -> RuntimeRegion {
    RuntimeRegion::new(id).expect("test region ids are above the reserved range")
}

#[test]
fn a_repeated_deferral_is_owed_once() {
    // The trap: a tail-recursive `go` re-enters the activation with the SAME
    // closure every iteration, so the same region is deferred once per step —
    // but the frame took exactly one reference and owes exactly one decref.
    // Counter-factual: pushing unconditionally releases N times for one
    // reference, freeing the closure under the recursion still running in it.
    let mut dues = ActivationDues::default();
    dues.defer(region(7));
    dues.defer(region(7));
    dues.defer(region(7));
    assert_eq!(
        dues.deferred,
        vec![region(7)],
        "one reference, one release, however many tail calls named it",
    );
}

#[test]
fn distinct_deferrals_are_each_owed() {
    // A single tail call can strand both channels — a merged closure-cycle
    // arena and the callee closure's own region — and each names a reference
    // the frame separately owns.
    let mut dues = ActivationDues::default();
    dues.defer(region(3));
    dues.defer(region(4));
    assert_eq!(dues.deferred, vec![region(3), region(4)]);
}

#[test]
fn the_abandoned_take_leaves_the_node_standing() {
    // An abandoned exit runs the deferred set and nothing else: the node rides
    // out to the caller that may still park the frame.
    let mut dues = ActivationDues::with_owner_node(region(9));
    dues.defer(region(2));
    assert_eq!(dues.take_deferred(), vec![region(2)]);
    assert_eq!(
        dues.owner_node,
        Some(region(9)),
        "the node is not the abandoned walk's to release",
    );
    assert!(
        dues.take_deferred().is_empty(),
        "a second take finds nothing — the release ran once",
    );
}

#[test]
fn an_activation_that_neither_adopted_nor_tail_called_owes_nothing() {
    assert!(ActivationDues::default().is_empty());
    assert!(!ActivationDues::with_owner_node(region(5)).is_empty());
    let mut deferred_only = ActivationDues::default();
    deferred_only.defer(region(6));
    assert!(!deferred_only.is_empty());
}
