// audited: 2026-09-19
//! That a merged slot mint-or-reuses and an unmerged one mints fresh.
//!
//! docs/impl/region/merging.md

use super::*;

#[test]
fn merged_slot_reuses_after_first_mint() {
    crate::value::arena::with_test_region(|| {
        let mut vm = VM::new();
        vm.push_activation_region_map();
        let slot = StaticRegion::new(7).expect("nonzero slot");
        let merged = [slot.get()];
        let merged = crate::value::closure::MergedSlots::from_sorted(&merged);
        // Child alloc: slot not yet mapped → mints.
        let child = vm.runtime_region_for_alloc_slot_maybe_merged(slot, merged);
        // Parent alloc against the same MERGED slot: reuses the child's region.
        let parent = vm.runtime_region_for_alloc_slot_maybe_merged(slot, merged);
        assert_eq!(
            child, parent,
            "a merged slot mint-or-reuses: the parent's alloc must reuse the \
             physical region the child's alloc minted"
        );
    });
}

#[test]
fn unmerged_slot_mints_fresh_each_execution() {
    crate::value::arena::with_test_region(|| {
        let mut vm = VM::new();
        vm.push_activation_region_map();
        let slot = StaticRegion::new(7).expect("nonzero slot");
        // The slot is NOT in the merged set — the unmerged baseline.
        let empty = crate::value::closure::MergedSlots::from_sorted(&[]);
        let first = vm.runtime_region_for_alloc_slot_maybe_merged(slot, empty);
        let second = vm.runtime_region_for_alloc_slot_maybe_merged(slot, empty);
        assert_ne!(
            first, second,
            "an unmerged slot mints a fresh physical region on every execution \
             (byte-identical to the pre-merge baseline)"
        );
    });
}
