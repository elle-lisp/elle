// audited: 2026-11-18
//! A pattern leaf a lambda captures keeps its cell placeholder through the destructure that fills it.
//!
//! docs/impl/region/cells.md
//! docs/bindings.md

use super::*;
use crate::value::SymbolId;

/// The `r` a lambda reads through a cell.
fn captured_r(hir: &Hir, arena: &BindingArena) -> Binding {
    fn walk(hir: &Hir, arena: &BindingArena, out: &mut Option<Binding>) {
        if let HirKind::Var(b) = &hir.kind {
            let info = arena.get(*b);
            if info.name == SymbolId::of("r") && info.needs_capture() {
                *out = Some(*b);
            }
        }
        hir.for_each_child(|c| walk(c, arena, out));
    }
    let mut out = None;
    walk(hir, arena, &mut out);
    out.expect("a lambda captures `r` through a cell")
}

/// A `letrec` binds the pattern `(a & r)`, and the lambda `g` before it reads
/// `r`. The letrec sets `r` to nil before any initializer runs, and the
/// destructure fills it (docs/bindings.md). The lambda captures `r` before it
/// holds its value, so `r` is a `populate_env` env cell, and the letrec's init
/// records the cell's placeholder among `r`'s regions. The destructure then
/// records the list's region. Both must stay: the placeholder is what releases
/// the cell at `r`'s last use, and the list region is what keeps the list alive
/// until then.
///
/// Counter-factual: a destructure that overwrote `binding_regions[r]` would
/// drop the placeholder, and the cell would never be released.
#[test]
fn a_captured_leaf_keeps_its_cell_through_the_destructure() {
    let (hir, arena, info) =
        pipeline("(defn go [] (letrec [g (fn [] r) (a & r) (list 1 2 3)] (g)))");
    let r = captured_r(&hir, &arena);
    let regions = info
        .binding_source_regions
        .get(&r)
        .cloned()
        .unwrap_or_default();
    assert!(
        regions
            .iter()
            .any(|region| info.cell_release_regions.contains(region)),
        "the captured `r` keeps its env-cell placeholder; regions={regions:?}"
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
