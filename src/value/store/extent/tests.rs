// audited: 2026-10-27
//! An extent's append of items it makes: each lands in order, a growth claims twice the capacity, and a refused claim changes nothing.
//!
//! docs/impl/mutable.md

use std::panic::{catch_unwind, AssertUnwindSafe};

use super::{no_room, Extent};
use crate::value::fiberheap::FiberHeap;

// The counter-factual: an append that claims only the items it adds passes
// every tree model test, because each claim still holds the new nodes. It
// claims on every append past the capacity, and copies the whole extent each
// time. The claims here start at the floor of four small items and double.
// A claim that panics, as a foreign shell's does, leaves the items and the
// borrow count as they were.
#[test]
fn an_append_of_made_items_doubles_its_claims() {
    let mut heap = FiberHeap::new();
    let region = heap.new_runtime_region();
    let extent: Extent<u64> = Extent::empty();
    let mut next = 0;
    let mut make = || {
        next += 1;
        next - 1
    };
    let mut claims = Vec::new();
    for n in [1, 3, 1, 3, 2] {
        extent.append_with(
            n,
            |want| {
                claims.push(want);
                heap.alloc_room_in_region(want, region)
            },
            &mut make,
        );
    }
    assert_eq!(claims, [4, 8, 16], "the claims of the appends");
    let want: Vec<u64> = (0..10).collect();
    assert_eq!(*extent.read(), want, "the items the appends made");
    extent.append_with(6, no_room, &mut make);
    let refused = catch_unwind(AssertUnwindSafe(|| {
        extent.append_with(1, |_| panic!("no region of this heap"), &mut make)
    }));
    assert!(refused.is_err(), "an append past the capacity claims");
    let want: Vec<u64> = (0..16).collect();
    assert_eq!(*extent.read(), want, "the items after a refused claim");
}
