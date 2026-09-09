// audited: 2026-09-09
// An `@struct` key is interned into the container's region, and costs that
// region nothing.
//
// docs/impl/values.md

use super::*;

// A key that is actually stored is interned into the container's region, so an
// `@struct` never holds a key borrowed from whatever region the caller built it
// in (docs/impl/values.md § "Struct keys").
#[test]
fn a_stored_at_struct_key_is_interned_into_the_container_region() {
    use crate::value::heap::TableKey;

    let heap_ptr = crate::value::arena::leaked_test_heap();
    let heap = unsafe { &mut *heap_ptr };
    let source_region = heap.new_runtime_region();
    let source = crate::value::build::string(heap, "name", source_region);

    let heap = unsafe { &mut *heap_ptr };
    let container_region = heap.new_runtime_region();
    let container = crate::value::build::struct_mut(heap, container_region);

    let heap = unsafe { &mut *heap_ptr };
    let probe = TableKey::from_value(&source).expect("a string is a valid key");
    struct_put_with_rebind(heap, container, probe, Value::int(1));

    let map = container.as_struct_mut_raw().expect("an @struct");
    let stored = *map.borrow().keys().next().expect("one entry");
    assert_ne!(
        stored.to_value().payload,
        source.payload,
        "the stored key must not alias the string it was built from"
    );
    assert_eq!(
        region_of(unsafe { &*heap_ptr }, stored.to_value()),
        Some(container_region),
        "the stored key's bytes belong to the container's region"
    );
}

// A rebind stores no key: `BTreeMap::insert` keeps the entry's existing key, so
// interning one for a key already present would leave the copy unreachable in
// the container's region for the rest of its life.
#[test]
fn rebinding_an_at_struct_key_interns_nothing_new() {
    use crate::value::heap::TableKey;

    let heap_ptr = crate::value::arena::leaked_test_heap();
    let heap = unsafe { &mut *heap_ptr };
    let source_region = heap.new_runtime_region();
    let first = crate::value::build::string(heap, "name", source_region);
    let second = crate::value::build::string(heap, "name", source_region);

    let heap = unsafe { &mut *heap_ptr };
    let container_region = heap.new_runtime_region();
    let container = crate::value::build::struct_mut(heap, container_region);

    let heap = unsafe { &mut *heap_ptr };
    let probe = TableKey::from_value(&first).expect("a string is a valid key");
    struct_put_with_rebind(heap, container, probe, Value::int(1));
    let map = container.as_struct_mut_raw().expect("an @struct");
    let stored = *map.borrow().keys().next().expect("one entry");

    let heap = unsafe { &mut *heap_ptr };
    let probe = TableKey::from_value(&second).expect("a string is a valid key");
    let displaced = struct_put_with_rebind(heap, container, probe, Value::int(2));

    assert_eq!(
        displaced,
        Some(Value::int(1)),
        "the rebind displaced a value"
    );
    assert_eq!(map.borrow().len(), 1, "an equal key is one entry");
    assert_eq!(
        map.borrow().keys().next().expect("one entry").to_value(),
        stored.to_value(),
        "a rebind keeps the key the first store interned"
    );
}

// ── A key's region is counted only when it is another region ───────────────
//
// docs/impl/values.md § "A key's region is counted like a value's". An interned
// key is co-region with its container, so it is a self-edge that neither the RC
// nor the outgoing-edge table counts — the rule the free-time cascade already
// follows through its `own_id` filter.

// The counter-factual: a put funnel that increfs the container's own region for
// its interned key takes a reference the free cascade never releases, because
// the cascade skips self-edges. `put` of a string key then leaked the container
// region outright — one region per call, measured in
// tests/elle/region-struct-key.lisp.
#[test]
fn an_interned_at_struct_key_adds_no_reference_to_the_container_region() {
    use crate::value::heap::TableKey;

    let heap_ptr = crate::value::arena::leaked_test_heap();
    let heap = unsafe { &mut *heap_ptr };
    let source_region = heap.new_runtime_region();
    let source = crate::value::build::string(heap, "name", source_region);

    let heap = unsafe { &mut *heap_ptr };
    let container_region = heap.new_runtime_region();
    let container = crate::value::build::struct_mut(heap, container_region);

    let rc_before = region_rc(unsafe { &*heap_ptr }, container_region);
    let heap = unsafe { &mut *heap_ptr };
    let probe = TableKey::from_value(&source).expect("a string is a valid key");
    struct_put_with_rebind(heap, container, probe, Value::int(1));

    let map = container.as_struct_mut_raw().expect("an @struct");
    let stored = *map.borrow().keys().next().expect("one entry");
    assert_eq!(
        region_of(unsafe { &*heap_ptr }, stored.to_value()),
        Some(container_region),
        "the put interned the key into the container's region",
    );
    assert_eq!(
        region_rc(unsafe { &*heap_ptr }, container_region),
        rc_before,
        "an interned key is a self-edge: it takes no reference on the container's region",
    );
    assert!(
        unsafe { (*heap_ptr).outgoing_edges(container_region) }.is_empty(),
        "and records no outgoing edge either",
    );
}

// The remove half follows the same rule, because it cannot tell whether the key
// it removes was interned by a constructor or by the put funnel. The
// counter-factual: `del` of a key `struct_mut_from` interned decrefs the
// container's region — taking a sole reference to zero — and the next read of
// the struct lands on a freed page (the macOS `--trace=scrub` crash in
// tests/elle/array-keys.lisp).
#[test]
fn removing_a_constructor_interned_key_leaves_the_container_region_live() {
    use crate::value::heap::TableKey;

    let heap_ptr = crate::value::arena::leaked_test_heap();
    let heap = unsafe { &mut *heap_ptr };
    let source_region = heap.new_runtime_region();
    let source = crate::value::build::string(heap, "name", source_region);
    let probe = TableKey::from_value(&source).expect("a string is a valid key");

    // The constructor funnel — what `@{"name" 1}` and the receiving side of
    // `send` both go through. It interns the key, and the alloc-time scan
    // counts nothing for it.
    let heap = unsafe { &mut *heap_ptr };
    let container_region = heap.new_runtime_region();
    let container = crate::value::build::struct_mut_from(
        heap,
        std::collections::BTreeMap::from([(probe, Value::int(1))]),
        container_region,
    );
    let rc_before = region_rc(unsafe { &*heap_ptr }, container_region);

    let heap = unsafe { &mut *heap_ptr };
    assert_eq!(
        struct_remove_with_decref(heap, container, &probe),
        Some(Value::int(1)),
        "the interned key is found by the probe it was built from",
    );
    assert_eq!(
        region_rc(unsafe { &*heap_ptr }, container_region),
        rc_before,
        "removing a self-edge key releases nothing on the container's region",
    );
    assert_eq!(
        region_of(unsafe { &*heap_ptr }, container),
        Some(container_region),
        "the container still lives in its region after the remove",
    );
}
