// audited: 2026-11-10
//! Pins the root's compensating releases against its members: an arm that frees the root frees the subtree with it.
//!
//! docs/impl/region/adopt.md
//! docs/impl/region/compensate.md
//!
//! Branch compensation gives a root whose release sits in one arm a release of its
//! own on each sibling arm. That release is a subtree drop too. Post-dominance reads
//! one release point per region, and a use in a sibling arm sequences before the
//! root's release with no control node between them, so the obligation admits it.

use super::*;

/// A closure `dl` captures `seen`, and the else arm calls `dl`. `BODY` is the then
/// arm.
const ARM: &str = "(fn [c] (let [seen (@struct)] \
                     (let [dl (fn [l] (%put seen l true))] \
                       (if c BODY (dl c)))))";

/// The region of the one closure in `hir` that captures a binding: `dl`, not the
/// outer `fn [c]`.
fn capturing_closure_region(hir: &Hir, info: &RegionInfo) -> Region {
    fn walk(h: &Hir, out: &mut Vec<HirId>) {
        if let HirKind::Lambda(lambda) = &h.kind {
            if !lambda.captures.is_empty() {
                out.push(h.id);
            }
        }
        h.for_each_child(|c| walk(c, out));
    }
    let mut lambdas = Vec::new();
    walk(hir, &mut lambdas);
    assert_eq!(lambdas.len(), 1, "the shape has one capturing closure");
    *info
        .alloc_region
        .get(&lambdas[0])
        .expect("the closure has an alloc region")
}

/// The closure, after checking the precondition every pin shares: the subtree
/// `{closure, seen}` is externally unique, and the then arm releases the closure.
fn closure_compensated_on_then_arm(src: &str) -> (Hir, RegionInfo, Region) {
    let (hir, info, owned) = owned_subtrees_with_effects(src);
    let closure = capturing_closure_region(&hir, &info);
    assert!(
        owned.get(&closure).is_some_and(|m| m.len() == 2),
        "precondition: {{closure r{}, seen}} is externally unique; got {owned:?}",
        closure.0,
    );
    assert!(
        info.branch_compensation
            .values()
            .any(|rs| rs.contains(&closure)),
        "precondition: the closure r{} is released at the head of the then arm; got {:?}",
        closure.0,
        info.branch_compensation,
    );
    (hir, info, closure)
}

/// The then arm stores into `seen` after the closure's head release, so the
/// closure's subtree drop frees `seen` under that store. The obligation refuses
/// the subtree to Shared.
///
/// Counter-factual: `seen`'s last use and the closure's release are both in the
/// else arm, so an obligation that reads one point per region admits the adopt,
/// and the then arm faults under guardfree
/// (tests/impl/region-arm-release-adopt-uaf.lisp).
#[test]
fn a_member_used_after_the_roots_arm_release_refuses_the_adopt() {
    let src = ARM.replace("BODY", "(begin (%put seen 2 true) 1)");
    let (_, _, closure) = closure_compensated_on_then_arm(&src);
    let (_, _, edges) = adopt_edges(&src);
    assert!(
        edges.capture.is_empty() && edges.store.is_empty(),
        "a member the then arm uses after the closure r{}'s head release refuses the \
         subtree; got capture={:?} store={:?}",
        closure.0,
        edges.capture,
        edges.store,
    );
}

/// The then arm names neither `seen` nor the closure, so the head release frees
/// the subtree on a path that never uses it again. The adopt stands.
///
/// Counter-factual: a refusal of every compensated root would leave this subtree
/// Shared for no reason.
#[test]
fn a_dead_arm_release_with_no_member_use_keeps_the_adopt() {
    let src = ARM.replace("BODY", "1");
    let (hir, info, closure) = closure_compensated_on_then_arm(&src);
    let (_, _, edges) = adopt_edges(&src);
    let adopts: Vec<(Region, Region)> = edges.capture.values().flatten().copied().collect();
    assert!(
        adopts.len() == 1 && adopts[0].1 == closure,
        "the closure r{} adopts seen; got capture={adopts:?}",
        closure.0,
    );
    assert!(
        closure_captures_region(&hir, &info, adopts[0].0, closure),
        "the adopt has a capture to reload its member from",
    );
}

/// The first node of the then arm's `begin`, where the tail pin keys its release.
fn first_then_node(hir: &Hir) -> HirId {
    fn find(h: &Hir, out: &mut Option<HirId>) {
        if let HirKind::If { then_branch, .. } = &h.kind {
            let HirKind::Begin(exprs) = &then_branch.kind else {
                panic!("the then arm is a begin; got {:?}", then_branch.kind);
            };
            assert_eq!(
                exprs.len(),
                2,
                "ANF splits the arm into its first store and the rest"
            );
            *out = Some(exprs[0].id);
        }
        h.for_each_child(|c| find(c, out));
    }
    let mut out = None;
    find(hir, &mut out);
    out.expect("the shape has an if")
}

/// The then arm releases the closure after its first node, as the tail route
/// releases a root after its last use in an arm, and stores into `seen` again
/// after that. No shape gives an Owned root a tail release yet, so the pin moves
/// the head release there.
///
/// Counter-factual: an obligation that read the head releases alone would admit
/// the adopt, and the store would write a freed table.
#[test]
fn a_member_used_after_the_roots_tail_release_refuses_the_adopt() {
    let src = ARM.replace("BODY", "(begin (%put seen 3 true) (%put seen 2 true) 1)");
    closure_compensated_on_then_arm(&src);
    let (_, info, edges) = adopt_edges_edited(&src, |hir, info| {
        let closure = capturing_closure_region(hir, info);
        for rs in info.branch_compensation.values_mut() {
            rs.retain(|&r| r != closure);
        }
        let first = first_then_node(hir);
        info.branch_arm_decrefs
            .entry(first)
            .or_default()
            .push(closure);
    });
    assert!(
        info.branch_arm_decrefs.values().flatten().count() == 1
            && info.branch_compensation.values().all(|rs| rs.is_empty()),
        "precondition: the closure's one compensating release is the tail release; got \
         head={:?} tail={:?}",
        info.branch_compensation,
        info.branch_arm_decrefs,
    );
    assert!(
        edges.capture.is_empty() && edges.store.is_empty(),
        "a member the then arm uses after the closure's tail release refuses the \
         subtree; got capture={:?} store={:?}",
        edges.capture,
        edges.store,
    );
}
