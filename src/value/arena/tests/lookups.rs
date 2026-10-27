// audited: 2026-10-26
//! The container funnels resolve a region only for an edge, so a write of immediates within capacity resolves none.
//!
//! docs/impl/mutable.md
//!
//! An immediate has no region, so a store of one records no edge whether or
//! not the funnel resolved the container's region first. Only the count of
//! regions resolved tells the two apart. Each container takes one write before
//! the count starts, so every counted write fits the room that write claimed.

use super::*;
use crate::value::types::TableKey;

/// The regions `write` resolves.
fn resolved_by(write: impl FnOnce()) -> u64 {
    let before = regions_resolved();
    write();
    regions_resolved() - before
}

#[test]
fn a_struct_write_of_immediates_resolves_no_region() {
    let mut heap = FiberHeap::new();
    let (st, _) = struct_in_fresh_region(&mut heap);
    struct_put_with_rebind(&mut heap, st, TableKey::Int(0), Value::int(0));
    let resolved = resolved_by(|| {
        for k in 1..8 {
            struct_put_with_rebind(&mut heap, st, TableKey::Int(k), Value::int(k));
        }
        struct_put_with_rebind(&mut heap, st, TableKey::Int(3), Value::int(30));
        for k in 0..8 {
            struct_remove_with_decref(&mut heap, st, &TableKey::Int(k));
        }
    });
    assert_eq!(
        resolved, 0,
        "a @struct put, rebind and remove resolved regions"
    );
}

#[test]
fn a_set_write_of_immediates_resolves_no_region() {
    let mut heap = FiberHeap::new();
    let (set, _) = set_in_fresh_region(&mut heap);
    set_add_with_incref(&mut heap, set, Value::int(0));
    let resolved = resolved_by(|| {
        for k in 1..8 {
            set_add_with_incref(&mut heap, set, Value::int(k));
        }
        for k in 0..8 {
            set_del_with_decref(&mut heap, set, &Value::int(k));
        }
    });
    assert_eq!(resolved, 0, "an @set add and delete resolved regions");
}

#[test]
fn an_array_write_of_immediates_resolves_no_region() {
    let mut heap = FiberHeap::new();
    let (arr, _) = array_in_fresh_region(&mut heap);
    push_with_incref(&mut heap, arr, Value::int(0));
    let resolved = resolved_by(|| {
        for k in 1..4 {
            push_with_incref(&mut heap, arr, Value::int(k));
        }
        set_at_with_rebind(&mut heap, arr, 2, Value::int(20));
        for _ in 0..4 {
            pop_with_decref(&mut heap, arr);
        }
    });
    assert_eq!(
        resolved, 0,
        "an @array push, store and pop resolved regions"
    );
}
