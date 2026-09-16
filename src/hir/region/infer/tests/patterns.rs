// audited: 2026-09-16
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

/// The pattern of the first `Destructure` node in the tree, so a test can ask
/// the same question of the pattern that the lowerer asks.
fn first_destructure_pattern(hir: &Hir) -> Option<&HirPattern> {
    fn walk<'h>(h: &'h Hir, found: &mut Option<&'h HirPattern>) {
        if found.is_none() {
            if let HirKind::Destructure { pattern, .. } = &h.kind {
                *found = Some(pattern);
                return;
            }
            h.for_each_child(|c| walk(c, found));
        }
    }
    let mut found = None;
    walk(hir, &mut found);
    found
}

/// One entry per collection `node`'s pattern builds, in the order the entries
/// were recorded: the placeholder, and the source names of the holders keyed on
/// it. A build no name reaches is an entry with an empty name list, which
/// `rest_names` cannot show because it has no name to show.
fn rest_entries(
    info: &RegionInfo,
    arena: &BindingArena,
    node: HirId,
) -> Vec<(Region, Vec<SymbolId>)> {
    info.pattern_rest_regions
        .get(&node)
        .map(|v| {
            v.iter()
                .map(|c| {
                    (
                        c.region,
                        c.holders.iter().map(|&b| arena.get(b).name).collect(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The placeholder regions recorded against `node`, each beside the source
/// name of the binding it was recorded for. A binding is found by identity
/// (`SymbolId::of`), never through a memo that may never have learned the name.
fn rest_regions(info: &RegionInfo, arena: &BindingArena, node: HirId) -> Vec<(SymbolId, Region)> {
    info.pattern_rest_regions
        .get(&node)
        .map(|v| {
            v.iter()
                .flat_map(|c| c.holders.iter().map(|&b| (arena.get(b).name, c.region)))
                .collect()
        })
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

/// The placeholder recorded for the name `name` at `node`, or a panic. The
/// name is resolved by identity, as `rest_regions` resolves every other.
fn rest_region_of(info: &RegionInfo, arena: &BindingArena, node: HirId, name: &str) -> Region {
    rest_regions(info, arena, node)
        .into_iter()
        .find(|&(n, _)| n == SymbolId::of(name))
        .unwrap_or_else(|| panic!("no placeholder was recorded for `{name}`"))
        .1
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
fn a_nested_rest_sub_pattern_takes_one_placeholder_for_every_name_it_binds() {
    // `& [p q]` builds a collection no single name holds, and `p` and `q` are
    // projections of THAT collection rather than of the scrutinee. So it must
    // outlive both, and one region over both is what the binding chain extends.
    let (hir, arena, info) = pipeline(&format!("{PRELUDE} (let [[x & [p q]] src] x)"));
    let node = first_destructure(&hir).expect("a Destructure node");
    let mut expected = vec![SymbolId::of("p"), SymbolId::of("q")];
    expected.sort_by_key(|s| format!("{s}"));
    assert_eq!(rest_names(&info, &arena, node), expected);
    let p = rest_region_of(&info, &arena, node, "p");
    let q = rest_region_of(&info, &arena, node, "q");
    assert_eq!(
        p, q,
        "one build, one region — two would park one collection in two slots \
         and release it twice"
    );
    assert!(
        info.call_result_regions.contains(&p),
        "the placeholder takes the value route, so it is a call-result region"
    );
    assert!(
        !info.live_regions.contains(&p),
        "the placeholder is phantom, exactly as a bare rest name's is"
    );
}

#[test]
fn a_read_of_a_nested_rest_name_moves_the_release_past_the_destructure() {
    // `p` is an uncounted read of the collection, so the collection is used for
    // as long as `p` is. Anchored at the destructure, `(length p)` reads pages
    // the release already cascaded.
    let (hir, arena, info) = pipeline(&format!("{PRELUDE} (let [[x & [p q]] src] (length p))"));
    let node = first_destructure(&hir).expect("a Destructure node");
    let r = rest_region_of(&info, &arena, node, "p");
    let dp = info
        .region_data
        .get(&r)
        .expect("the nested rest collection has a decref_point")
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
fn the_holder_set_stops_at_a_nested_build() {
    // `[x & [p & q]]` builds TWO collections. `q`'s is a fresh array of values
    // copied out of `p`'s, so it points into no page the outer one owns and
    // owes it nothing — two regions, each released on its own.
    let (hir, arena, info) = pipeline(&format!("{PRELUDE} (let [[x & [p & q]] src] x)"));
    let node = first_destructure(&hir).expect("a Destructure node");
    let p = rest_region_of(&info, &arena, node, "p");
    let q = rest_region_of(&info, &arena, node, "q");
    assert_ne!(
        p, q,
        "the inner collection is a build of its own, not a projection of the \
         outer one"
    );
}

#[test]
fn a_wildcard_rest_takes_no_placeholder() {
    // A wildcard reads nothing out of the collection, so the lowerer emits no
    // build for it and there is no allocation for a placeholder to release.
    // The fixed element's own strict extraction keeps the type check the
    // skipped opcode would have made.
    let (hir, arena, info) = pipeline(&format!("{PRELUDE} (let [[x & _] src] x)"));
    let node = first_destructure(&hir).expect("a Destructure node");
    assert!(
        rest_entries(&info, &arena, node).is_empty(),
        "a wildcard rest builds nothing to release"
    );
}

#[test]
fn a_bare_wildcard_rest_keeps_the_build_its_type_check_needs() {
    // THE COUNTER-FACTUAL. `ArrayMutSliceFrom` signals a type error on a
    // non-array as well as building the slice, and `[& _]` has no fixed
    // element to make that check. So the build stays — and a build that stays
    // owes a release, which no name of the program can key.
    let (hir, arena, info) = pipeline(&format!("{PRELUDE} (let [[& _] src] 0)"));
    let node = first_destructure(&hir).expect("a Destructure node");
    let entries = rest_entries(&info, &arena, node);
    assert_eq!(entries.len(), 1, "the build stays, so its placeholder does");
    assert!(
        entries[0].1.is_empty(),
        "no name of the program reaches the collection"
    );
    assert!(
        info.call_result_regions.contains(&entries[0].0),
        "the placeholder takes the value route like any other"
    );
}

#[test]
fn a_rest_whose_only_name_holds_the_inner_collection_still_takes_a_placeholder() {
    // `[& q]` binds `q` beneath a further build, so the descent that stops
    // there leaves the OUTER collection with no holder at all. Keyed on a name
    // it would take no placeholder and strand; keyed on its POSITION among the
    // pattern's builds it takes one like any other.
    let (hir, arena, info) = pipeline(&format!("{PRELUDE} (let [[x & [& q]] src] x)"));
    let node = first_destructure(&hir).expect("a Destructure node");
    let entries = rest_entries(&info, &arena, node);
    assert_eq!(entries.len(), 2, "two builds, two placeholders");
    assert!(
        entries[0].1.is_empty(),
        "the outer collection is reached by no name"
    );
    assert_eq!(
        entries[1].1,
        vec![SymbolId::of("q")],
        "the inner collection is the one `q` holds"
    );
    assert_ne!(
        entries[0].0, entries[1].0,
        "one region over both would park two collections in one slot"
    );
    let data = info
        .region_data
        .get(&entries[0].0)
        .expect("the nameless collection still has a decref_point");
    assert_eq!(
        data.decref_point, node,
        "with no holder to extend over, the release lands on the destructure node"
    );
}

#[test]
fn the_placeholders_follow_the_order_the_lowerer_builds_in() {
    // THE TRAP. The lowerer takes the n-th placeholder at the n-th build site,
    // so a solver that recorded a different count or a different order would
    // park each collection against another one's region. Both sides read
    // `building_rests`, and this is the assertion that they agree.
    // Three builds, and the MIDDLE one is the nameless one: a recording that
    // drops it shifts the inner collection's placeholder onto the middle build
    // and leaves the inner one with none.
    let (hir, arena, info) = pipeline(&format!(
        "{PRELUDE} (let [[x & [p & [& q]]] src] (length q))"
    ));
    let node = first_destructure(&hir).expect("a Destructure node");
    let pattern = first_destructure_pattern(&hir).expect("a Destructure pattern");
    let entries = rest_entries(&info, &arena, node);
    let built = pattern.building_rests();
    assert_eq!(
        entries.len(),
        built.len(),
        "one placeholder per build the lowerer emits"
    );
    for (i, rest) in built.iter().enumerate() {
        let holders: Vec<SymbolId> = rest
            .rest_collection_holders()
            .into_iter()
            .map(|b| arena.get(b).name)
            .collect();
        assert_eq!(
            entries[i].1, holders,
            "the {i}-th placeholder was recorded for the {i}-th build's holders"
        );
    }
}

#[test]
fn a_match_nested_rest_sub_pattern_stays_on_the_baseline() {
    // The over-reach this guards (elle-lisp/elle#1127). A decision tree loads
    // each name by walking its own access path and re-runs the `Slice` step on
    // every path through the rest, so one placeholder per sub-pattern would not
    // be one per allocation.
    let (hir, arena, info) = pipeline(&format!("{PRELUDE} (match src [x y z & [p q]] x)"));
    let node = first_match(&hir).expect("a Match node");
    assert!(
        rest_regions(&info, &arena, node).is_empty(),
        "a `match` arm's nested rest sub-pattern takes no placeholder"
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

// The pure pattern predicates these placements rest on — what a rest builds,
// and which names reach it — are unit-tested beside `HirPattern` itself
// (`hir::pattern::rest_tests`).
