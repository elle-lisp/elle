// audited: 2026-11-14
//! Each branch arm that leaves through a tail call takes a replica of a release emitted past the merge.
//!
//! docs/impl/region/mechanism.md
//! docs/impl/region/replicate.md

use super::*;
use crate::hir::region::{Region, RegionInfo};

/// For the first function with two `TailCall`-bearing blocks — a branch whose
/// arms each make one — the local slots each block releases BEFORE its call and
/// those it releases after.
///
/// Reading by SLOT rather than by instruction count is what makes these pins
/// specific: an arm carries the replicated release of *every* region the merge
/// releases, so "some release precedes the call" says nothing about which.
fn branch_arm_release_slots(module: &FrozenModule) -> Vec<(Vec<u16>, Vec<u16>)> {
    for f in functions(module) {
        let arms: Vec<(Vec<u16>, Vec<u16>)> = tail_call_blocks_of(f)
            .map(|(b, at)| {
                let (mut before, mut after) = (Vec::new(), Vec::new());
                for (idx, slot) in value_releases(b.instrs()) {
                    if idx < at {
                        before.push(slot);
                    } else {
                        after.push(slot);
                    }
                }
                (before, after)
            })
            .collect();
        if arms.len() >= 2 {
            return arms;
        }
    }
    Vec::new()
}

/// The local slot the first `MakeClosure` of the first function that leaves
/// through a `TailCall` is stored into.
///
/// A `letrec`-bound closure's release names that slot, and reading the slot off
/// the emission rather than counting locals is what keeps the pins below specific:
/// which index a binder gets depends on how many parameters and ANF temporaries
/// precede it.
fn tail_calling_functions_closure_slot(module: &FrozenModule) -> Option<u16> {
    for f in functions(module) {
        if tail_call_blocks_of(f).next().is_none() {
            continue;
        }
        let mut made: Option<Reg> = None;
        for i in flat_instrs(f) {
            match i {
                InstrRef::MakeClosure { dst, .. } => made = Some(dst),
                InstrRef::StoreLocal { slot, src } if Some(src) == made => return Some(slot),
                _ => {}
            }
        }
    }
    None
}

/// For the first function that BRANCHES and builds a closure, that closure's
/// region and slot beside the region ids the same function releases and the slots
/// it releases by value.
///
/// Scoped to one function on purpose: every compile releases some closure region
/// by id somewhere (each top-level `defn`'s own), so a module-wide reading would
/// satisfy a route pin without measuring the subject.
fn branching_functions_closure_routes(
    module: &FrozenModule,
) -> Option<(StaticRegion, u16, Vec<StaticRegion>, Vec<u16>)> {
    for f in functions(module) {
        if !f
            .view()
            .blocks()
            .any(|b| matches!(b.terminator(), Terminator::Branch { .. }))
        {
            continue;
        }
        let mut made: Option<(Reg, StaticRegion)> = None;
        let mut subject: Option<(StaticRegion, u16)> = None;
        let mut from_slot: std::collections::HashMap<Reg, u16> = std::collections::HashMap::new();
        let (mut by_id, mut by_value) = (Vec::new(), Vec::new());
        for i in flat_instrs(f) {
            match i {
                InstrRef::MakeClosure { dst, region, .. } => made = Some((dst, region)),
                InstrRef::StoreLocal { slot, src } => {
                    if let Some((reg, region)) = made {
                        if reg == src && subject.is_none() {
                            subject = Some((region, slot));
                        }
                    }
                }
                InstrRef::LoadLocal { dst, slot } => {
                    from_slot.insert(dst, slot);
                }
                InstrRef::DecrefRegion { region_id } => by_id.push(region_id),
                InstrRef::DecrefValueRegion { src } => {
                    by_value.extend(from_slot.get(&src).copied())
                }
                _ => {}
            }
        }
        if let Some((region, slot)) = subject {
            return Some((region, slot, by_id, by_value));
        }
    }
    None
}

/// A `letrec` body whose tail is a branch, each arm of which leaves through a
/// frame-replacing callee. `go` is last called inside the then-arm.
const LETREC_UNDER_BRANCH: &str = "(begin (def s (fn () 0)) (def s2 (fn () 1)) \
     (def f (fn (n t) \
       (letrec [go (fn (m) (if (%lt m 1) 0 (go (%sub m 1))))] \
         (if t (begin (go n) (s)) (s2))))) \
     (f 3 true))";

/// Assert that each arm of `module`'s branch releases the letrec closure's
/// slot ahead of its `TailCall`.
fn assert_letrec_closure_replicated(module: &FrozenModule) {
    let slot = tail_calling_functions_closure_slot(module)
        .expect("the letrec binder stores its closure into a slot");
    let arms = branch_arm_release_slots(module);
    assert_eq!(arms.len(), 2, "the branch lowers to one TailCall per arm");
    for (before, after) in &arms {
        assert!(
            before.contains(&slot),
            "an arm takes no copy of the letrec closure's release \
             (slot={slot}, before={before:?}, after={after:?}) — dead on that \
             arm's closure path, one closure and env per call",
        );
    }
}

#[test]
fn a_letrec_closure_under_a_branch_tail_is_replicated_into_every_arm() {
    // The branch window moves `go`'s release onto the branch, a merge no path
    // arrives at. A replica counts once only where the run nil-stamps the slot
    // it read, which this region's DEFAULT release by region id does not — so
    // it takes the value route of the slot its `letrec` binder recorded
    // (docs/impl/region/replicate.md).
    assert_letrec_closure_replicated(&compile_to_lir(LETREC_UNDER_BRANCH));
}

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

#[test]
fn a_letrec_closure_no_arm_strands_keeps_its_release_by_id() {
    // The narrowness of the reroute, and the reason the id route is the default:
    // with no arm leaving through a callee the merge is a point every path
    // reaches, so one instruction does the work of four and the release stays a
    // `DecrefRegion` naming the closure's own region. Arms that do leave through a
    // callee strand nothing either when `go` is last called before the branch.
    // Counter-factual: a `letrec` init last used at the `letrec` node puts that
    // release past the merge, and the second shape takes the value route.
    for arms in ["(if t 4 5)", "(if t (s) (s2))"] {
        let module = compile_to_lir(&format!(
            "(begin (def s (fn () 0)) (def s2 (fn () 1)) (def f (fn (n t) \
               (letrec [go (fn (m) (if (%lt m 1) 0 (go (%sub m 1))))] \
                 (go n) \
                 {arms}))) \
             (f 3 true))"
        ));
        let (region, slot, by_id, by_value) = branching_functions_closure_routes(&module)
            .expect("the letrec binder stores its closure into a slot");
        assert!(
            by_id.contains(&region) && !by_value.contains(&slot),
            "the letrec closure did not keep its release by id under {arms} \
             (region={region}, slot={slot}, by_id={by_id:?}, by_value={by_value:?}) — \
             the value route is for the releases a branch's frame-exiting arms make \
             the relocation replicate, not for every release",
        );
    }
}

#[test]
fn stranded_param_release_is_replicated_into_every_branch_arm() {
    // The release lands past the MERGE, which each arm leaves through a
    // frame-replacing tail call — so the merge copy alone reaches neither path.
    // The merge's inherited relocation points put a copy ahead of each arm's
    // `TailCall` (docs/impl/region/replicate.md; the `tail-frame-exit-arms` probe).
    // `x` is the first parameter,
    // hence local slot 0.
    let module = compile_to_lir(
        "(begin (def s (fn () 0)) (def s2 (fn () 1)) \
         (def f (fn (x t) (if t (s) (s2)))) (f (list 1 2) true))",
    );
    let arms = branch_arm_release_slots(&module);
    assert_eq!(arms.len(), 2, "the body lowers to one TailCall per arm");
    for (before, after) in &arms {
        assert!(
            before.contains(&0),
            "an arm's copy of the stranded parameter's release is missing \
             (before={before:?}, after={after:?}) — dead on that arm's closure path",
        );
    }
}

#[test]
fn moved_argument_takes_no_replica_in_the_arm_that_moves_it() {
    // The exemption is read PER point. `x` (local slot 0) is the then-arm call's
    // argument, so that arm takes no replica of `x`'s release — the callee's
    // owned-parameter release is what frees it there. The same arm still takes a
    // replica of `t`'s release, and the merge's other point, whose call names
    // nothing, takes one of `x`'s.
    let module = compile_to_lir(
        "(begin (def s (fn (a) a)) (def s2 (fn () 1)) \
         (def f (fn (x t) (if t (s x) (s2)))) (f (list 1 2) true))",
    );
    let arms = branch_arm_release_slots(&module);
    assert_eq!(arms.len(), 2, "the body lowers to one TailCall per arm");
    let (moving_before, moving_after) = &arms[0];
    assert!(
        !moving_before.contains(&0),
        "the moved argument's release was replicated ahead of the arm's TailCall \
         (before={moving_before:?}) — that release IS the ownership move",
    );
    assert!(
        !moving_after.contains(&0),
        "the moved argument's release was left in the arm's dead block \
         (after={moving_after:?}) — nothing there runs",
    );
    assert!(
        moving_before.contains(&1),
        "the arm took no replica at all (before={moving_before:?}) — the exemption \
         is read per REGION at each point, not per point",
    );
    let (other_before, _) = &arms[1];
    assert!(
        other_before.contains(&0),
        "the sibling arm, whose call names nothing, did not take the replica \
         (before={other_before:?})",
    );
}
