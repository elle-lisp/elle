// audited: 2026-11-09
//! A `letrec` binding a sibling captures inside a lambda takes an env cell, and the walk arms that cell's release.
//!
//! docs/impl/region/cells.md

use super::*;

/// Every binding of a `Letrec` under a `Lambda` that a sibling captures and
/// whose init is not a closure: the bindings `populate_env` gives an env cell.
fn env_celled_letrec_bindings(hir: &Hir, arena: &BindingArena) -> Vec<Binding> {
    fn walk(h: &Hir, arena: &BindingArena, in_lambda: bool, out: &mut Vec<Binding>) {
        if in_lambda {
            if let HirKind::Letrec { bindings, .. } = &h.kind {
                out.extend(
                    bindings
                        .iter()
                        .filter(|(b, init)| {
                            arena.get(*b).needs_capture()
                                && !matches!(init.kind, HirKind::Lambda { .. })
                        })
                        .map(|(b, _)| *b),
                );
            }
        }
        let inner = in_lambda || matches!(&h.kind, HirKind::Lambda { .. });
        h.for_each_child(|c| walk(c, arena, inner, out));
    }
    let mut out = Vec::new();
    walk(hir, arena, false, &mut out);
    out
}

#[test]
fn an_in_lambda_letrec_env_cell_takes_one_release_placeholder() {
    // `v` holds a heap value and `w` an immediate. A sibling captures both, and
    // a `letrec` binding is prebound, so each takes a `populate_env` env cell,
    // the immutable `w` included. The walk owes each cell one placeholder in
    // `cell_release_regions`, as its `Let` and `Define` arms record one, and
    // the placeholder needs a release point.
    //
    // The counter-factual: the `Letrec` arm recorded no placeholder, so no
    // `DecrefCellRegion` was emitted, and each cell, with the value it holds,
    // leaked once per call (tests/impl/region-letrec-capture-cell-leak.lisp).
    let (hir, arena, info) =
        pipeline("(defn f [] (letrec [v (%pair 1 2) w 5 g (fn [] (%pair v w))] (g)))");
    let bindings = env_celled_letrec_bindings(&hir, &arena);
    assert_eq!(
        bindings.len(),
        2,
        "the shape must give `v` and `w` env cells; found {bindings:?} — if the \
         analyzer changed, update the shape so this test keeps biting",
    );
    for b in bindings {
        let regions = info
            .binding_source_regions
            .get(&b)
            .cloned()
            .unwrap_or_default();
        let cells: Vec<Region> = regions
            .iter()
            .copied()
            .filter(|r| info.cell_release_regions.contains(r))
            .collect();
        assert_eq!(
            cells.len(),
            1,
            "letrec binding {b:?} must own exactly one env-cell placeholder; \
             binding_source_regions={regions:?}",
        );
        assert!(
            info.region_data.contains_key(&cells[0]),
            "the env-cell placeholder r{} of {b:?} has no release point",
            cells[0].0,
        );
    }
}
