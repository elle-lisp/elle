// audited: 2026-09-15
//! What a pattern's rest name owns, and where the solver anchors its release.
//!
//! docs/impl/region/anchors.md

use super::*;
use crate::hir::HirPattern;
use crate::value::SymbolId;

/// The HirId of the first `Destructure` node in the tree.
fn first_destructure(hir: &Hir) -> Option<HirId> {
    find_first(hir, |h| matches!(&h.kind, HirKind::Destructure { .. }))
}

/// The HirId of the first `Match` node in the tree.
fn first_match(hir: &Hir) -> Option<HirId> {
    find_first(hir, |h| matches!(&h.kind, HirKind::Match { .. }))
}

/// The placeholder regions recorded against `node`, each beside the source
/// name of the binding it was recorded for. A binding is found by identity
/// (`SymbolId::of`), never through a memo that may never have learned the name.
fn rest_regions(info: &RegionInfo, arena: &BindingArena, node: HirId) -> Vec<(SymbolId, Region)> {
    info.pattern_rest_regions
        .get(&node)
        .map(|v| v.iter().map(|&(b, r)| (arena.get(b).name, r)).collect())
        .unwrap_or_default()
}

/// The names `node`'s placeholders were recorded for, sorted so the assertion
/// does not depend on the walk's visit order.
fn rest_names(info: &RegionInfo, arena: &BindingArena, node: HirId) -> Vec<SymbolId> {
    let mut names: Vec<SymbolId> = rest_regions(info, arena, node)
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    names.sort_by_key(|s| format!("{s}"));
    names
}

/// The region of the one rest name `node`'s pattern binds, or a panic naming
/// how many were recorded instead.
fn sole_rest_region(info: &RegionInfo, arena: &BindingArena, node: HirId) -> Region {
    let recorded = rest_regions(info, arena, node);
    assert_eq!(
        recorded.len(),
        1,
        "one rest name, one placeholder — {} recorded",
        recorded.len()
    );
    recorded[0].1
}

const PRELUDE: &str = "(def src [1 2 3 4 5]) \
                       (def rec {:a 1 :b 2 :c 3}) \
                       (def lst '(1 2 3 4 5)) ";

#[test]
fn an_array_rest_name_takes_a_placeholder_region_of_its_own() {
    let (hir, arena, info) = pipeline(&format!("{PRELUDE} (let [[x y & r] src] x)"));
    let node = first_destructure(&hir).expect("a Destructure node");
    assert_eq!(
        rest_names(&info, &arena, node),
        vec![SymbolId::of("r")],
        "the rest name is the one name the destructure builds a value for"
    );
    let r = sole_rest_region(&info, &arena, node);
    assert!(
        info.call_result_regions.contains(&r),
        "the placeholder takes the value route, so it is a call-result region"
    );
    assert!(
        !info.live_regions.contains(&r),
        "the placeholder is phantom: the opcode mints the physical region, so \
         no compiled allocation names a static slot for it"
    );
    assert!(
        !info.alloc_region.values().any(|&a| a == r),
        "no HIR node allocates the placeholder"
    );
}

#[test]
fn a_struct_rest_name_takes_a_placeholder_region_of_its_own() {
    let (hir, arena, info) = pipeline(&format!("{PRELUDE} (let [{{:a one & r}} rec] one)"));
    let node = first_destructure(&hir).expect("a Destructure node");
    assert_eq!(
        rest_names(&info, &arena, node),
        vec![SymbolId::of("r")],
        "`StructRest` builds a new struct exactly as `ArrayMutSliceFrom` builds \
         a new array"
    );
}

#[test]
fn a_flat_pattern_takes_no_placeholder() {
    // The counter-factual: the same five elements, bound by name. Every one is
    // a projection of the scrutinee, so nothing is built and nothing is owned.
    let (hir, arena, info) = pipeline(&format!("{PRELUDE} (let [[x y z w v] src] x)"));
    let node = first_destructure(&hir).expect("a Destructure node");
    assert!(
        rest_regions(&info, &arena, node).is_empty(),
        "a pattern with no rest builds nothing"
    );
}

#[test]
fn a_list_rest_takes_no_placeholder() {
    // A list rest is the remaining cons tail — a pointer into the scrutinee's
    // own cells, so it is a borrow like every other projection.
    let (hir, arena, info) = pipeline(&format!("{PRELUDE} (let [(x y & r) lst] x)"));
    let node = first_destructure(&hir).expect("a Destructure node");
    assert!(
        rest_regions(&info, &arena, node).is_empty(),
        "a list rest allocates nothing"
    );
}

#[test]
fn a_nested_rest_sub_pattern_takes_no_placeholder() {
    // The boundary of the closed case (elle-lisp/elle#1127): `& [p q]` builds a
    // collection no name holds, so there is no slot for a value route to load
    // and the collection keeps the conservative baseline. `p` and `q` are
    // projections of it, not of the scrutinee.
    let (hir, arena, info) = pipeline(&format!("{PRELUDE} (let [[x & [p q]] src] x)"));
    let node = first_destructure(&hir).expect("a Destructure node");
    assert!(
        rest_regions(&info, &arena, node).is_empty(),
        "a rest matched by a further pattern binds no name to the collection"
    );
}

#[test]
fn an_unread_rest_name_still_gets_a_release_point() {
    // The shape that provokes the defect most often. Nothing reads `r`, so the
    // binding chain has no use to extend a release over; without the base pin
    // at the destructure node the region has no `region_data` entry at all and
    // the lowerer emits no release.
    let (hir, arena, info) = pipeline(&format!("{PRELUDE} (let [[x y & r] src] x)"));
    let node = first_destructure(&hir).expect("a Destructure node");
    let r = sole_rest_region(&info, &arena, node);
    let data = info
        .region_data
        .get(&r)
        .expect("an unread rest name's collection still has a decref_point");
    assert_eq!(
        data.decref_point, node,
        "with no use to extend over, the release lands on the destructure node"
    );
}

#[test]
fn a_read_of_the_rest_name_moves_the_release_past_the_destructure() {
    // Every pin is a max, so the ordinary binding chain still wins over the
    // base. Anchored at the destructure, `(length r)` reads freed pages.
    let (hir, arena, info) = pipeline(&format!("{PRELUDE} (let [[x y & r] src] (length r))"));
    let node = first_destructure(&hir).expect("a Destructure node");
    let r = sole_rest_region(&info, &arena, node);
    let dp = info
        .region_data
        .get(&r)
        .expect("the rest collection has a decref_point")
        .decref_point;
    let order = compute_order(&hir);
    let at = |id: HirId| order.get(&id).copied().unwrap_or(0);
    assert!(
        at(dp) > at(node),
        "the read extends the release past the destructure: dp @{} vs \
         destructure @{}",
        dp.0,
        node.0
    );
}

#[test]
fn a_match_arm_rest_name_takes_a_placeholder_pinned_at_the_match() {
    // The `Match` node is post-ordered after every arm body, so the base pin
    // covers the paths a failed guard and a rejected alternative leave behind.
    let (hir, arena, info) = pipeline(&format!("{PRELUDE} (match src [x y & r] x)"));
    let node = first_match(&hir).expect("a Match node");
    let r = sole_rest_region(&info, &arena, node);
    assert!(
        info.call_result_regions.contains(&r),
        "a `match` rest name owns its collection exactly as a `let` one does"
    );
    let dp = info
        .region_data
        .get(&r)
        .expect("the rest collection has a decref_point")
        .decref_point;
    assert_eq!(
        dp, node,
        "the release lands at the merge every arm arrives at"
    );
}

#[test]
fn each_arm_of_a_match_gets_its_own_placeholder() {
    // Two arms, two collections, two regions: one release per arm, each
    // loading its own slot, so the arm that did not run releases a `nil`.
    let (hir, arena, info) = pipeline(&format!("{PRELUDE} (match src [x & r] x [x y & q] y _ 0)"));
    let node = first_match(&hir).expect("a Match node");
    let mut expected = vec![SymbolId::of("q"), SymbolId::of("r")];
    expected.sort_by_key(|s| format!("{s}"));
    assert_eq!(rest_names(&info, &arena, node), expected);
    let regions: Vec<Region> = rest_regions(&info, &arena, node)
        .into_iter()
        .map(|(_, r)| r)
        .collect();
    assert_ne!(
        regions[0], regions[1],
        "two arms never share one placeholder — a shared slot orphans the \
         first collection the moment the second maps its own mint over it"
    );
}

#[test]
fn the_rest_name_is_the_only_pattern_name_that_owns_anything() {
    // The fixed names stay borrows of the scrutinee: giving one a placeholder
    // would release a value living inside the scrutinee's own pages.
    let (hir, arena, info) = pipeline(&format!("{PRELUDE} (let [[x y & r] src] x)"));
    let node = first_destructure(&hir).expect("a Destructure node");
    let names = rest_names(&info, &arena, node);
    assert!(
        !names.contains(&SymbolId::of("x")) && !names.contains(&SymbolId::of("y")),
        "a fixed element name is a projection, not a built value"
    );
}

/// The pattern predicate the solver and the lowerer both read, over every shape
/// the corpus writes. A disagreement here is a placeholder with no route (an
/// over-keep) or a route with no placeholder (nothing emitted).
#[test]
fn the_allocating_rest_predicate_names_exactly_the_building_patterns() {
    let count = |p: &HirPattern| p.allocating_rest_bindings().len();
    let b = |n: u32| crate::hir::Binding(n);
    let var = |n: u32| HirPattern::Var(b(n));

    assert_eq!(
        count(&HirPattern::Tuple {
            elements: vec![var(1)],
            rest: Some(Box::new(var(2))),
        }),
        1,
        "an array rest lowers to ArrayMutSliceFrom"
    );
    assert_eq!(
        count(&HirPattern::Array {
            elements: vec![],
            rest: Some(Box::new(var(2))),
        }),
        1,
        "an `@array` rest lowers to the same opcode"
    );
    assert_eq!(
        count(&HirPattern::Struct {
            entries: vec![],
            rest: Some(Box::new(var(2))),
        }),
        1,
        "a struct rest lowers to StructRest"
    );
    assert_eq!(
        count(&HirPattern::Table {
            entries: vec![],
            rest: Some(Box::new(var(2))),
        }),
        1,
        "an `@struct` rest lowers to StructRest"
    );
    assert_eq!(
        count(&HirPattern::List {
            elements: vec![var(1)],
            rest: Some(Box::new(var(2))),
        }),
        0,
        "a list rest is the remaining cons tail"
    );
    assert_eq!(
        count(&HirPattern::Tuple {
            elements: vec![var(1)],
            rest: None,
        }),
        0,
        "no rest, nothing built"
    );
    assert_eq!(
        count(&HirPattern::Tuple {
            elements: vec![],
            rest: Some(Box::new(HirPattern::Wildcard)),
        }),
        0,
        "a wildcard rest binds no name to the collection"
    );
    assert_eq!(
        count(&HirPattern::Tuple {
            elements: vec![],
            rest: Some(Box::new(HirPattern::Tuple {
                elements: vec![var(1)],
                rest: Some(Box::new(var(2))),
            })),
        }),
        1,
        "a nested rest contributes only the inner bare name it does bind"
    );
    assert_eq!(
        count(&HirPattern::Tuple {
            elements: vec![HirPattern::Struct {
                entries: vec![],
                rest: Some(Box::new(var(1))),
            }],
            rest: Some(Box::new(var(2))),
        }),
        2,
        "a rest inside an element and the pattern's own rest are two builds"
    );
}
