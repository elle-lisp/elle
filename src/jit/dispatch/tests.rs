//! Unit tests (`super` is the parent impl module).

use super::*;
use crate::jit::value::JitValue;
use crate::vm::VM;

/// Regression: elle_jit_array_push must MUTATE the input @array in place
/// and return the same Value, matching the VM's handle_array_push contract
/// in `src/vm/data.rs` (`push_with_incref`). The earlier implementation cloned
/// the contents and returned a freshly-allocated @array, which (a) gave
/// the user-visible wrong semantics for `(push @arr x)` and (b) skipped
/// the cross-region RC accounting (`incref_inserted_element`), letting the source
/// region of inserted heap values be freed while the @array still held
/// dangling pointers — eventually corrupting the C heap. Counterfactual
/// for the corruption in tests/elle/jit-double-import-uaf.lisp.
#[test]
fn array_push_mutates_in_place_and_returns_same_value() {
    crate::value::arena::with_test_region(|| {
        use crate::primitives::register_primitives;
        use crate::symbol::SymbolTable;

        let mut symbols = SymbolTable::new();
        let mut vm = VM::new();
        let _signals = register_primitives(&mut vm, &mut symbols);

        let h = crate::primitives::ctx::TestHeap::new();
        let arr = h.ctx().array_mut(vec![]);
        let v = Value::int(42);
        let ret = elle_jit_array_push(
            arr.tag,
            arr.payload,
            v.tag,
            v.payload,
            &mut vm as *mut VM as *mut (),
        );
        let ret_val = Value {
            tag: ret.tag,
            payload: ret.payload,
        };
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

/// elle_jit_array_push must bump the source region's RC when a heap
/// Value is inserted into an @array that lives in a different region.
/// This is what keeps the source region alive across the insertion;
/// without it the source region can drop to RC=0 and be freed while
/// the @array still references it, producing the heap corruption that
/// tests/elle/jit-double-import-uaf.lisp reproduced.
#[test]
fn array_push_track_inserts_cross_region_value() {
    use crate::value::arena::{alloc_in_fresh_region, region_rc};
    use crate::value::heap::{HeapObject, Pair};
    crate::value::arena::with_test_region(|| {
        use crate::primitives::register_primitives;
        use crate::symbol::SymbolTable;

        let mut symbols = SymbolTable::new();
        let mut vm = VM::new();
        let _signals = register_primitives(&mut vm, &mut symbols);

        // The JIT push helper operates on the VM's heap, so the @array and the
        // cross-region value must live there too (the heap is named explicitly
        // through the ctx).
        let heap_ptr = vm.heap_ptr;
        let arr_region = unsafe { (*heap_ptr).new_runtime_region() };
        let arr = crate::primitives::ctx::NativeCtx::with_region_vm(
            arr_region,
            unsafe { &mut *heap_ptr },
            &mut vm as *mut VM,
        )
        .array_mut(vec![]);
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
            &mut vm as *mut VM as *mut (),
        );
        let rc_after = region_rc(unsafe { &*heap_ptr }, source_rid);
        assert_eq!(
            rc_after,
            rc_before + 1,
            "elle_jit_array_push must incref the source region of an inserted cross-region value"
        );
    });
}

/// elle_jit_push (the IntrPush intrinsic helper) shares the same
/// contract: @array mutate-in-place plus incref_inserted_element for cross-region
/// values. Mirrors elle_jit_array_push's tests.
#[test]
fn intr_push_track_inserts_cross_region_value() {
    use crate::jit::runtime::elle_jit_push;
    use crate::value::arena::{alloc_in_fresh_region, region_rc};
    use crate::value::heap::{HeapObject, Pair};
    crate::value::arena::with_test_region(|| {
        use crate::primitives::register_primitives;
        use crate::symbol::SymbolTable;

        let mut symbols = SymbolTable::new();
        let mut vm = VM::new();
        let _signals = register_primitives(&mut vm, &mut symbols);

        // The JIT push helper reaches its heap through the threaded JitCtx (VM),
        // so the @array and the cross-region value must live on the VM's heap.
        let heap_ptr = vm.heap_ptr;
        let arr_region = unsafe { (*heap_ptr).new_runtime_region() };
        let arr = crate::primitives::ctx::NativeCtx::with_region_vm(
            arr_region,
            unsafe { &mut *heap_ptr },
            &mut vm as *mut VM,
        )
        .array_mut(vec![]);
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
        let ret_val = Value {
            tag: ret.tag,
            payload: ret.payload,
        };
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

#[test]
fn test_has_exception() {
    crate::value::arena::with_test_region(|| {
        use crate::primitives::register_primitives;
        use crate::symbol::SymbolTable;

        let mut symbols = SymbolTable::new();
        let mut vm = VM::new();
        let _signals = register_primitives(&mut vm, &mut symbols);

        // Initially no exception
        let result = elle_jit_has_exception(&mut vm as *mut VM as *mut () as u64);
        assert_eq!(result, JitValue::bool_val(false));

        // Set an error signal
        let err = vm.escaping_error("division-by-zero", "test");
        vm.fiber.signal = Some((crate::value::SIG_ERROR, err));

        // Now should return true
        let result = elle_jit_has_exception(&mut vm as *mut VM as *mut () as u64);
        assert_eq!(result, JitValue::bool_val(true));

        // Clear signal
        vm.fiber.signal = None;

        // Should return false again
        let result = elle_jit_has_exception(&mut vm as *mut VM as *mut () as u64);
        assert_eq!(result, JitValue::bool_val(false));
    });
}

/// E4 region-coherence: `elle_jit_freeze` must allocate its fresh immutable
/// copy into the SLOT region the emitter threads to it (the region the
/// matching `DecrefRegion(slot)` frees). The counterfactual is exact: build
/// the source @array in a DISTINCT region `source`, thread a different region
/// `target` as the `region` argument, and assert the result lands in
/// `target`. A helper that ignored its region argument would land the copy in
/// `source` (or a fresh region) — the `DecrefRegion(target)` would then free
/// an empty slot. Pins that the region is threaded.
#[test]
fn freeze_allocates_into_threaded_region() {
    use crate::jit::runtime::elle_jit_freeze;
    // freeze sources its VM (and the heap, via vm.heap_ptr) from the threaded
    // JitCtx, so build a real VM and a ctx over it.
    let mut vm = VM::new();
    let heap_ptr = vm.heap_ptr;
    let source = unsafe { (*heap_ptr).new_runtime_region() };
    let target = unsafe { (*heap_ptr).new_runtime_region() };
    assert_ne!(source, target);
    // A mutable @array born in `source`; freeze copies it into `target`.
    // Region is load-bearing (the test asserts the result lands in `target`,
    // distinct from `source`), so allocate `arr` into `source` explicitly via a
    // NativeCtx over this VM rather than dropping the region.
    let arr = crate::primitives::ctx::NativeCtx::with_region_vm(
        source,
        unsafe { &mut *heap_ptr },
        &mut vm as *mut VM,
    )
    .array_mut(vec![Value::int(1), Value::int(2)]);
    let mut jit_ctx = crate::jit::JitCtx::new(&mut vm as *mut VM);
    let jv = elle_jit_freeze(arr.tag, arr.payload, target.get(), &mut jit_ctx as *mut _);
    let result = Value {
        tag: jv.tag,
        payload: jv.payload,
    };
    assert!(result.is_array(), "freeze yields an immutable array");
    assert_eq!(
        crate::value::arena::region_of(unsafe { &mut *heap_ptr }, result),
        Some(target),
        "elle_jit_freeze must allocate into the threaded SLOT region",
    );
    unsafe {
        (*heap_ptr).decref_region_if_present(source);
        (*heap_ptr).decref_region_if_present(target);
    }
}

/// E4 region-coherence: `elle_jit_push` on an *immutable* array yields a
/// FRESH copy born in a freshly-minted call-result region
/// (`run_alloc_intrinsic`) — matching `elle_jit_put`/`del` and the
/// `produces_call_result_region` model the compiler uses for %array-push (the
/// result is freed by a value-based `DecrefValueRegion`). The counterfactual:
/// build the source array in a distinct region `source`; assert the copy's
/// region is NOT `source` (it is the fresh minted region) and contents match.
#[test]
fn push_immutable_result_is_fresh_region() {
    use crate::jit::runtime::elle_jit_push;
    let mut vm = VM::new();
    let heap_ptr = vm.heap_ptr;
    let source = unsafe { (*heap_ptr).new_runtime_region() };
    // Region is load-bearing (the test asserts the immutable-push copy lands in
    // a fresh region, NOT `source`), so build `arr` into `source` explicitly.
    let arr = crate::primitives::ctx::NativeCtx::with_region_vm(
        source,
        unsafe { &mut *heap_ptr },
        &mut vm as *mut VM,
    )
    .array(vec![Value::int(1), Value::int(2)]);
    let mut jit_ctx = crate::jit::JitCtx::new(&mut vm as *mut VM);
    let jv = elle_jit_push(
        arr.tag,
        arr.payload,
        Value::int(3).tag,
        Value::int(3).payload,
        &mut jit_ctx as *mut _,
    );
    let result = Value {
        tag: jv.tag,
        payload: jv.payload,
    };
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
    let region = crate::value::arena::region_of(unsafe { &mut *heap_ptr }, result)
        .expect("array has a region");
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

/// E4 region-coherence twin of the freeze pin: `elle_jit_thaw` must allocate
/// its fresh mutable copy into the threaded SLOT region.
#[test]
fn thaw_allocates_into_threaded_region() {
    use crate::jit::runtime::elle_jit_thaw;
    let mut vm = VM::new();
    let heap_ptr = vm.heap_ptr;
    let source = unsafe { (*heap_ptr).new_runtime_region() };
    let target = unsafe { (*heap_ptr).new_runtime_region() };
    assert_ne!(source, target);
    // An immutable array born in `source`; thaw copies it into `target`.
    // Region is load-bearing (the test asserts the result lands in `target`), so
    // allocate `arr` into `source` explicitly via a NativeCtx over this VM.
    let arr = crate::primitives::ctx::NativeCtx::with_region_vm(
        source,
        unsafe { &mut *heap_ptr },
        &mut vm as *mut VM,
    )
    .array(vec![Value::int(1), Value::int(2)]);
    let mut jit_ctx = crate::jit::JitCtx::new(&mut vm as *mut VM);
    let jv = elle_jit_thaw(arr.tag, arr.payload, target.get(), &mut jit_ctx as *mut _);
    let result = Value {
        tag: jv.tag,
        payload: jv.payload,
    };
    assert!(result.is_array_mut(), "thaw yields a mutable @array");
    assert_eq!(
        crate::value::arena::region_of(unsafe { &mut *heap_ptr }, result),
        Some(target),
        "elle_jit_thaw must allocate into the threaded SLOT region",
    );
    unsafe {
        (*heap_ptr).decref_region_if_present(source);
        (*heap_ptr).decref_region_if_present(target);
    }
}

/// `elle_jit_adopt_region` mirrors the interpreter's `handle_adopt_region`: it
/// resolves the parent and child Values to their runtime regions and moves the
/// child `Counted → Owned`, freezing its RC, so the parent's later subtree drop
/// reclaims both. Pins both halves: the child's count is *consumed* (region_rc
/// reads 0, an Owned region has no count), and dropping the parent reclaims the
/// parent + the adopted child as one subtree. A no-op (broken) helper would leave
/// the child `Counted(1)` and free only the parent — both assertions catch it.
#[test]
fn adopt_region_freezes_child_and_subtree_drops_with_parent() {
    use crate::value::arena::{alloc_in_fresh_region, region_rc};
    use crate::value::heap::{HeapObject, Pair};
    let mut vm = VM::new();
    let heap_ptr = vm.heap_ptr;
    // Parent and child, each a pair in its own fresh region.
    let (parent, parent_rid) = alloc_in_fresh_region(
        unsafe { &mut *heap_ptr },
        HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)),
    );
    let (child, child_rid) = alloc_in_fresh_region(
        unsafe { &mut *heap_ptr },
        HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)),
    );
    assert_ne!(parent_rid, child_rid);
    assert_eq!(
        region_rc(unsafe { &*heap_ptr }, child_rid),
        1,
        "fresh child region starts Counted(1)"
    );
    let before = unsafe { &*heap_ptr }.active_region_count();

    elle_jit_adopt_region(
        parent.tag,
        parent.payload,
        child.tag,
        child.payload,
        &mut vm as *mut VM as *mut (),
    );

    assert_eq!(
        region_rc(unsafe { &*heap_ptr }, child_rid),
        0,
        "adopt moves the child Counted -> Owned, consuming its count (an Owned \
         region has no RC); a no-op helper would leave it at 1",
    );
    assert_eq!(
        region_rc(unsafe { &*heap_ptr }, parent_rid),
        1,
        "the parent stays Counted(1) — only the child is frozen"
    );

    // Dropping the parent (rc 1 -> 0) subtree-drops the adopted child with it.
    unsafe { (*heap_ptr).decref_region(parent_rid) };
    let after = unsafe { &*heap_ptr }.active_region_count();
    assert_eq!(
        before - after,
        2,
        "the parent's subtree drop must reclaim parent + adopted child (2 regions); \
         a failed adopt would free only the parent (delta 1)",
    );
}

/// `elle_jit_adopt_into_activation` mirrors the interpreter's
/// `handle_adopt_into_activation`: it resolves the child Value to its runtime
/// region, lazily mints the current activation's pages-less owner node, and
/// moves the child `Counted → Owned` (count consumed). Releasing the node
/// (`VM::release_activation_dues`, the completion free both tiers share)
/// then subtree-drops node + member as one unit. A no-op (broken) helper would
/// leave the child `Counted(1)` and the release would reclaim nothing — both
/// assertions catch it.
#[test]
fn adopt_into_activation_adopts_into_lazily_minted_node() {
    use crate::value::arena::{alloc_in_fresh_region, region_rc};
    use crate::value::heap::{HeapObject, Pair};
    let mut vm = VM::new();
    let heap_ptr = vm.heap_ptr;
    let (child, child_rid) = alloc_in_fresh_region(
        unsafe { &mut *heap_ptr },
        HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)),
    );
    assert_eq!(
        region_rc(unsafe { &*heap_ptr }, child_rid),
        1,
        "fresh child region starts Counted(1)"
    );

    elle_jit_adopt_into_activation(child.tag, child.payload, &mut vm as *mut VM as *mut ());

    assert_eq!(
        region_rc(unsafe { &*heap_ptr }, child_rid),
        0,
        "the adopt moves the child Counted -> Owned, consuming its count; a no-op \
         helper would leave it at 1",
    );

    // The node was lazily minted into the current (base) activation slot;
    // releasing it reclaims node + adopted member as one subtree.
    let before = unsafe { &*heap_ptr }.active_region_count();
    vm.release_activation_dues();
    let after = unsafe { &*heap_ptr }.active_region_count();
    assert_eq!(
        before - after,
        2,
        "releasing the owner node must reclaim the node's entry + the adopted \
         member (2 regions); a failed adopt would reclaim only the node (delta 1)",
    );
}

/// An immediate child (no region) must adopt nothing AND mint no node — the
/// lazy mint fires only for a real member, so an activation whose adopts all
/// resolve to immediates pays nothing and its completion release is a no-op.
#[test]
fn adopt_into_activation_immediate_child_mints_no_node() {
    let mut vm = VM::new();
    let heap_ptr = vm.heap_ptr;
    let before = unsafe { &*heap_ptr }.active_region_count();
    let n = Value::int(42);
    elle_jit_adopt_into_activation(n.tag, n.payload, &mut vm as *mut VM as *mut ());
    assert!(
        vm.take_activation_dues().is_empty(),
        "an immediate child must not mint an owner node"
    );
    assert_eq!(
        unsafe { &*heap_ptr }.active_region_count(),
        before,
        "no region state may change for an immediate child"
    );
}

/// `elle_jit_free_region_group` mirrors the interpreter's `handle_free_region_group`:
/// it resolves each member Value to its runtime region and frees the whole set as
/// one unit, regardless of the members' reference counts. Pins that both members'
/// regions are reclaimed (active region count falls by 2). The cycle-reclamation /
/// cascade semantics of the underlying `free_region_group` are pinned in
/// `regionstore::tests::free_region_group_reclaims_bare_cycle`; this pins the JIT
/// helper's resolve-and-forward.
#[test]
fn free_region_group_reclaims_member_regions() {
    use crate::value::arena::alloc_in_fresh_region;
    use crate::value::heap::{HeapObject, Pair};
    let mut vm = VM::new();
    let heap_ptr = vm.heap_ptr;
    let (a, _a_rid) = alloc_in_fresh_region(
        unsafe { &mut *heap_ptr },
        HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)),
    );
    let (b, _b_rid) = alloc_in_fresh_region(
        unsafe { &mut *heap_ptr },
        HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)),
    );
    let before = unsafe { &*heap_ptr }.active_region_count();

    // The compiled translate arm spills members to a contiguous stack slot as
    // Value pairs; a Rust array of Values is the same layout.
    let members = [a, b];
    elle_jit_free_region_group(members.as_ptr(), 2, &mut vm as *mut VM as *mut ());

    let after = unsafe { &*heap_ptr }.active_region_count();
    assert_eq!(
        before - after,
        2,
        "free_region_group must reclaim both members' regions as one unit",
    );
}

/// Each JIT intrinsic fast-path helper resolves its driving VM from the threaded
/// `JitCtx` handed up from compiled code — there is no process-shared VM slot to
/// consult. Covers all three VM-resolution paths — `run_alloc_intrinsic` (`put`),
/// `boundary_vm` (`has`), and `with_region_vm` (`freeze`).
#[test]
fn jit_intrinsics_use_threaded_vm() {
    use crate::jit::runtime::{elle_jit_freeze, elle_jit_has, elle_jit_put};
    let mut vm = VM::new();
    let heap_ptr = vm.heap_ptr;
    let region = unsafe { (*heap_ptr).new_runtime_region() };
    let mut jit_ctx = crate::jit::JitCtx::new(&mut vm as *mut VM);
    let ctx_ptr = &mut jit_ctx as *mut _;

    // put: an immutable struct gains a key, the fresh copy born via
    // run_alloc_intrinsic off the threaded VM. The source struct/@array are built
    // into `region` (the test's working region) via a NativeCtx over this VM.
    let empty = crate::primitives::ctx::NativeCtx::with_region_vm(
        region,
        unsafe { &mut *heap_ptr },
        &mut vm as *mut VM,
    )
    .struct_from_sorted(vec![]);
    let key = Value::keyword("k");
    let one = Value::int(1);
    let put = elle_jit_put(
        empty.tag,
        empty.payload,
        key.tag,
        key.payload,
        one.tag,
        one.payload,
        ctx_ptr,
    );
    let put_val = Value {
        tag: put.tag,
        payload: put.payload,
    };
    assert!(put_val.is_struct(), "put yields an immutable struct");

    // has: queries the just-built struct (boundary_vm path).
    let has = elle_jit_has(put_val.tag, put_val.payload, key.tag, key.payload, ctx_ptr);
    assert_eq!(has, JitValue::bool_val(true), "has? finds the inserted key");

    // freeze: with_region_vm path, copies a mutable @array into `region`.
    let arr = crate::primitives::ctx::NativeCtx::with_region_vm(
        region,
        unsafe { &mut *heap_ptr },
        &mut vm as *mut VM,
    )
    .array_mut(vec![Value::int(1)]);
    let fr = elle_jit_freeze(arr.tag, arr.payload, region.get(), ctx_ptr);
    let fr_val = Value {
        tag: fr.tag,
        payload: fr.payload,
    };
    assert!(fr_val.is_array(), "freeze yields an immutable array");

    unsafe {
        (*heap_ptr).decref_region_if_present(region);
    }
}

/// The compiled error exit's ABI: `elle_jit_release_abandoned_frame` reads the
/// value routes out of the locals the exit spilled, the slot routes out of the
/// activation map the prologue pushed, and the payload off `fiber.signal`
/// (docs/impl/region/mechanism.md § "An abandoned frame runs the releases it
/// still owes").
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
        let vm_ptr = &mut vm as *mut VM as *mut ();

        // The compiled prologue: this activation's region-remap frame.
        crate::jit::dispatch::elle_jit_push_region_map(vm_ptr);

        // A slot-routed alloc: the mint records slot → physical, which is the
        // release's receipt.
        let region_slot = StaticRegion::new(5).expect("nonzero slot");
        let mapped = crate::hir::region::RuntimeRegion::new(
            crate::jit::dispatch::elle_jit_resolve_alloc_region(vm_ptr, region_slot.get()),
        )
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
