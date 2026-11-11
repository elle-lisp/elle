// audited: 2026-11-09
//! A leaf a body hoists keeps its cell placeholder through the destructure that fills it.
//!
//! docs/impl/region/cells.md
//! docs/bindings.md

use super::*;

/// A lambda reads `r` before the `def` that binds it, so the body hoists `r`:
/// a `Define` sets it nil before the lambda, and the destructure in the `when`
/// fills it (docs/bindings.md). The lambda captures `r`, which is reassigned,
/// so `r` is a `populate_env` env cell, and the `Define` records the cell's
/// placeholder among `r`'s regions. The destructure then records the list's
/// region. Both must stay: the placeholder is what releases the cell at `r`'s
/// last use, and the list region is what keeps the list alive until then.
///
/// Counter-factual: a destructure that overwrote `binding_regions[r]` would
/// drop the placeholder, and the cell would never be released.
#[test]
fn a_hoisted_leaf_keeps_its_cell_through_the_destructure() {
    let (hir, arena, info) =
        pipeline("(defn go [c] (defn g [] r) (when c (def (a & r) (list 1 2 3))) (g))");
    let r = find_binding_by_name(&hir, "r", &arena).expect("expected binding `r`");
    let regions = info
        .binding_source_regions
        .get(&r)
        .cloned()
        .unwrap_or_default();
    assert!(
        regions
            .iter()
            .any(|region| info.cell_release_regions.contains(region)),
        "the hoisted `r` keeps its env-cell placeholder; regions={regions:?}"
    );
    let lists = find_calls_to_primitive(&hir, "list", &arena);
    assert_eq!(lists.len(), 1, "the shape has one (list ...) call");
    let list_region = info
        .alloc_region
        .get(&lists[0])
        .copied()
        .expect("the list has a region");
    assert!(
        regions.contains(&list_region),
        "the destructure records the list's region r{} for `r`; regions={regions:?}",
        list_region.0
    );
}
