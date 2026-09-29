// audited: 2026-09-29
//! Which push sites the append seed admits: a fresh value pushed into a container
//! nothing ever takes a value out of.
//!
//! docs/impl/region/colocation.md
//!
//! The harness compiles without the stdlib, so each shape names the store it
//! means: `push` on a known `@array` monomorphizes to `%push-array-mut`, and a
//! stdlib-free fragment has no `push` to call.

use super::*;
use crate::value::SymbolId;

fn out() -> SymbolId {
    SymbolId::of("out")
}

/// The solver's joins for `source`, as `(allocation site, container name)`.
fn joins(source: &str) -> (Hir, BindingArena, RegionInfo, Vec<(HirId, SymbolId)>) {
    let (hir, arena, info) = analyze_full_with_arena(source);
    let mut found: Vec<(HirId, SymbolId)> = info
        .joins
        .iter()
        .map(|(&site, &b)| (site, arena.get(b).name))
        .collect();
    found.sort_by_key(|(site, _)| site.0);
    (hir, arena, info, found)
}

/// The one call to `callee` in `hir`.
fn sole_call(hir: &Hir, arena: &BindingArena, callee: &str) -> HirId {
    let calls = find_calls_to_primitive(hir, callee, arena);
    assert_eq!(calls.len(), 1, "the shape has one call to {callee}");
    calls[0]
}

const PAIRS: &str = "
(defn pairs [n]
  (def out @[])
  (var k 0)
  (while (%lt k n)
    (%push-array-mut out [k k])
    (assign k (%add k 1)))
  out)
(pairs 3)";

#[test]
fn a_push_only_builder_joins_each_pushed_value_to_its_container() {
    let (hir, arena, info, found) = joins(PAIRS);
    let site = sole_call(&hir, &arena, "array");
    assert_eq!(found, vec![(site, out())]);

    let value = info.alloc_region[&site];
    let binding = find_binding_by_name(&hir, "out", &arena).expect("the container binding");
    let container = info.binding_source_regions[&binding].clone();
    assert!(
        info.join_regions.contains(&value),
        "the pushed value's region is a join region"
    );
    for r in container {
        assert!(
            info.join_regions.contains(&r),
            "the container's region is a join region"
        );
    }
}

#[test]
fn a_pushed_string_joins_as_a_pushed_array_does() {
    let (hir, arena, _, found) = joins(
        "
(defn strings [n]
  (def out @[])
  (var k 0)
  (while (%lt k n)
    (%push-array-mut out (string \"v\" k))
    (assign k (%add k 1)))
  out)
(strings 3)",
    );
    assert_eq!(found, vec![(sole_call(&hir, &arena, "string"), out())]);
}

#[test]
fn a_container_read_by_a_native_that_stores_nothing_still_joins() {
    let (_, _, _, found) = joins(
        "
(defn fill [n]
  (def out @[])
  (while (%lt (length out) n)
    (%push-array-mut out [1 1]))
  out)
(fill 3)",
    );
    assert_eq!(
        found.len(),
        1,
        "`length` reads the container and keeps nothing"
    );
}

#[test]
fn every_push_site_into_one_container_joins() {
    let (_, _, _, found) = joins(
        "
(defn two [n]
  (def out @[])
  (%push-array-mut out [n 1])
  (%push-array-mut out [n 2])
  out)
(two 3)",
    );
    assert_eq!(found.len(), 2);
    assert!(found.iter().all(|&(_, c)| c == out()));
}

/// The refusals. Each shape does one thing the bound refuses.
fn assert_refused(what: &str, source: &str) {
    let (_, _, info, found) = joins(source);
    assert!(found.is_empty(), "{what}: the seed admitted {found:?}");
    assert!(
        info.join_regions.is_empty(),
        "{what}: no join, no join region"
    );
}

#[test]
fn a_container_that_loses_a_value_is_refused() {
    assert_refused(
        "a pop",
        "
(defn f [n]
  (def out @[])
  (%push-array-mut out [n n])
  (%pop out)
  out)
(f 3)",
    );
    assert_refused(
        "an overwrite",
        "
(defn f [n]
  (def out @[])
  (%push-array-mut out [n n])
  (%put-array-mut out 0 [n 1])
  out)
(f 3)",
    );
}

#[test]
fn a_container_that_leaves_the_function_other_than_as_its_result_is_refused() {
    assert_refused(
        "a capture",
        "
(defn f [n]
  (def out @[])
  (def size (fn [] (length out)))
  (%push-array-mut out [n n])
  (size)
  out)
(f 3)",
    );
    assert_refused(
        "a call to a function",
        "
(defn measure [c] (length c))
(defn f [n]
  (def out @[])
  (%push-array-mut out [n n])
  (measure out)
  out)
(f 3)",
    );
    assert_refused(
        "a store into another container",
        "
(defn f [n]
  (def out @[])
  (def other @[])
  (%push-array-mut out [n n])
  (%push-array-mut other out)
  other)
(f 3)",
    );
}

#[test]
fn a_container_the_function_did_not_build_is_refused() {
    assert_refused(
        "a parameter",
        "
(defn f [out]
  (%push-array-mut out [1 2])
  out)
(f @[])",
    );
    assert_refused(
        "a mutable binding",
        "
(defn f [n]
  (var out @[])
  (%push-array-mut out [n n])
  out)
(f 3)",
    );
}

#[test]
fn a_pushed_value_a_binding_also_holds_is_refused() {
    assert_refused(
        "a held value",
        "
(defn f [n]
  (def out @[])
  (var last nil)
  (def p [n n])
  (%push-array-mut out p)
  (assign last p)
  [out last])
(f 3)",
    );
}

#[test]
fn a_join_region_is_never_adopted_or_merged() {
    // A joined region's count belongs to every site that joined it, so no single
    // owner may take it, and a merge would release it through another slot.
    let (_, _, info, found) = joins(PAIRS);
    assert!(!found.is_empty(), "the shape joins");
    let adopted = info
        .owned_adopt_edges
        .values()
        .flatten()
        .flat_map(|&(child, parent)| [child, parent])
        .chain(info.activation_adopt_sites.values().flatten().copied())
        .chain(info.owned_group_members.iter().copied());
    for r in adopted {
        assert!(
            !info.join_regions.contains(&r),
            "join region {r:?} is in the forest"
        );
    }
    for (child, parent) in &info.merged_parent {
        assert!(
            !info.join_regions.contains(child) && !info.join_regions.contains(parent),
            "join region merged: {child:?} → {parent:?}",
        );
    }
}
