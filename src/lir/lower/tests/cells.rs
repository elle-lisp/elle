// audited: 2026-11-10
//! The coalescing predicate over a compiled capture cell's region, which names the box and never the content a read hands back.
//!
//! docs/impl/region/mechanism.md

use super::*;
use crate::hir::region::Region;

/// Every region a compiled capture cell was minted in.
fn compiled_cell_regions(info: &RegionInfo) -> Vec<Region> {
    info.begin_cell_regions
        .values()
        .flatten()
        .map(|&(_, r)| r)
        .collect()
}

/// Every `Var` node in `hir` whose binding's source regions are all compiled
/// cell regions. Raw pointers for the reason `return_value_ptrs` gives.
fn cell_only_reads(hir: &Hir, info: &RegionInfo) -> Vec<*const Hir> {
    let cells = compiled_cell_regions(info);
    fn go(h: &Hir, info: &RegionInfo, cells: &[Region], out: &mut Vec<*const Hir>) {
        if let HirKind::Var(b) = &h.kind {
            let only_cells = info
                .binding_source_regions
                .get(b)
                .is_some_and(|rs| !rs.is_empty() && rs.iter().all(|r| cells.contains(r)));
            if only_cells {
                out.push(h as *const Hir);
            }
        }
        h.for_each_child(|c| go(c, info, cells, out));
    }
    let mut out = Vec::new();
    go(hir, info, &cells, &mut out);
    out
}

#[test]
fn coalescible_refuses_a_compiled_cell_region() {
    // A `Var` whose one source region is live and in no other dynamic class,
    // but is a compiled cell's region. The value a read of the binding hands
    // back is the cell's content, so the slot that cell region names resolves
    // to the box, never to the value. Counterfactual: a predicate that reads the
    // cell region as a fresh local allocation accepts it, and the
    // `AssertRegionMatches` oracle detonates on an immediate's `None` region.
    let arena = crate::hir::BindingArena::new();
    let b = crate::hir::Binding(0);
    let var = Hir::silent(HirKind::Var(b), make_span());
    let cell = Region(100);
    let mut info = RegionInfo::empty();
    info.live_regions.insert(cell);
    info.binding_source_regions.insert(b, vec![cell]);
    info.begin_cell_regions
        .insert(var.id, vec![(crate::hir::Binding(1), cell)]);
    let lowerer = Lowerer::new(&arena).with_region_info(info);
    assert_eq!(
        lowerer.coalescible_solver_region(&var),
        None,
        "a Var whose single source region is a compiled cell's must NOT coalesce",
    );
}

#[test]
fn a_read_of_a_compiled_cell_binding_does_not_coalesce() {
    // `f` captures `x` before its def runs, so `x` takes a compiled cell outside
    // any lambda: through the `Begin` pre-pass in the first shape, through a
    // `letrec` in the second. `x` holds an immediate, so the only
    // source region a read of `x` names is the cell's, and `y`, bound to that
    // read, inherits it. Counterfactual: the predicate accepts the cell region
    // as `y`'s single fresh allocation, and the file's final mint coalesces onto
    // the box.
    for body in [
        "(begin (def x 42) (def f (fn () x)) (def y x) y)",
        "(letrec [x 42 f (fn () x) y x] y)",
    ] {
        let (lowerer, hir) = make_lowerer(body);
        let reads = cell_only_reads(&hir, &lowerer.region_info);
        assert!(
            !reads.is_empty(),
            "precondition: a read of `x` or `y` names only the cell region in {body}",
        );
        for p in reads {
            let v = unsafe { &*p };
            assert_eq!(
                lowerer.coalescible_solver_region(v),
                None,
                "a read naming only a compiled cell's region must NOT coalesce ({body})",
            );
        }
    }
}
