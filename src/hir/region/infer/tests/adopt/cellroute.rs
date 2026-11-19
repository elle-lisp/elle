// audited: 2026-11-19
//! Pins the adopt of a capture cell whose binding's release is routed through it: refused past the closure's drop, kept under it.
//!
//! docs/impl/region/adopt.md
//! docs/impl/region/cells.md

use super::*;

/// The one compiled capture cell a single-captured-binding shape mints, and every
/// capture adopt the map emits.
fn sole_cell_and_capture_adopts(
    info: &RegionInfo,
    edges: &ownership::AdoptEdges,
) -> (Region, Vec<(Region, Region)>) {
    let cells: Vec<Region> = info
        .begin_cell_regions
        .values()
        .flatten()
        .map(|&(_, r)| r)
        .collect();
    assert_eq!(
        cells.len(),
        1,
        "precondition: `v` is the one captured binding; cells={cells:?}",
    );
    let adopts: Vec<(Region, Region)> = edges.capture.values().flatten().copied().collect();
    (cells[0], adopts)
}

#[test]
fn cell_routed_release_past_the_root_refuses_the_adopt() {
    // A top-level `letrec` binds `v`, and `thunk` captures it, so `v` lives in a
    // compiled cell. Its init is a call result, so the frame releases that result by
    // value THROUGH the cell's slot: the release loads the cell and unwraps it
    // (docs/impl/region/adopt.md). `thunk`'s last use is the spawn, so the closure's
    // region dies there. The `%get` reads `v` before the spawn, but `list` consumes its
    // result after it, so `v`'s array lives to `list`, and the routed release with it
    // (docs/impl/region/borrow.md). A cell adopted into the closure would be freed by the
    // closure's subtree drop before the routed release unwraps it, so the obligation
    // refuses the adopt: no capture adopt names the cell.
    //
    // Counterfactual: the `%get`'s consumer raises the cell's tight last use to
    // `list`, past the spawn (docs/impl/region/borrow.md), and that refuses the cell.
    // An obligation that read the cell's use at the `%get` alone admits it.
    let (_hir, info, edges) = adopt_edges(
        "(letrec [v (array (array 1) 3) \
                  r (let [thunk (fn [] (first v))] \
                      (list (%get v 0) (sys/thread-state (sys/spawn-vm thunk))))] \
           r)",
    );
    let (cell, adopts) = sole_cell_and_capture_adopts(&info, &edges);
    assert!(
        !adopts.iter().any(|&(member, _)| member == cell),
        "the cell r{} is read by a release the closure's drop does not post-dominate, so \
         it must stay Shared; capture adopts={:?}",
        cell.0,
        adopts,
    );
}

#[test]
fn cell_routed_release_needs_no_bound_of_its_own() {
    // The walk records the routed region as the cell's content, `cell ⊇ routed`, so a
    // subtree holding the cell holds it as a member the cell owns, which keeps its own
    // release. The edit drops the cell-route pairs and moves that release past the
    // closure's drop, to the end of the program. The member obligation alone then
    // refuses the adopt the shape otherwise keeps.
    //
    // Counterfactual: an obligation that bounded a cell-owned member by its tight use,
    // as it bounds a capture-adopted one, admits the cell.
    let (_hir, info, edges) = adopt_edges_edited(
        "(letrec [v (array 1 2 3) thunk (fn [] (first v))] (thunk))",
        |hir, info| {
            let routes: Vec<(Region, Region)> = info.cell_route_regions.drain().collect();
            assert!(
                !routes.is_empty(),
                "precondition: the shape routes a release through the cell"
            );
            for (cell, routed) in routes {
                assert!(
                    info.containment_edges
                        .iter()
                        .any(|&(_, s, d)| s == routed && d == cell),
                    "the walk records r{} as the content of the cell r{}",
                    routed.0,
                    cell.0,
                );
                info.region_data
                    .get_mut(&routed)
                    .expect("the routed region has release data")
                    .lifetime_point = hir.id;
            }
        },
    );
    let (cell, adopts) = sole_cell_and_capture_adopts(&info, &edges);
    assert!(
        !adopts.iter().any(|&(member, _)| member == cell),
        "the routed region's own bound refuses the cell r{}; capture adopts={:?}",
        cell.0,
        adopts,
    );
}

#[test]
fn cell_routed_release_under_the_root_keeps_the_adopt() {
    // The bound is a bound, not a blanket refusal: here `thunk` is a binding of the
    // same letrec whose last use is `(thunk)`, and `v`'s routed release rides the
    // binding chain to that same call. The closure's drop post-dominates the routed
    // release, so the cell is adopted into the closure as before.
    //
    // Counterfactual: a rule that refused every cell with a value-routed init would
    // leave the plain bind-then-capture shape Shared and the refusal above vacuous.
    let (_hir, info, edges) =
        adopt_edges("(letrec [v (array 1 2 3) thunk (fn [] (first v))] (thunk))");
    let (cell, adopts) = sole_cell_and_capture_adopts(&info, &edges);
    assert!(
        adopts.iter().any(|&(member, _)| member == cell),
        "the cell r{} is read by no release past the closure's drop, so it keeps its \
         capture adopt; capture adopts={:?}",
        cell.0,
        adopts,
    );
}

/// `v` a `letrec` binds, captured by `thunk` and released through its cell, with no
/// read of `v` after the spawn. The routed release lands at the spawn, where the
/// closure dies, so the closure's drop post-dominates it and the cell keeps its
/// capture adopt. The lowerer releases the routed value ahead of the cell, and the
/// cell ahead of its owner.
///
/// Counter-factual: the routed release was held to the end of `r`'s initializer,
/// past the spawn. The routed region, a member of the cell, then refused the
/// subtree by its own bound, and the cell was released on its own.
#[test]
fn cell_routed_release_at_the_spawn_keeps_the_adopt() {
    let (_hir, info, edges) = adopt_edges(
        "(letrec [v (array 1 2 3) \
                  r (let [thunk (fn [] (first v))] \
                      (let [h (sys/spawn-vm thunk)] (sys/thread-state h)))] \
           r)",
    );
    let (cell, adopts) = sole_cell_and_capture_adopts(&info, &edges);
    assert!(
        adopts.iter().any(|&(member, _)| member == cell),
        "the cell r{} is adopted by the closure that captures it; capture adopts={:?}",
        cell.0,
        adopts,
    );
}
