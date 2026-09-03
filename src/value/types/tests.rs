//! Unit tests (`super` is the parent impl module).

use super::*;

#[test]
fn test_arity_matches() {
    assert!(Arity::Exact(2).matches(2));
    assert!(!Arity::Exact(2).matches(1));
    assert!(!Arity::Exact(2).matches(3));

    assert!(Arity::AtLeast(2).matches(2));
    assert!(Arity::AtLeast(2).matches(3));
    assert!(!Arity::AtLeast(2).matches(1));

    assert!(Arity::Range(1, 3).matches(1));
    assert!(Arity::Range(1, 3).matches(2));
    assert!(Arity::Range(1, 3).matches(3));
    assert!(!Arity::Range(1, 3).matches(0));
    assert!(!Arity::Range(1, 3).matches(4));
}

#[test]
fn test_arity_display() {
    assert_eq!(format!("{}", Arity::Exact(2)), "2");
    assert_eq!(format!("{}", Arity::AtLeast(1)), "1+");
    assert_eq!(format!("{}", Arity::Range(1, 3)), "1-3");
}

#[test]
fn test_symbol_id_display() {
    assert_eq!(format!("{}", SymbolId(42)), "Symbol(42)");
}

// An immutable struct and an immutable set are sorted arrays, and symbol keys
// sort by raw id. Two compilations that met the same names in opposite orders
// must therefore lay their keys out identically, or a container built by one is
// misread by the other.
//
// Counter-factual: under mint-order ids the two sorts are exact reverses of
// each other, and `sorted_struct_get` on the second layout finds the wrong
// entry — which is what a worker returning a symbol-keyed struct used to do.
#[test]
fn symbol_keys_sort_the_same_in_every_table() {
    use crate::symbol::SymbolTable;

    let names = ["zeta-sort", "mu-sort", "alpha-sort"];
    let mut forward = SymbolTable::new();
    let mut backward = SymbolTable::new();

    let mut a: Vec<TableKey> = names
        .iter()
        .map(|n| TableKey::Symbol(forward.intern(n)))
        .collect();
    let mut b: Vec<TableKey> = names
        .iter()
        .rev()
        .map(|n| TableKey::Symbol(backward.intern(n)))
        .collect();
    a.sort();
    b.sort();

    assert_eq!(a, b);
}

// A sorted struct built against one table is probed correctly through the
// other — the binary search and the layout share a comparator that depends on
// nothing but the names.
#[test]
fn a_struct_built_in_one_table_is_probed_by_another() {
    use crate::symbol::SymbolTable;

    let mut builder = SymbolTable::new();
    let mut prober = SymbolTable::new();
    let _ = prober.intern("probe-decoy-1");
    let _ = prober.intern("probe-decoy-2");

    let mut entries: Vec<(TableKey, Value)> = ["probe-zeta", "probe-mu", "probe-alpha"]
        .iter()
        .enumerate()
        .map(|(i, n)| (TableKey::Symbol(builder.intern(n)), Value::int(i as i64)))
        .collect();
    entries.sort_by_key(|(a, _)| *a);

    for (i, n) in ["probe-zeta", "probe-mu", "probe-alpha"].iter().enumerate() {
        let key = TableKey::Symbol(prober.intern(n));
        assert_eq!(
            sorted_struct_get(&entries, &key),
            Some(&Value::int(i as i64)),
            "{} must resolve to the value the builder stored",
            n
        );
    }
}

// ── Struct keys (docs/impl/values.md § "Struct keys") ──────────────────────

// A key owns no Rust-heap allocation, so a struct's entries can be page bytes.
// `Copy` is the compile-time statement of that: a variant holding a `String` or
// a `Vec` cannot satisfy it.
#[test]
fn a_key_owns_no_rust_heap_allocation() {
    fn only_copy_types<T: Copy>() {}
    only_copy_types::<TableKey>();
}

// A probe key aliases what it reads, so `(get s "name")` allocates nothing.
#[test]
fn a_probe_key_borrows_the_value_it_reads() {
    let h = crate::primitives::ctx::TestHeap::new();
    let source = h.ctx().string("name");
    let key = TableKey::from_value(&source).expect("a string is a valid key");
    let TableKey::String(borrowed) = key else {
        panic!("a string value must build a string key");
    };
    assert_eq!(
        borrowed.payload, source.payload,
        "a probe key must not copy the string it reads"
    );
}

// A stored key is interned into the destination region, so a struct's key bytes
// are part of the struct's own body. The trap: `from_value` hands back a
// borrowed key, and storing it verbatim leaves the struct pointing into
// whatever region the caller's key came from.
#[test]
fn a_stored_string_key_is_copied_into_the_struct_region() {
    let heap_ptr = crate::value::arena::leaked_test_heap();

    let heap = unsafe { &mut *heap_ptr };
    let source_region = heap.new_runtime_region();
    let source = crate::value::build::string(heap, "name", source_region);

    let heap = unsafe { &mut *heap_ptr };
    let struct_region = heap.new_runtime_region();
    let mut fields = std::collections::BTreeMap::new();
    fields.insert(
        TableKey::from_value(&source).expect("a string is a valid key"),
        Value::int(1),
    );
    let built = crate::value::build::struct_from(heap, fields, struct_region);

    let entries = built.as_struct().expect("an immutable struct");
    let TableKey::String(stored) = entries[0].0 else {
        panic!("a string value must build a string key");
    };
    assert_ne!(
        stored.payload, source.payload,
        "a stored key must not alias the string it was built from"
    );
    assert_eq!(
        crate::value::arena::region_of(unsafe { &*heap_ptr }, stored),
        Some(struct_region),
        "a stored key's bytes belong to the struct's own region"
    );
}

// Interning is what keeps a struct from pinning the region its key came from.
// Counter-factual: store the borrowed key instead, and the alloc-time
// cross-region scan increfs the source region — every `put` in a chain then
// holds the whole previous link alive.
#[test]
fn a_struct_does_not_pin_the_region_its_key_came_from() {
    let heap_ptr = crate::value::arena::leaked_test_heap();

    let heap = unsafe { &mut *heap_ptr };
    let source_region = heap.new_runtime_region();
    let source = crate::value::build::string(heap, "name", source_region);
    let rc_before = crate::value::arena::region_rc(unsafe { &*heap_ptr }, source_region);

    let heap = unsafe { &mut *heap_ptr };
    let struct_region = heap.new_runtime_region();
    let mut fields = std::collections::BTreeMap::new();
    fields.insert(
        TableKey::from_value(&source).expect("a string is a valid key"),
        Value::int(1),
    );
    let _built = crate::value::build::struct_from(heap, fields, struct_region);

    assert_eq!(
        crate::value::arena::region_rc(unsafe { &*heap_ptr }, source_region),
        rc_before,
        "an interned key takes no reference to the region it was copied from"
    );
}

// The alloc-time scan and the `@struct` store funnels reach a key's region
// through `for_each_heap_value`. A string or array key holds a `Value`, so it
// must be enumerated there; a key the walk skips is a region nothing counts.
#[test]
fn the_heap_value_walk_enumerates_string_and_array_keys() {
    let h = crate::primitives::ctx::TestHeap::new();
    let text = h.ctx().string("name");
    let nested = h.ctx().array(vec![text, Value::int(1)]);

    let mut seen = Vec::new();
    TableKey::from_value(&text)
        .expect("a string is a valid key")
        .for_each_heap_value(&mut |v| seen.push(*v));
    assert_eq!(seen, vec![text], "a string key holds one heap value");

    let mut seen = Vec::new();
    TableKey::from_value(&nested)
        .expect("an immutable array is a valid key")
        .for_each_heap_value(&mut |v| seen.push(*v));
    assert_eq!(seen, vec![nested], "an array key holds one heap value");
}

// A struct prints and iterates in key order, so the key ranking is user-visible
// and independent of `Value::Ord`. The trap: `Value::Ord` ranks keywords BEFORE
// strings, so folding either variant into `Heap` — whose comparison delegates to
// `Value::Ord` — silently reorders every struct that mixes the two.
#[test]
fn keys_rank_in_declaration_order_not_value_order() {
    let h = crate::primitives::ctx::TestHeap::new();
    let text = h.ctx().string("s");
    let array = h.ctx().array(vec![Value::int(1)]);
    let pair = h.ctx().pair(Value::int(1), Value::EMPTY_LIST);

    let mut keys = vec![
        TableKey::from_value(&pair).unwrap(),
        TableKey::from_value(&array).unwrap(),
        TableKey::EmptyList,
        TableKey::keyword("k"),
        TableKey::from_value(&text).unwrap(),
        TableKey::Symbol(SymbolId::of("s")),
        TableKey::Int(1),
        TableKey::Bool(true),
        TableKey::Nil,
    ];
    keys.sort();

    let ranks: Vec<u8> = keys.iter().map(|k| k.discriminant_index()).collect();
    assert_eq!(
        ranks,
        vec![0, 1, 2, 3, 4, 5, 6, 7, 8],
        "keys must sort nil, bool, int, symbol, string, keyword, empty list, array, heap"
    );
}
