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
    entries.sort_by(|(a, _), (b, _)| a.cmp(b));

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
