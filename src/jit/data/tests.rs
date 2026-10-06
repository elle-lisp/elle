// audited: 2026-10-06
//! Unit tests for the JIT data helpers: cons, arrays, capture cells, and the
//! prologue's own-region env values.

use super::*;

/// Mint a fresh region on `heap` — the explicit region the JIT data helpers
/// take. The region MUST be on the same heap the helper allocates into (the
/// driving VM's `heap_ptr`), so callers pass `vm.heap_ptr` here and that same VM
/// to the helper.
fn fresh(heap: *mut crate::value::fiberheap::FiberHeap) -> u32 {
    unsafe { (*heap).new_runtime_region().get() }
}

#[test]
fn test_cons_car_cdr() {
    let mut vm = crate::vm::VM::new();
    let heap = vm.heap_ptr;
    let vm_ptr = &mut vm as *mut crate::vm::VM as *mut ();
    let head = Value::int(1);
    let tail = Value::int(2);
    let pair = elle_jit_pair(
        head.tag,
        head.payload,
        tail.tag,
        tail.payload,
        fresh(heap),
        vm_ptr,
    )
    .to_value();

    let car_val = elle_jit_first(pair.tag, pair.payload).to_value();
    let cdr_val = elle_jit_rest(pair.tag, pair.payload).to_value();

    assert_eq!(car_val.as_int(), Some(1));
    assert_eq!(cdr_val.as_int(), Some(2));
}

#[test]
fn test_is_pair() {
    let mut vm = crate::vm::VM::new();
    let heap = vm.heap_ptr;
    let vm_ptr = &mut vm as *mut crate::vm::VM as *mut ();
    let head = Value::int(1);
    let tail = Value::int(2);
    let pair = elle_jit_pair(
        head.tag,
        head.payload,
        tail.tag,
        tail.payload,
        fresh(heap),
        vm_ptr,
    )
    .to_value();

    assert_eq!(
        elle_jit_is_pair(pair.tag, pair.payload),
        JitValue::bool_val(true)
    );
    assert_eq!(
        elle_jit_is_pair(Value::int(42).tag, Value::int(42).payload),
        JitValue::bool_val(false)
    );
}

#[test]
fn test_make_array() {
    let mut vm = crate::vm::VM::new();
    let heap = vm.heap_ptr;
    let vm_ptr = &mut vm as *mut crate::vm::VM as *mut ();
    let elements = [Value::int(1), Value::int(2), Value::int(3)];
    let vec_val = elle_jit_make_array(elements.as_ptr(), 3, fresh(heap), vm_ptr).to_value();

    assert!(vec_val.is_array_mut());
    let vec_ref = vec_val.as_array_mut().unwrap();
    let borrowed = vec_ref.borrow();
    assert_eq!(borrowed.len(), 3);
    assert_eq!(borrowed[0].as_int(), Some(1));
    assert_eq!(borrowed[1].as_int(), Some(2));
    assert_eq!(borrowed[2].as_int(), Some(3));
}

#[test]
fn test_cell_operations() {
    let mut vm = crate::vm::VM::new();
    let heap = vm.heap_ptr;
    let vm_ptr = &mut vm as *mut crate::vm::VM as *mut ();
    let v = Value::int(42);
    let cell = elle_jit_make_capture(
        v.tag,
        v.payload,
        fresh(heap),
        crate::value::SymbolId::of("jit-cell").0,
        0,
        vm_ptr,
    )
    .to_value();
    assert!(cell.is_capture_cell());

    let loaded = elle_jit_load_capture_cell(cell.tag, cell.payload).to_value();
    assert_eq!(loaded.as_int(), Some(42));

    let new_val = Value::int(100);
    elle_jit_store_capture_cell(cell.tag, cell.payload, new_val.tag, new_val.payload, vm_ptr);

    let loaded2 = elle_jit_load_capture_cell(cell.tag, cell.payload).to_value();
    assert_eq!(loaded2.as_int(), Some(100));
}

// ── wall E: JIT-prologue env values must be born in their OWN region ──
//
// A JIT-compiled function's prologue builds env values — capture cells (a
// mutable-captured param/local) and the variadic rest cons-list — that the
// interpreter's `populate_env` mints FRESH regions for
// (`env_value_region`, src/vm/env.rs; `args_to_list`, src/vm/env/rest.rs).
// The prologue must do the
// same. On a JIT->JIT call the callee inherits the caller's region; an env value
// allocated into the *caller's* region commingles with it
// (docs/impl/region/rules.md Rule 6) and its value-based `DecrefCellRegion` /
// `DecrefValueRegion` decrefs the caller's region — a leak (Rule 8) and a latent
// use-after-free. The owned helpers (`elle_jit_make_capture_owned` /
// `elle_jit_collect_rest_list`) mint regions of their own, exactly like the
// interpreter: one per value, or one for a rest list whose layout says so
// (docs/impl/region/restlist.md). These counterfactuals pin that.

/// Build a VM and mint a live "caller" region on its heap, then run `f` with both.
/// The region stands in for a JIT caller's region that the owned env helpers must
/// NOT allocate into (they mint their own). Returns `f`'s result.
fn with_caller_region<R>(
    f: impl FnOnce(&mut crate::vm::VM, crate::hir::region::RuntimeRegion) -> R,
) -> R {
    let mut vm = crate::vm::VM::new();
    let heap = vm.heap_ptr;
    let other = unsafe { (*heap).new_runtime_region() };
    // Keep the region live (rc>0) like a real caller's region, so it is never the
    // recycled id a fresh mint would hand back.
    unsafe { (*heap).incref_region(other) };
    f(&mut vm, other)
}

/// SPEC (Rule 6): a prologue capture cell is born in its OWN fresh region, never
/// the caller's region. RED against `elle_jit_make_capture` (caller's region),
/// GREEN against `elle_jit_make_capture_owned`.
#[test]
fn prologue_capture_cell_gets_its_own_region_not_callers() {
    with_caller_region(|vm, caller_region| {
        let heap = vm.heap_ptr;
        let vm_ptr = vm as *mut crate::vm::VM as *mut ();
        let v = Value::int(42);
        let cell = elle_jit_make_capture_owned(v.tag, v.payload, vm_ptr).to_value();
        assert!(cell.is_capture_cell());
        let region = crate::value::arena::region_of(unsafe { &*heap }, cell)
            .expect("a heap-allocated capture cell must have a region");
        assert_ne!(
            region, caller_region,
            "JIT-prologue capture cell commingled into the caller's region (Rule 6) \
             — it must mint its own per-value region like populate_env"
        );
        // The wrapped value is reachable (alloc_obj scanned + increfed it).
        let loaded = elle_jit_load_capture_cell(cell.tag, cell.payload).to_value();
        assert_eq!(loaded.as_int(), Some(42));
    });
}

/// SPEC (Rule 6 + args_to_list): each rest-list cons is born in its OWN fresh
/// region, none in the caller's region; the list reads back correctly. RED
/// against an `elle_jit_pair` cons-loop (caller's region), GREEN against
/// `elle_jit_collect_rest_list`.
#[test]
fn prologue_rest_list_conses_get_own_regions_not_callers() {
    with_caller_region(|vm, caller_region| {
        let heap = vm.heap_ptr;
        let vm_ptr = vm as *mut crate::vm::VM as *mut ();
        // args = [1, 2, 3]; build the rest list from index 0.
        let args = [Value::int(1), Value::int(2), Value::int(3)];
        let head = elle_jit_collect_rest_list(
            args.as_ptr(),
            0,
            3,
            crate::value::RestListLayout::PerCell as u32,
            vm_ptr,
        )
        .to_value();

        // Walk the list: every cons must be off the caller's region, in a region
        // no other cons shares, and the elements must read back in order.
        let regions = walk_rest_list(heap, head, caller_region);
        let mut distinct = regions.clone();
        distinct.sort_by_key(|r| r.get());
        distinct.dedup();
        assert_eq!(
            distinct.len(),
            3,
            "the per-cell layout builds each cons into a region of its own: {regions:?}"
        );
    });
}

/// Walk a rest list built from `[1, 2, …]`: check each element and that no cons
/// sits in `caller_region`, and answer each cons's region, head first.
fn walk_rest_list(
    heap: *mut crate::value::fiberheap::FiberHeap,
    head: Value,
    caller_region: crate::hir::region::RuntimeRegion,
) -> Vec<crate::hir::region::RuntimeRegion> {
    let mut regions = Vec::new();
    let mut cur = head;
    while cur.as_pair().is_some() {
        let region = crate::value::arena::region_of(unsafe { &*heap }, cur)
            .expect("a heap cons must have a region");
        assert_ne!(
            region, caller_region,
            "JIT-prologue rest cons commingled into the caller's region (Rule 6) \
             — the list must mint its own region like args_to_list"
        );
        let car = elle_jit_first(cur.tag, cur.payload).to_value();
        assert_eq!(car.as_int(), Some(regions.len() as i64 + 1));
        cur = elle_jit_rest(cur.tag, cur.payload).to_value();
        regions.push(region);
    }
    assert_eq!(regions.len(), 3, "rest list must have all 3 elements");
    assert!(
        cur.is_empty_list(),
        "rest list must terminate in empty-list"
    );
    regions
}

/// SPEC (docs/impl/region/restlist.md): under the one-region layout every cons
/// shares one region of the list's own, off the caller's region, and that
/// region holds one reference, the head's.
#[test]
fn prologue_rest_list_one_region_layout_shares_one_region() {
    with_caller_region(|vm, caller_region| {
        let heap = vm.heap_ptr;
        let vm_ptr = vm as *mut crate::vm::VM as *mut ();
        let args = [Value::int(1), Value::int(2), Value::int(3)];
        let head = elle_jit_collect_rest_list(
            args.as_ptr(),
            0,
            3,
            crate::value::RestListLayout::OneRegion as u32,
            vm_ptr,
        )
        .to_value();
        let regions = walk_rest_list(heap, head, caller_region);
        assert!(
            regions.iter().all(|r| *r == regions[0]),
            "every cons shares the head's region: {regions:?}"
        );
        assert_eq!(
            unsafe { (*heap).region_rc(regions[0]) },
            1,
            "the list's region holds one reference, the head's"
        );
    });
}

/// An empty rest list (no varargs) is the empty list — no allocation.
#[test]
fn prologue_rest_list_empty_is_empty_list() {
    with_caller_region(|vm, _caller_region| {
        let vm_ptr = vm as *mut crate::vm::VM as *mut ();
        let args = [Value::int(1)];
        // start == nargs → nothing to collect.
        let head = elle_jit_collect_rest_list(
            args.as_ptr(),
            1,
            1,
            crate::value::RestListLayout::OneRegion as u32,
            vm_ptr,
        )
        .to_value();
        assert!(head.is_empty_list());
    });
}
