use super::*;
use crate::value::arena::{alloc_in_fresh_region, region_rc};
use crate::value::heap::{HeapObject, Pair};

/// Counterfactual: `%array-push` (the NativeFn `prim_push` reached
/// under `--checked-intrinsics`) must call `incref_inserted_element` on the
/// pushed value so its source region's RC accounts for the new
/// reference from the destination @array. Without this, the source
/// region's RC stays at its baseline; when the destination @array
/// is later freed, cascade decref drops the entry's source region
/// RC and frees it while the pushed value is still live elsewhere
/// — UAF. `handle_intr_push` (VM bytecode) and `elle_jit_push` (JIT)
/// both route through `push_with_incref`; the NativeFn `prim_push`
/// reached under `--checked-intrinsics` must do the same.
#[test]
fn push_track_inserts_cross_region_value() {
    crate::value::arena::with_test_region(|| {
        // One heap per VM (for default trait tables, needed by `array_mut`):
        // `arr`, `cross`, and the `prim_push` ctx all share the VM's single
        // heap — the one-heap invariant the RC assertion depends on.
        let mut vm = crate::vm::VM::new();
        let vm_ptr: *mut crate::vm::VM = &mut vm as *mut _;
        let heap_ptr = vm.heap_ptr;
        let region = unsafe { (*heap_ptr).new_runtime_region() };
        let arr = {
            let ctx = crate::primitives::ctx::NativeCtx::with_region_vm(
                region,
                unsafe { &mut *heap_ptr },
                vm_ptr,
            );
            ctx.array_mut(vec![])
        };
        let (cross, source_rid) = alloc_in_fresh_region(
            unsafe { &mut *heap_ptr },
            HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)),
        );
        let rc_baseline = region_rc(unsafe { &*heap_ptr }, source_rid);
        let (bits, _) = {
            let mut ctx = crate::primitives::ctx::NativeCtx::with_region_vm(
                region,
                unsafe { &mut *heap_ptr },
                vm_ptr,
            );
            prim_push(&mut ctx, &[arr, cross])
        };
        assert_eq!(bits, SIG_OK);
        assert_eq!(
            region_rc(unsafe { &*heap_ptr }, source_rid),
            rc_baseline + 1,
            "prim_push must call incref_inserted_element on cross-region value (RC=baseline+1)"
        );
    });
}

/// Counterfactual: `%pop` (the NativeFn `prim_pop` reached under
/// `--checked-intrinsics`) must call `decref_removed_element` on the popped
/// value to undo the `incref_inserted_element` from the matching push. Without
/// it, the source region's RC stays bumped — region-RC leak. The
/// symmetric defect of `push_track_inserts_cross_region_value`.
#[test]
fn pop_track_removes_cross_region_value() {
    use crate::value::arena::push_with_incref;
    crate::value::arena::with_test_region(|| {
        // One heap per test (see `push_track_inserts_cross_region_value`):
        // `arr`, `cross`, the precondition `push_with_incref`, and the
        // `prim_pop` ctx all route through the VM's single heap.
        let mut vm = crate::vm::VM::new();
        let vm_ptr: *mut crate::vm::VM = &mut vm as *mut _;
        let heap_ptr = vm.heap_ptr;
        let region = unsafe { (*heap_ptr).new_runtime_region() };
        let arr = {
            let ctx = crate::primitives::ctx::NativeCtx::with_region_vm(
                region,
                unsafe { &mut *heap_ptr },
                vm_ptr,
            );
            ctx.array_mut(vec![])
        };
        let (cross, source_rid) = alloc_in_fresh_region(
            unsafe { &mut *heap_ptr },
            HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)),
        );
        let rc_baseline = region_rc(unsafe { &*heap_ptr }, source_rid);
        let _ = push_with_incref(unsafe { &mut *heap_ptr }, arr, cross);
        assert_eq!(
            region_rc(unsafe { &*heap_ptr }, source_rid),
            rc_baseline + 1,
            "precondition: push_with_incref bumps source region RC"
        );
        let (bits, popped) = {
            let mut ctx = crate::primitives::ctx::NativeCtx::with_region_vm(
                region,
                unsafe { &mut *heap_ptr },
                vm_ptr,
            );
            prim_pop(&mut ctx, &[arr])
        };
        assert_eq!(bits, SIG_OK);
        assert_eq!(
            (popped.tag, popped.payload),
            (cross.tag, cross.payload),
            "prim_pop must return the popped Value"
        );
        assert_eq!(
            region_rc(unsafe { &*heap_ptr }, source_rid),
            rc_baseline,
            "prim_pop must call decref_removed_element on cross-region value (RC=baseline)"
        );
    });
}
