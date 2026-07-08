use super::*;
use crate::value::arena::{alloc_in_fresh_region, region_rc};
use crate::value::heap::{HeapObject, Pair};

/// Counterfactual: `%array-push` (the funnel native `prim_push` every
/// compiled call-position use lowers to) must call
/// `incref_inserted_element` on the pushed value so its source region's
/// RC accounts for the new reference from the destination @array.
/// Without this, the source region's RC stays at its baseline; when the
/// destination @array is later freed, cascade decref drops the entry's
/// source region RC and frees it while the pushed value is still live
/// elsewhere — UAF. `handle_intr_push` (VM bytecode) and `elle_jit_push`
/// (JIT) both route through `push_with_incref`; the NativeFn `prim_push`
/// must do the same.
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

/// Counterfactual: `%bytes-push` (`prim_bytes_push`) must bulk-append a whole
/// bytes/@bytes VALUE, not only a single integer byte. This is the linear
/// binary-append path `core.lisp`'s `push-all` relies on — the mirror of
/// `%string-push` bulk-appending a whole string. Without it, `push-all` /
/// `append` / `concat` walk a bytes source one byte at a time through the VM
/// (O(n) interpreted iterations with a large constant — orders of magnitude
/// slower than the text bulk path, the HTTP/2 body-copy bottleneck). The
/// single-integer form must keep working, so both are pinned here.
#[test]
fn bytes_push_bulk_appends_bytes_value() {
    crate::value::arena::with_test_region(|| {
        let mut vm = crate::vm::VM::new();
        let vm_ptr: *mut crate::vm::VM = &mut vm as *mut _;
        let heap_ptr = vm.heap_ptr;
        let region = unsafe { (*heap_ptr).new_runtime_region() };
        // @bytes dst = [1,2,3]; immutable bytes src = [4,5,6].
        let (dst, src) = {
            let ctx = crate::primitives::ctx::NativeCtx::with_region_vm(
                region,
                unsafe { &mut *heap_ptr },
                vm_ptr,
            );
            (ctx.bytes_mut(vec![1, 2, 3]), ctx.bytes(vec![4, 5, 6]))
        };
        // Bulk-append the whole bytes value in one shot.
        let (bits, res) = {
            let mut ctx = crate::primitives::ctx::NativeCtx::with_region_vm(
                region,
                unsafe { &mut *heap_ptr },
                vm_ptr,
            );
            prim_bytes_push(&mut ctx, &[dst, src])
        };
        assert_eq!(bits, SIG_OK, "bulk bytes append must succeed");
        assert_eq!(
            res.as_bytes_mut().unwrap().borrow().as_slice(),
            &[1, 2, 3, 4, 5, 6],
            "every source byte appended in order (@bytes extended in place)"
        );
        // The single-byte integer form still works.
        let (bits2, res2) = {
            let mut ctx = crate::primitives::ctx::NativeCtx::with_region_vm(
                region,
                unsafe { &mut *heap_ptr },
                vm_ptr,
            );
            prim_bytes_push(&mut ctx, &[dst, Value::int(9)])
        };
        assert_eq!(bits2, SIG_OK);
        assert_eq!(
            res2.as_bytes_mut().unwrap().borrow().as_slice(),
            &[1, 2, 3, 4, 5, 6, 9],
            "single integer byte still appends"
        );
    });
}

/// Counterfactual: `%pop` (the funnel native `prim_pop` every compiled
/// call-position use lowers to) MOVES the popped element out to the caller. It must do
/// two things in lockstep: release the container's stored reference
/// (`decref_removed_element`, undoing the push's `incref_inserted_element`) AND
/// hold the caller's owning reference (the pass-through retain), the retain taken
/// BEFORE the release so a sole-owned element's region is never transiently freed
/// under the returned Value (the free-before-retain UAF; `arena::pop_with_decref`,
/// the `raw-pop` oracle probe). So the source region's RC is UNCHANGED from the
/// push state after the pop — not dropped to baseline — and returns to baseline
/// only when the caller releases the moved-out result. Two defects this pins:
/// forgetting the release leaves RC one too high (a region-RC leak, the symmetric
/// defect of `push_track_inserts_cross_region_value`); forgetting the retain drops
/// RC to baseline here and frees the element under the returned Value.
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
        // The MOVE: the container's stored reference is released and the caller's
        // acquired in lockstep, so the source region's RC is unchanged from the
        // push state (baseline + 1), NOT dropped to baseline. Dropping it here
        // would free a sole-owned element under the returned Value.
        assert_eq!(
            region_rc(unsafe { &*heap_ptr }, source_rid),
            rc_baseline + 1,
            "prim_pop holds the caller's reference (RC unchanged from push, not dropped)"
        );
        // The caller releasing the moved-out result (its `DecrefValueRegion` at the
        // result's decref_point — the whole caller side, since dispatch skips its
        // own pass-through retain for the `moves_out` `%pop`) completes the move,
        // returning the source region's RC to baseline.
        crate::value::arena::decref_region(unsafe { &mut *heap_ptr }, Some(source_rid));
        assert_eq!(
            region_rc(unsafe { &*heap_ptr }, source_rid),
            rc_baseline,
            "releasing the moved-out result returns the source region RC to baseline"
        );
    });
}
