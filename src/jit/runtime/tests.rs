use super::*;

#[test]
fn test_add_integers() {
    let a = Value::int(10);
    let b = Value::int(20);
    let v = elle_jit_add(a.tag, a.payload, b.tag, b.payload).to_value();
    assert_eq!(v.as_int(), Some(30));
}

#[test]
fn test_sub_integers() {
    let a = Value::int(30);
    let b = Value::int(10);
    let v = elle_jit_sub(a.tag, a.payload, b.tag, b.payload).to_value();
    assert_eq!(v.as_int(), Some(20));
}

#[test]
fn test_mul_integers() {
    let a = Value::int(6);
    let b = Value::int(7);
    let v = elle_jit_mul(a.tag, a.payload, b.tag, b.payload).to_value();
    assert_eq!(v.as_int(), Some(42));
}

#[test]
fn test_comparison() {
    let a = Value::int(10);
    let b = Value::int(20);

    assert_eq!(
        elle_jit_lt(a.tag, a.payload, b.tag, b.payload),
        JitValue::bool_val(true)
    );
    assert_eq!(
        elle_jit_gt(a.tag, a.payload, b.tag, b.payload),
        JitValue::bool_val(false)
    );
    assert_eq!(
        elle_jit_eq(a.tag, a.payload, a.tag, a.payload),
        JitValue::bool_val(true)
    );
}

#[test]
fn test_not() {
    let t = Value::TRUE;
    let f = Value::FALSE;
    let n = Value::NIL;

    assert_eq!(elle_jit_not(t.tag, t.payload), JitValue::bool_val(false));
    assert_eq!(elle_jit_not(f.tag, f.payload), JitValue::bool_val(true));
    assert_eq!(elle_jit_not(n.tag, n.payload), JitValue::bool_val(true));
}

#[test]
fn test_eq_heap_values() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let list1 = h.ctx().pair(
            Value::int(1),
            h.ctx().pair(Value::int(2), Value::EMPTY_LIST),
        );
        let list2 = h.ctx().pair(
            Value::int(1),
            h.ctx().pair(Value::int(2), Value::EMPTY_LIST),
        );

        assert_eq!(
            elle_jit_eq(list1.tag, list1.payload, list2.tag, list2.payload),
            JitValue::bool_val(true),
            "equal lists must be eq"
        );
        assert_eq!(
            elle_jit_ne(list1.tag, list1.payload, list2.tag, list2.payload),
            JitValue::bool_val(false),
            "equal lists must not be ne"
        );

        let list3 = h.ctx().pair(
            Value::int(1),
            h.ctx().pair(Value::int(3), Value::EMPTY_LIST),
        );
        assert_eq!(
            elle_jit_eq(list1.tag, list1.payload, list3.tag, list3.payload),
            JitValue::bool_val(false),
            "different lists must not be eq"
        );

        let s1 = h.ctx().string("hello");
        let s2 = h.ctx().string("hello");
        assert_eq!(
            elle_jit_eq(s1.tag, s1.payload, s2.tag, s2.payload),
            JitValue::bool_val(true),
            "equal strings must be eq"
        );

        let s3 = h.ctx().string("world");
        assert_eq!(
            elle_jit_eq(s1.tag, s1.payload, s3.tag, s3.payload),
            JitValue::bool_val(false),
            "different strings must not be eq"
        );
    });
}

#[test]
fn test_lt_strings() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let a = h.ctx().string("apple");
        let b = h.ctx().string("banana");
        assert_eq!(
            elle_jit_lt(a.tag, a.payload, b.tag, b.payload),
            JitValue::bool_val(true)
        );
        assert_eq!(
            elle_jit_lt(b.tag, b.payload, a.tag, a.payload),
            JitValue::bool_val(false)
        );
        assert_eq!(
            elle_jit_lt(a.tag, a.payload, a.tag, a.payload),
            JitValue::bool_val(false)
        );
    });
}

#[test]
fn test_gt_strings() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let a = h.ctx().string("banana");
        let b = h.ctx().string("apple");
        assert_eq!(
            elle_jit_gt(a.tag, a.payload, b.tag, b.payload),
            JitValue::bool_val(true)
        );
        assert_eq!(
            elle_jit_gt(b.tag, b.payload, a.tag, a.payload),
            JitValue::bool_val(false)
        );
    });
}

#[test]
fn test_le_strings() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let a = h.ctx().string("apple");
        let b = h.ctx().string("banana");
        assert_eq!(
            elle_jit_le(a.tag, a.payload, b.tag, b.payload),
            JitValue::bool_val(true)
        );
        assert_eq!(
            elle_jit_le(a.tag, a.payload, a.tag, a.payload),
            JitValue::bool_val(true)
        );
        assert_eq!(
            elle_jit_le(b.tag, b.payload, a.tag, a.payload),
            JitValue::bool_val(false)
        );
    });
}

#[test]
fn test_ge_strings() {
    crate::value::arena::with_test_region(|| {
        let h = crate::primitives::ctx::TestHeap::new();
        let a = h.ctx().string("banana");
        let b = h.ctx().string("apple");
        assert_eq!(
            elle_jit_ge(a.tag, a.payload, b.tag, b.payload),
            JitValue::bool_val(true)
        );
        assert_eq!(
            elle_jit_ge(a.tag, a.payload, a.tag, a.payload),
            JitValue::bool_val(true)
        );
        assert_eq!(
            elle_jit_ge(b.tag, b.payload, a.tag, a.payload),
            JitValue::bool_val(false)
        );
    });
}

#[test]
fn test_lt_keywords() {
    // Keyword order is hash order (the portable order sorted containers use),
    // so the smaller operand is whichever spelling hashes lower.
    let x = Value::keyword("apple");
    let y = Value::keyword("banana");
    let (a, b) = if x.payload < y.payload {
        (x, y)
    } else {
        (y, x)
    };
    assert_eq!(
        elle_jit_lt(a.tag, a.payload, b.tag, b.payload),
        JitValue::bool_val(true)
    );
    assert_eq!(
        elle_jit_lt(b.tag, b.payload, a.tag, a.payload),
        JitValue::bool_val(false)
    );
}

/// `elle_jit_pop` MOVES the popped element out to the caller, mirroring
/// `handle_intr_pop` in src/vm/types.rs (both route through
/// `arena::pop_with_decref`). The move releases the container's stored
/// reference (`decref_removed_element`, undoing the push's
/// `incref_inserted_element`) AND holds the caller's owning reference (the
/// pass-through retain), taken BEFORE the release so a sole-owned element's
/// region is never freed under the returned Value. Net: the source region's RC
/// stays at the post-push state (baseline + 1), held now by the returned value
/// rather than the container; it returns to baseline only when the caller
/// releases the moved-out result. Dropping the RC to baseline HERE (the pre-move
/// destroy semantics) would free the element under the returned Value — the
/// free-before-retain UAF the `raw-pop` oracle probe pins.
#[test]
fn pop_track_removes_cross_region_value() {
    use crate::value::arena::{alloc_in_fresh_region, push_with_incref, region_rc};
    use crate::value::heap::{HeapObject, Pair};
    crate::value::arena::with_test_region(|| {
        // The JIT pop helper reaches its heap through a `JitCtx` over a VM, so the
        // @array, the cross-region value, the push, and the pop must all live on
        // that VM's heap. Build the VM first and thread `vm.heap_ptr` through
        // every heap op (the heap is named explicitly via the ctx).
        let mut vm = crate::vm::VM::new();
        let heap_ptr = vm.heap_ptr;
        let arr_region = unsafe { (*heap_ptr).new_runtime_region() };
        let arr = crate::primitives::ctx::NativeCtx::with_region_vm(
            arr_region,
            unsafe { &mut *heap_ptr },
            &mut vm as *mut crate::vm::VM,
        )
        .array_mut(vec![]);
        let (cross, source_rid) = alloc_in_fresh_region(
            unsafe { &mut *heap_ptr },
            HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)),
        );
        let rc_baseline = region_rc(unsafe { &*heap_ptr }, source_rid);
        // Push goes through push_with_incref, which calls incref_inserted_element →
        // source region RC is now baseline + 1.
        let _ = push_with_incref(unsafe { &mut *heap_ptr }, arr, cross);
        let rc_after_push = region_rc(unsafe { &*heap_ptr }, source_rid);
        assert_eq!(
            rc_after_push,
            rc_baseline + 1,
            "precondition: push_with_incref must bump source region RC"
        );
        // Pop via the JIT helper. The move releases the container's reference and
        // holds the caller's, so the source region's RC is UNCHANGED from the
        // post-push state (baseline + 1) — held now by the returned value.
        let mut jit_ctx = crate::jit::JitCtx::new(&mut vm as *mut crate::vm::VM);
        let popped = elle_jit_pop(
            arr.tag,
            arr.payload,
            &mut jit_ctx as *mut crate::jit::JitCtx,
        );
        let popped_val = Value {
            tag: popped.tag,
            payload: popped.payload,
        };
        assert_eq!(
            (popped_val.tag, popped_val.payload),
            (cross.tag, cross.payload),
            "elle_jit_pop must return the popped Value"
        );
        let rc_after_pop = region_rc(unsafe { &*heap_ptr }, source_rid);
        assert_eq!(
            rc_after_pop,
            rc_baseline + 1,
            "elle_jit_pop MOVES the element out — its region survives the pop, held by the returned value (not dropped to baseline)"
        );
        // The caller releasing the moved-out result (its `DecrefValueRegion` at
        // the result's decref_point — the whole caller side, since dispatch skips
        // its own pass-through retain for the `moves_out` `%pop`/`pop`) completes
        // the move, returning the source region's RC to baseline.
        crate::value::arena::decref_region(unsafe { &mut *heap_ptr }, Some(source_rid));
        let rc_after_release = region_rc(unsafe { &*heap_ptr }, source_rid);
        assert_eq!(
            rc_after_release, rc_baseline,
            "releasing the moved-out result returns the source region RC to baseline"
        );
    });
}
