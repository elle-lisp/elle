// audited: 2026-09-29
//! How compiled code leaves on an error: the pending-exception probe, and the release of what an abandoned frame owes.
//!
//! docs/impl/jit.md
//! docs/impl/region/mechanism.md

use super::*;

#[test]
fn test_has_exception() {
    crate::value::arena::with_test_region(|| {
        let mut vm = vm_with_primitives();

        // Initially no exception
        let result = elle_jit_has_exception(vm_arg(&mut vm) as u64);
        assert_eq!(result, JitValue::bool_val(false));

        // Set an error signal
        let err = vm.escaping_error("division-by-zero", "test");
        vm.fiber.signal = Some((crate::value::SIG_ERROR, err));

        // Now should return true
        let result = elle_jit_has_exception(vm_arg(&mut vm) as u64);
        assert_eq!(result, JitValue::bool_val(true));

        // Clear signal
        vm.fiber.signal = None;

        // Should return false again
        let result = elle_jit_has_exception(vm_arg(&mut vm) as u64);
        assert_eq!(result, JitValue::bool_val(false));
    });
}

/// The compiled error exit's ABI: `elle_jit_release_abandoned_frame` reads the
/// value routes out of the locals the exit spilled, the slot routes out of the
/// activation map the prologue pushed, and the payload off `fiber.signal`.
///
/// The trap: the tables are laid out as two SEPARATE buffers of different widths
/// — `u16` local slots and `u32` static region ids. Reading either through the
/// other's pointer names a slot the frame never had, so this drives both at once
/// with distinct ids and asserts each release lands where it belongs.
#[test]
fn release_abandoned_frame_runs_both_routes_off_the_compiled_exits_buffers() {
    use crate::hir::region::StaticRegion;
    use crate::value::arena::alloc_in_fresh_region;
    use crate::value::heap::{HeapObject, Pair};

    crate::value::arena::with_test_region(|| {
        let mut vm = VM::new();
        let heap_ptr = vm.heap_ptr;
        let vm_ptr = vm_arg(&mut vm);

        // The compiled prologue: this activation's region-remap frame.
        crate::jit::dispatch::elle_jit_push_region_map(vm_ptr);

        // A slot-routed alloc: the mint records slot → physical, which is the
        // release's receipt.
        let region_slot = StaticRegion::new(5).expect("nonzero slot");
        let mapped = RuntimeRegion::new(crate::jit::dispatch::elle_jit_resolve_alloc_region(
            vm_ptr,
            region_slot.get(),
        ))
        .expect("the mint returns a live region");

        // A value-routed local in local slot 2.
        let (held, held_region) = alloc_in_fresh_region(
            unsafe { &mut *heap_ptr },
            HeapObject::Pair(Pair::new(Value::int(1), Value::NIL)),
        );
        let locals = [Value::NIL, Value::NIL, held];

        // The raise: an immediate payload exempts nothing.
        vm.fiber.signal = Some((crate::value::SIG_ERROR, Value::int(7)));

        let slots: [u16; 1] = [2];
        let regions: [u32; 1] = [region_slot.get()];
        crate::jit::dispatch::elle_jit_release_abandoned_frame(
            vm_ptr,
            slots.as_ptr(),
            slots.len() as u64,
            regions.as_ptr(),
            regions.len() as u64,
            locals.as_ptr(),
            locals.len() as u64,
        );
        crate::jit::dispatch::elle_jit_pop_region_map(vm_ptr);

        assert_eq!(
            unsafe { &*heap_ptr }.region_rc(held_region),
            0,
            "the value route named by local slot 2 must release what the spill \
             holds there",
        );
        assert_eq!(
            unsafe { &*heap_ptr }.region_rc(mapped),
            0,
            "the slot route must release the physical region its alloc mapped",
        );
    });
}
