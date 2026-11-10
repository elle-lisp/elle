// audited: 2026-11-09
//! Pins where a cell the file's letrec mints is released: at its binding's last use, once, and never at the end of the file.
//!
//! docs/impl/region/cells.md
//! docs/impl/region/lastuse.md

use super::*;
use crate::hir::liveness::{compute_order, Order};

/// A form after every shape, so a release at the end of the file reads as
/// later than the trailing form's init.
const TRAILER: &str = " (def tail-mark (array 9)) (length tail-mark)";

/// The release point of `r`.
fn release_of(info: &RegionInfo, r: Region) -> HirId {
    info.region_data
        .get(&r)
        .unwrap_or_else(|| panic!("r{} has a release", r.0))
        .decref_point
}

/// The file letrec's node and the init of its `tail-mark` binding.
fn file_letrec_and_trailer(hir: &Hir, arena: &BindingArena) -> (HirId, HirId) {
    let HirKind::Letrec { bindings, .. } = &hir.kind else {
        panic!("the file compiles to one letrec");
    };
    let tail = bindings
        .iter()
        .find(|(b, _)| arena.get(*b).name == crate::value::SymbolId::of("tail-mark"))
        .map(|(_, init)| init.id)
        .expect("the trailing form's binding");
    (hir.id, tail)
}

/// Post-order intervals `[low, index]`, one per node.
type Spans = Vec<(u32, u32)>;

/// Every loop node and every branch arm, each as its post-order interval.
fn loops_and_arms(hir: &Hir, order: &Order) -> (Spans, Spans) {
    fn span(h: &Hir, order: &Order) -> (u32, u32) {
        (order.low(&h.id).unwrap_or(0), order[&h.id])
    }
    fn walk(h: &Hir, order: &Order, loops: &mut Spans, arms: &mut Spans) {
        match &h.kind {
            HirKind::While { .. } | HirKind::Loop { .. } => loops.push(span(h, order)),
            HirKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                arms.push(span(then_branch, order));
                arms.push(span(else_branch, order));
            }
            _ => {}
        }
        h.for_each_child(|c| walk(c, order, loops, arms));
    }
    let (mut loops, mut arms) = (Vec::new(), Vec::new());
    walk(hir, order, &mut loops, &mut arms);
    (loops, arms)
}

/// `v` is read last by the call `(t)`, through the closure that captures it.
/// Both its cell and its array must be released there, before the trailing
/// form runs.
///
/// Counter-factual: the cell takes the letrec's last use as its base, and the
/// array's last use is solved from the letrec node down. Both then wait for
/// the end of the file.
#[test]
fn a_captured_def_is_released_before_the_file_ends() {
    let src = format!("(def v (array 1 2 3)) (defn t [] (length v)) (t){TRAILER}");
    let (hir, arena, info) = pipeline(&src);
    let order = compute_order(&hir);
    let (letrec, tail) = file_letrec_and_trailer(&hir, &arena);
    let v = find_binding_by_name(&hir, "v", &arena).expect("binding v");
    let cell = info
        .begin_cell_regions
        .get(&letrec)
        .into_iter()
        .flatten()
        .find(|(b, _)| *b == v)
        .map(|&(_, r)| r)
        .expect("the file letrec mints v's cell");
    let mut arrays = find_calls_to_primitive(&hir, "array", &arena);
    arrays.sort_by_key(|id| order[id]);
    let array = info.alloc_region[&arrays[0]];
    for (what, r) in [("cell", cell), ("array", array)] {
        let at = release_of(&info, r);
        assert!(
            order[&at] < order[&tail],
            "v's {what} r{} is released at @{}, after the trailing form @{}",
            r.0,
            at.0,
            tail.0
        );
    }
}

/// Every shape a top-level `def` takes: plain, inside a `begin`, read after a
/// skippable `def`, stored inside a loop, destructured, reassigned, and read
/// through a borrow that outlives the binding's own reads.
///
/// For each cell the file letrec mints, the release lands before the trailing
/// form, outside every loop and branch arm (the cell is minted once), and at
/// or after any value release routed through the cell's slot.
///
/// Counter-factual: a release placed at a store inside the loop would run once
/// per pass and free the one cell on the first; a release before a routed
/// value release would unwrap a freed cell.
#[test]
fn every_file_cell_is_released_once_before_the_file_ends() {
    let shapes = [
        "(def v (array 1 2 3)) (defn t [] (length v)) (t)",
        "(begin (def v (array 1 2 3)) (defn t [] (length v)) (t))",
        "(when true (def v (array 1 2))) (defn t [] (length v)) (t)",
        "(def @k 0) (while (< k 3) (def v (array k)) (assign k (+ k 1))) \
         (defn t [] (length v)) (t)",
        "(def [a b] [(array 1) (array 2)]) (defn t [] (length a)) (t)",
        "(def @v (array 1)) (defn t [] (length v)) (t) (assign v (array 2)) (t)",
        "(def v (array (array 7))) (defn t [] v) (def e (get v 0)) (t) (length e)",
    ];
    for shape in shapes {
        let src = format!("{shape}{TRAILER}");
        let (hir, arena, info) = pipeline(&src);
        let order = compute_order(&hir);
        let (letrec, tail) = file_letrec_and_trailer(&hir, &arena);
        let (loops, arms) = loops_and_arms(&hir, &order);
        let cells: Vec<Region> = info
            .begin_cell_regions
            .get(&letrec)
            .into_iter()
            .flatten()
            .map(|&(_, r)| r)
            .collect();
        assert!(!cells.is_empty(), "{shape}: the file letrec mints a cell");
        for cell in cells {
            let at = order[&release_of(&info, cell)];
            assert!(
                at < order[&tail],
                "{shape}: cell r{} is released after the trailing form",
                cell.0
            );
            // The lowerer emits a node's releases after the node, so a release
            // at a loop node runs after the loop, and one at an arm's own node
            // still runs inside the arm.
            for &(lo, hi) in &loops {
                assert!(
                    !(lo <= at && at < hi),
                    "{shape}: cell r{} is released inside a loop",
                    cell.0
                );
            }
            for &(lo, hi) in &arms {
                assert!(
                    !(lo <= at && at <= hi),
                    "{shape}: cell r{} is released inside a branch arm",
                    cell.0
                );
            }
            for (&routed_cell, &value) in &info.cell_route_regions {
                if routed_cell == cell {
                    assert!(
                        at >= order[&release_of(&info, value)],
                        "{shape}: cell r{} is released before r{}, routed through it",
                        cell.0,
                        value.0
                    );
                }
            }
        }
    }
}
