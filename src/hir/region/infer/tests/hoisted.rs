// audited: 2026-11-17
//! A leaf a body hoists keeps its cell placeholder through the destructure that fills it.
//!
//! docs/impl/region/cells.md
//! docs/bindings.md

use super::*;
use crate::value::SymbolId;

/// The `r` the body hoists. The file's `r` is never assigned, and the hoisted
/// one is, because its `def` fills it after the hoist set it.
fn hoisted_r(hir: &Hir, arena: &BindingArena) -> Binding {
    fn walk(hir: &Hir, arena: &BindingArena, out: &mut Option<Binding>) {
        if let HirKind::Var(b) = &hir.kind {
            let info = arena.get(*b);
            if info.name == SymbolId::of("r") && info.is_mutated {
                *out = Some(*b);
            }
        }
        hir.for_each_child(|c| walk(c, arena, out));
    }
    let mut out = None;
    walk(hir, arena, &mut out);
    out.expect("the body hoists an assigned `r`")
}

/// A lambda reads `r` before the `def` that binds it, and the file binds `r`
/// outside the body, so the body hoists `r`: a `Define` sets it to the file's
/// `r` before the lambda, and the destructure fills it
/// (docs/bindings.md). The lambda captures `r`, which is reassigned,
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
        pipeline("(def r 0) (defn go [] (defn g [] r) (def (a & r) (list 1 2 3)) (g))");
    let r = hoisted_r(&hir, &arena);
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
