// audited: 2026-11-14
//! A branch arm's compensation and a relocation's replica take the route the region's own `decref_point` release takes.
//!
//! src/lir/lower/AGENTS.md
//! docs/impl/region/compensate.md
//! docs/impl/region/replicate.md
//!
//! No shape reaches most of these routes inside an arm, so each test edits the
//! analysis at the `make_lowerer_with` seam. A compensation test marks `t`'s
//! region the way the route reads it, and names that region in a compensation
//! at the `7` that follows its last use. A compensation's cost is what it adds
//! to the same lowering without it.

use super::armexit::{assert_letrec_closure_replicated, LETREC_UNDER_BRANCH};
use super::*;
use crate::hir::region::{Region, RegionInfo};

#[test]
fn a_mutated_closure_a_counted_cell_stores_is_still_replicated() {
    // The cell's counted reference exempts the value from the mutated refusal,
    // so the replica reads the value route the `decref_point` reads.
    // Counter-factual: a replica that refuses every mutated region keeps the
    // id route, which does not replicate, and every arm strands the closure.
    // The edit marks the init of each `letrec` nested in a lambda: `go`'s.
    fn nested_letrec_inits(h: &Hir, in_lambda: bool, info: &RegionInfo, out: &mut Vec<Region>) {
        if let HirKind::Letrec { bindings, .. } = &h.kind {
            if in_lambda {
                out.extend(
                    bindings
                        .iter()
                        .filter_map(|(_, init)| info.alloc_region.get(&init.id)),
                );
            }
        }
        let in_lambda = in_lambda || matches!(h.kind, HirKind::Lambda(_));
        h.for_each_child(|c| nested_letrec_inits(c, in_lambda, info, out));
    }
    let (mut lowerer, hir) = make_lowerer_with(LETREC_UNDER_BRANCH, |info, hir| {
        let mut inits = Vec::new();
        nested_letrec_inits(hir, false, info, &mut inits);
        assert!(!inits.is_empty(), "`go`'s init allocates a region");
        for r in inits {
            info.mutated_binding_value_regions.insert(r);
            info.cell_stored_regions.insert(r);
        }
    });
    assert_letrec_closure_replicated(&lowerer.lower(&hir).expect("lower"));
}

/// `t`'s region is a call result its `let` binder stores in a slot, and the
/// `7` is a node no other release names.
const SHAPE: &str = "(begin (def p (fn (g) nil)) \
     (def w (fn (q xs) (let [t (xs 0)] (begin (q t) 7)))) \
     (w p [1]))";

/// Which compensation names `t`'s region at the `7`.
#[derive(Clone, Copy, Debug)]
enum Compensation {
    /// The head route, `branch_compensation`, which runs before the node.
    Head,
    /// The tail route, `branch_arm_decrefs`, which runs after the node's own
    /// releases.
    Tail,
}

const BOTH: [Compensation; 2] = [Compensation::Head, Compensation::Tail];

/// An edit of the analysis, handed the tree and `t`'s region.
type Mark = fn(&mut RegionInfo, &Hir, Region);

/// The id of the `7` node under `h`, if `h` holds it.
fn seven_in(h: &Hir) -> Option<HirId> {
    if matches!(h.kind, HirKind::Int(7)) {
        return Some(h.id);
    }
    let mut found = None;
    h.for_each_child(|c| found = found.or_else(|| seven_in(c)));
    found
}

fn seven(hir: &Hir) -> HirId {
    seven_in(hir).expect("the shape holds a 7")
}

/// The region of `t`'s init: the call a `let` inside a lambda binds, whose
/// body holds the `7`.
fn t_region(info: &RegionInfo, hir: &Hir) -> Region {
    fn walk(info: &RegionInfo, h: &Hir, in_lambda: bool, out: &mut Vec<Region>) {
        if let HirKind::Let { bindings, body } = &h.kind {
            if in_lambda && seven_in(body).is_some() {
                for (_, init) in bindings {
                    if let (HirKind::Call { .. }, Some(&r)) =
                        (&init.kind, info.alloc_region.get(&init.id))
                    {
                        out.push(r);
                    }
                }
            }
        }
        let in_lambda = in_lambda || matches!(h.kind, HirKind::Lambda(_));
        h.for_each_child(|c| walk(info, c, in_lambda, out));
    }
    let mut out = Vec::new();
    walk(info, hir, false, &mut out);
    let [r] = out.as_slice() else {
        panic!("expected one call `t` binds, found {out:?}");
    };
    *r
}

/// How many `DecrefValueRegion` and `AdoptIntoActivation` instructions `w`
/// holds once `mark` has edited `t`'s region and `comp` has named it.
fn releases(mark: Mark, comp: Option<Compensation>) -> (usize, usize) {
    let (mut lowerer, hir) = make_lowerer_with(SHAPE, |info, hir| {
        let r = t_region(info, hir);
        mark(info, hir, r);
        let at = seven(hir);
        match comp {
            Some(Compensation::Head) => info.branch_compensation.entry(at).or_default().push(r),
            Some(Compensation::Tail) => info.branch_arm_decrefs.entry(at).or_default().push(r),
            None => {}
        }
    });
    let module = lowerer.lower(&hir).expect("lower");
    let w = named(&module, "w");
    (
        func_count(w, |i| matches!(i, InstrRef::DecrefValueRegion { .. })),
        func_count(w, |i| matches!(i, InstrRef::AdoptIntoActivation { .. })),
    )
}

/// What `comp` adds to `w` once `mark` has run: its value releases and its
/// adopts.
fn added(mark: Mark, comp: Compensation) -> (usize, usize) {
    let (decrefs, adopts) = releases(mark, Some(comp));
    let (base_decrefs, base_adopts) = releases(mark, None);
    (decrefs - base_decrefs, adopts - base_adopts)
}

#[test]
fn an_unmarked_compensation_releases_by_value() {
    // The control: the edit reaches the lowerer, and adds one value release.
    for comp in BOTH {
        assert_eq!(added(|_, _, _| {}, comp), (1, 0), "{comp:?}");
    }
}

#[test]
fn a_compensation_releases_a_mutated_value_a_counted_cell_stores() {
    // The `decref_point` releases such a value through the producer's slot,
    // because the cell holds a counted reference of its own. Counter-factual:
    // an arm that refuses every mutated region strands the value on its path.
    for comp in BOTH {
        let mark: Mark = |info, _, r| {
            info.mutated_binding_value_regions.insert(r);
            info.cell_stored_regions.insert(r);
        };
        assert_eq!(added(mark, comp), (1, 0), "{comp:?}");
    }
}

#[test]
fn a_compensation_adopts_a_transfer_consumer() {
    // The `decref_point` adopts the value into the activation, which consumes
    // the region's whole count. Counter-factual: a plain decref on the arm
    // drops one reference, and the stuck back edge of a returned cycle keeps
    // the rest.
    for comp in BOTH {
        let mark: Mark = |info, _, r| {
            info.transfer_adopt_regions.insert(r);
        };
        assert_eq!(added(mark, comp), (0, 1), "{comp:?}");
    }
}

#[test]
fn a_compensation_takes_no_reassigned_slot() {
    // A reassigned fn-local slot holds a later value by the release, so the
    // `decref_point` takes no route through it. Counter-factual: an arm that
    // loads the slot frees whatever was last assigned there.
    //
    // The trap: functionalization gives each `assign` in straight-line code a
    // fresh binding, so the edit marks `t`'s binding reassigned at the seam.
    fn let_bindings(h: &Hir, out: &mut Vec<Binding>) {
        if let HirKind::Let { bindings, .. } = &h.kind {
            out.extend(bindings.iter().map(|(b, _)| *b));
        }
        h.for_each_child(|c| let_bindings(c, out));
    }
    for comp in BOTH {
        let mark: Mark = |info, hir, _| {
            let mut lets = Vec::new();
            let_bindings(hir, &mut lets);
            info.reassigned_local_bindings.extend(lets);
        };
        assert_eq!(added(mark, comp), (0, 0), "{comp:?}");
    }
}
