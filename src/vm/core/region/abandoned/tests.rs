// audited: 2026-09-19
//! The abandoned-frame walk over a COMPILED frame, whose locals arrive
//! spilled rather than on the fiber stack.
//!
//! docs/impl/region/mechanism.md

use super::*;

/// Allocate a pair in a fresh region of its own, returning `(value, region)`.
fn owned_pair(vm: &mut VM) -> (Value, RuntimeRegion) {
    use crate::value::heap::{HeapObject, Pair};
    let heap = unsafe { &mut *vm.heap_ptr };
    crate::value::arena::alloc_in_fresh_region(
        heap,
        HeapObject::Pair(Pair::new(Value::int(1), Value::NIL)),
    )
}

#[test]
fn a_compiled_frames_spilled_locals_run_the_value_route() {
    crate::value::arena::with_test_region(|| {
        let mut vm = VM::new();
        vm.push_activation_region_map();
        let (held, region) = owned_pair(&mut vm);
        // Slot 1 holds it; slot 0 is nil, as an already-run release reads.
        let spill = [Value::NIL, held];
        vm.release_abandoned(&[1], &[], Value::NIL, FrameLocals::Spilled(&spill));
        assert_eq!(
            vm.heap().region_rc(region),
            0,
            "a value route the compiled frame still owed must run off the \
             spilled local its table names",
        );
    });
}

/// The counter-factual for the test above: the walk runs the releases the TABLE
/// names, not everything the frame spilled. A slot missing from the table is a
/// route the emitter declined, and the frame owes nothing for it.
#[test]
fn a_compiled_frames_spilled_walk_runs_only_the_tabled_slots() {
    crate::value::arena::with_test_region(|| {
        let mut vm = VM::new();
        vm.push_activation_region_map();
        let (held, region) = owned_pair(&mut vm);
        let spill = [Value::NIL, held];
        vm.release_abandoned(&[0], &[], Value::NIL, FrameLocals::Spilled(&spill));
        assert_eq!(
            vm.heap().region_rc(region),
            1,
            "slot 1 is not in the table, so its region keeps the frame's reference",
        );
    });
}

#[test]
fn a_compiled_frames_spilled_walk_skips_an_unminted_payload() {
    crate::value::arena::with_test_region(|| {
        let mut vm = VM::new();
        vm.push_activation_region_map();
        let (payload, region) = owned_pair(&mut vm);
        let spill = [payload];
        // A native raise installs the payload with no retain of its own, so the
        // frame's reference IS the delivery and its release stays owed.
        vm.release_abandoned(&[0], &[], payload, FrameLocals::Spilled(&spill));
        assert_eq!(
            vm.heap().region_rc(region),
            1,
            "the skipped release is the delivery the catcher reads the payload by",
        );
        // An emit-raised error minted the delivery itself, so nothing is exempt.
        vm.fiber.delivery.record_mint(payload);
        vm.release_abandoned(&[0], &[], payload, FrameLocals::Spilled(&spill));
        assert_eq!(
            vm.heap().region_rc(region),
            0,
            "a recorded mint funds the delivery, so the frame's own reference is \
             reclaimed by the compiled walk too",
        );
    });
}

/// The slot route needs no spill: its receipt is the activation region map, which
/// a compiled prologue pushes and `elle_jit_resolve_alloc_region` records into.
/// A slot still mapped when the frame is abandoned is a release that did not run.
#[test]
fn a_compiled_frames_walk_runs_the_slot_route_off_the_activation_map() {
    crate::value::arena::with_test_region(|| {
        let mut vm = VM::new();
        vm.push_activation_region_map();
        let slot = StaticRegion::new(9).expect("nonzero slot");
        let region = vm.runtime_region_for_alloc_slot(slot);
        vm.release_abandoned(&[], &[slot.get()], Value::NIL, FrameLocals::Spilled(&[]));
        assert_eq!(
            vm.heap().region_rc(region),
            0,
            "a `DecrefRegion` the compiled frame never reached must run off the \
             mapping its alloc minted",
        );
    });
}
