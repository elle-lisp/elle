// audited: 2026-09-29
//! Unit tests for the JIT data helpers: cons, arrays, capture cells, and the
//! env values a compiled function's entry builds.
//!
//! docs/impl/region/colocation.md

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

// ── the prologue's env values never land in the caller's region ──
//
// A compiled function's JIT entry builds env values: capture cells for a
// captured mutable parameter, and the rest list. On a JIT->JIT call the callee
// runs in the caller's region, so a helper that allocated there would release
// the caller's region with the env value. The owned helpers mint through
// `new_value_region`, as the interpreter's `env_value_region` does, and these
// tests pin that no env value lands in the caller's region.

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

/// A prologue capture cell never lands in the caller's region. RED against
/// `elle_jit_make_capture`, which allocates into the region it is handed; GREEN
/// against `elle_jit_make_capture_owned`.
#[test]
fn prologue_capture_cell_is_not_in_the_callers_region() {
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
             — it must mint through new_value_region like populate_env"
        );
        // The wrapped value is reachable (alloc_obj scanned + increfed it).
        let loaded = elle_jit_load_capture_cell(cell.tag, cell.payload).to_value();
        assert_eq!(loaded.as_int(), Some(42));
    });
}

/// No rest-list cons lands in the caller's region, and the list reads back in
/// order. RED against an `elle_jit_pair` cons-loop, which allocates into the
/// caller's region; GREEN against `elle_jit_collect_rest_list`.
#[test]
fn prologue_rest_list_is_not_in_the_callers_region() {
    with_caller_region(|vm, caller_region| {
        let heap = vm.heap_ptr;
        let vm_ptr = vm as *mut crate::vm::VM as *mut ();
        // args = [1, 2, 3]; build the rest list from index 0.
        let args = [Value::int(1), Value::int(2), Value::int(3)];
        let head = elle_jit_collect_rest_list(args.as_ptr(), 0, 3, vm_ptr).to_value();

        // Walk the list: every cons must be off the caller's region, and the
        // elements must read back in order.
        let mut cur = head;
        let mut seen = 0;
        while cur.as_pair().is_some() {
            let region = crate::value::arena::region_of(unsafe { &*heap }, cur)
                .expect("a heap cons must have a region");
            assert_ne!(
                region, caller_region,
                "JIT-prologue rest cons commingled into the caller's region (Rule 6) \
                 — the list must mint through new_value_region like args_to_list"
            );
            let car = elle_jit_first(cur.tag, cur.payload).to_value();
            assert_eq!(car.as_int(), Some(seen + 1));
            cur = elle_jit_rest(cur.tag, cur.payload).to_value();
            seen += 1;
        }
        assert_eq!(seen, 3, "rest list must have all 3 elements");
        assert!(
            cur.is_empty_list(),
            "rest list must terminate in empty-list"
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
        let head = elle_jit_collect_rest_list(args.as_ptr(), 1, 1, vm_ptr).to_value();
        assert!(head.is_empty_list());
    });
}

// ── colocation: the prologue's env values share the interpreter's regions ──
//
// The interpreter builds a rest list in one region and mints every env value
// through `new_value_region`, which joins an open macro scope's arena
// (docs/impl/region/colocation.md). A compiled callee builds the same values in
// its JIT entry, so the entry's helpers must place them exactly as the
// interpreter does, or the two tiers keep different footprints for one program.

/// A rest list is one region: every cons shares the head's region, and that
/// region holds the one reference the list's release gives back. The
/// counter-factual: a region per cons, chained head → tail, also reads back in
/// order and frees on the head's release, so only the region check sees it.
#[test]
fn prologue_rest_list_is_one_region() {
    with_caller_region(|vm, caller_region| {
        let heap = vm.heap_ptr;
        let vm_ptr = vm as *mut crate::vm::VM as *mut ();
        let args = [Value::int(1), Value::int(2), Value::int(3), Value::int(4)];
        let head = elle_jit_collect_rest_list(args.as_ptr(), 0, 4, vm_ptr).to_value();
        let region = crate::value::arena::region_of(unsafe { &*heap }, head)
            .expect("a heap cons must have a region");
        assert_ne!(region, caller_region);

        let mut cur = head;
        let mut seen = 0;
        while cur.as_pair().is_some() {
            assert_eq!(
                crate::value::arena::region_of(unsafe { &*heap }, cur),
                Some(region),
                "cons {seen} of the rest list is in a region of its own",
            );
            cur = elle_jit_rest(cur.tag, cur.payload).to_value();
            seen += 1;
        }
        assert_eq!(seen, 4);
        assert_eq!(
            crate::value::arena::region_rc(unsafe { &*heap }, region),
            1,
            "the list's region holds one reference, the one its release gives back",
        );
    });
}

/// Inside a macro scope, the entry's env values join the scope's arena, as the
/// interpreter's `env_value_region` does: the owned capture cell and the rest
/// list alike. The close then frees them with the arena.
#[test]
fn prologue_env_values_join_an_open_macro_scope_arena() {
    let mut vm = crate::vm::VM::new();
    let heap = vm.heap_ptr;
    let vm_ptr = &mut vm as *mut crate::vm::VM as *mut ();
    let scope = crate::value::arena::begin_macro_scope(unsafe { &mut *heap });
    let arena = scope.arena();

    let v = Value::int(42);
    let cell = elle_jit_make_capture_owned(v.tag, v.payload, vm_ptr).to_value();
    let args = [Value::int(1), Value::int(2)];
    let list = elle_jit_collect_rest_list(args.as_ptr(), 0, 2, vm_ptr).to_value();
    assert_eq!(
        crate::value::arena::region_of(unsafe { &*heap }, cell),
        Some(arena),
        "a capture cell the JIT entry makes inside a macro scope must join its arena",
    );
    assert_eq!(
        crate::value::arena::region_of(unsafe { &*heap }, list),
        Some(arena),
        "a rest list the JIT entry builds inside a macro scope must join its arena",
    );

    crate::value::arena::reclaim_macro_scope(unsafe { &mut *heap }, scope);
    assert_eq!(
        crate::value::arena::region_rc(unsafe { &*heap }, arena),
        0,
        "the close frees the arena and every env value joined into it",
    );
}
