// audited: 2026-09-29
//! The push helpers mutate a mutable @array in place, count the region of what they insert, and copy an immutable one.
//!
//! docs/impl/jit.md
//! docs/impl/region/owner.md

use super::*;
use crate::value::arena::{alloc_in_fresh_region, region_rc};
use crate::value::heap::{HeapObject, Pair};

/// `elle_jit_array_push` mutates the input @array in place and returns the same
/// Value, the contract of the VM's `handle_array_push`
/// (src/vm/data/structops.rs, through `push_with_incref`).
///
/// The counter-factual: a helper that clones the contents and returns a fresh
/// @array gives `(push @arr x)` the wrong meaning, and skips the cross-region
/// count (`incref_inserted_element`) that the next test pins.
#[test]
fn array_push_mutates_in_place_and_returns_same_value() {
    crate::value::arena::with_test_region(|| {
        let mut vm = vm_with_primitives();

        let h = crate::primitives::ctx::TestHeap::new();
        let arr = h.ctx().array_mut(vec![]);
        let v = Value::int(42);
        let ret = elle_jit_array_push(arr.tag, arr.payload, v.tag, v.payload, vm_arg(&mut vm));
        let ret_val = ret.to_value();
        // Returned Value must be identical (same heap object) to the input.
        assert_eq!(
            ret_val.tag, arr.tag,
            "elle_jit_array_push must return the same @array (tag mismatch)"
        );
        assert_eq!(
            ret_val.payload, arr.payload,
            "elle_jit_array_push must return the same @array (payload mismatch)"
        );
        // Input @array must reflect the push.
        let inner = arr.as_array_mut().expect("input is @array");
        assert_eq!(
            inner.borrow().len(),
            1,
            "@array length should be 1 after push"
        );
        assert_eq!(
            inner.borrow()[0],
            v,
            "@array element should be the pushed value"
        );
    });
}

/// `elle_jit_array_push` increfs the source region when it inserts a heap Value
/// into an @array that lives in a different region.
///
/// The counter-factual: without the incref the source region can drop to RC=0
/// and be freed while the @array still references it, the heap corruption
/// `tests/impl/jit-double-import-uaf.lisp` checks for.
#[test]
fn array_push_track_inserts_cross_region_value() {
    crate::value::arena::with_test_region(|| {
        let mut vm = vm_with_primitives();
        let heap_ptr = vm.heap_ptr;
        let arr_region = unsafe { (*heap_ptr).new_runtime_region() };
        let arr = ctx_in(&mut vm, arr_region).array_mut(vec![]);
        // Allocate a heap value in a different fresh region.
        let (cross, source_rid) = alloc_in_fresh_region(
            unsafe { &mut *heap_ptr },
            HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)),
        );
        let rc_before = region_rc(unsafe { &*heap_ptr }, source_rid);
        let _ret = elle_jit_array_push(
            arr.tag,
            arr.payload,
            cross.tag,
            cross.payload,
            vm_arg(&mut vm),
        );
        let rc_after = region_rc(unsafe { &*heap_ptr }, source_rid);
        assert_eq!(
            rc_after,
            rc_before + 1,
            "elle_jit_array_push must incref the source region of an inserted cross-region value"
        );
    });
}

/// `elle_jit_push` (the IntrPush intrinsic helper) keeps the same contract: it
/// mutates a mutable @array in place and increfs the region of a cross-region
/// value it inserts.
#[test]
fn intr_push_track_inserts_cross_region_value() {
    use crate::jit::runtime::elle_jit_push;
    crate::value::arena::with_test_region(|| {
        let mut vm = vm_with_primitives();
        let heap_ptr = vm.heap_ptr;
        let arr_region = unsafe { (*heap_ptr).new_runtime_region() };
        let arr = ctx_in(&mut vm, arr_region).array_mut(vec![]);
        let (cross, source_rid) = alloc_in_fresh_region(
            unsafe { &mut *heap_ptr },
            HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)),
        );
        let rc_before = region_rc(unsafe { &*heap_ptr }, source_rid);
        let mut jit_ctx = crate::jit::JitCtx::new(&mut vm as *mut VM);
        let ret = elle_jit_push(
            arr.tag,
            arr.payload,
            cross.tag,
            cross.payload,
            &mut jit_ctx as *mut _,
        );
        let ret_val = ret.to_value();
        assert_eq!(
            (ret_val.tag, ret_val.payload),
            (arr.tag, arr.payload),
            "elle_jit_push must return the same @array Value"
        );
        let rc_after = region_rc(unsafe { &*heap_ptr }, source_rid);
        assert_eq!(
            rc_after,
            rc_before + 1,
            "elle_jit_push must incref the source region of an inserted cross-region value"
        );
    });
}

/// `elle_jit_push` on an *immutable* array yields a fresh copy born in a
/// freshly minted call-result region (`run_alloc_intrinsic`). That matches
/// `elle_jit_put` and `elle_jit_del`, and the `produces_call_result_region`
/// model the compiler uses for `%array-push`: a value-based
/// `DecrefValueRegion` frees the result.
///
/// The counter-factual: build the source array in a distinct region `source`,
/// and assert the copy's region is NOT `source` and its contents match.
#[test]
fn push_immutable_result_is_fresh_region() {
    use crate::jit::runtime::elle_jit_push;
    let mut vm = VM::new();
    let heap_ptr = vm.heap_ptr;
    let source = unsafe { (*heap_ptr).new_runtime_region() };
    let arr = ctx_in(&mut vm, source).array(vec![Value::int(1), Value::int(2)]);
    let mut jit_ctx = crate::jit::JitCtx::new(&mut vm as *mut VM);
    let result = elle_jit_push(
        arr.tag,
        arr.payload,
        Value::int(3).tag,
        Value::int(3).payload,
        &mut jit_ctx as *mut _,
    )
    .to_value();
    let contents: Vec<i64> = result
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_int().unwrap())
        .collect();
    assert_eq!(
        contents,
        vec![1, 2, 3],
        "immutable push appends to a fresh copy"
    );
    let region = region_of(&vm, result).expect("array has a region");
    assert_ne!(
        region, source,
        "elle_jit_push's immutable-array copy must be born in its own minted \
         call-result region, not the source's",
    );
    unsafe {
        (*heap_ptr).decref_region_if_present(source);
        (*heap_ptr).decref_region_if_present(region);
    }
}
