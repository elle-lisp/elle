use super::*;

// ── ownership inference: the lifetime obligation is STRUCTURAL, not numeric ──────
//
// The root's single decref subtree-drops every member, so it must post-dominate every
// member's last use. That is decided by structural post-dominance over the scope tree
// (`region::infer::postdom::drop_post_dominates`, `EmitMode::Adopt`), NOT by a numeric
// `ord(member) <= ord(root)` (region/adopt.md § "The lifetime obligation the root carries"):
// across a branch arm or a loop back-edge, post-order index is not a post-dominance
// proxy. These two pins exercise the cases where the numeric test admits but structural
// post-dominance refuses — both straight-line-admitted by the old `ord` compare, both
// refused to Shared (the always-legal baseline) now.

#[test]
fn adopt_edges_refuses_loop_enclosed_member() {
    // A container `root` and a member `a`, both built inside a `while` body, with `a`
    // pushed into `root` (the edge `root ⊇ a`) sequenced BEFORE `root`'s last use. The
    // externally-unique subtree {root, a} is admitted, and `a`'s owner is the root.
    //
    // The lifetime obligation: the root's free re-runs every iteration. `a` is
    // STORE-adopted (it keeps its own DecrefValueRegion), so a loop enclosing the root's
    // free is the cross-iteration hazard the `EmitMode::Adopt` loop clause refuses — the
    // root's drop frees `a` at iteration K and the deref re-runs at K+1. So no adopt is
    // emitted; the subtree stays Shared.
    //
    // Counterfactual: the prior NUMERIC obligation admitted this — `a`'s last use (the
    // push) has a SMALLER post-order index than the root's last use, both inside the
    // loop body, so `ord(member) <= ord(root)` passed and an adopt `(a, root)` was
    // emitted. Structural post-dominance refuses it (a `While`/`Loop` encloses the
    // root's free). This assertion was RED before the structural cut.
    let (_, info, edges) = adopt_edges(
        "(let [c 1] (while c (let [root (@array) a (@array)] \
           (begin (%array-push root a) (%array-push root 7) nil))))",
    );
    let (root, members) = container_root_and_members(&info);
    let non_root: Vec<Region> = members.into_iter().filter(|m| *m != root).collect();
    assert_eq!(
        non_root.len(),
        1,
        "precondition: the shape has one member `a`; got {:?}",
        non_root
    );
    let adopts: Vec<(Region, Region)> = edges.store.values().flatten().copied().collect();
    assert!(
        adopts.is_empty(),
        "a member whose owner's free is re-run by an enclosing loop must emit no adopt — \
         the subtree stays Shared (root r{}, member {:?}); got {:?}",
        root.0,
        non_root,
        adopts,
    );
}

#[test]
fn adopt_edges_refuses_branch_separated_member() {
    // A container `root` directly holds a member `a`, but `a` is pushed into `root` only
    // inside an `if` arm, while `root`'s last use is AFTER the `if`. The subtree {root, a}
    // is admitted and `a`'s owner is the root.
    //
    // The lifetime obligation: the `if` encloses `a`'s last use (the conditional push) but
    // NOT the root's free, so a control node separates them — the straight-line "executes
    // before" that an `ord` compare would assert does not hold across the branch. Structural
    // post-dominance refuses (no adopt), leaving the subtree Shared (the always-legal
    // baseline; a conservative refusal — refusing a control-flow-separated member is sound).
    //
    // Counterfactual: the prior numeric obligation admitted it — `a`'s push (in the
    // then-arm) has a smaller post-order index than the root's later use, so
    // `ord(member) <= ord(root)` passed and an adopt was emitted. RED before the cut.
    let (_, info, edges) = adopt_edges(
        "(begin (let [c 1 root (@array) a (@array)] \
           (begin (if c (%array-push root a) nil) (%array-push root 7) nil)) \
         nil)",
    );
    let (root, members) = container_root_and_members(&info);
    let non_root: Vec<Region> = members.into_iter().filter(|m| *m != root).collect();
    assert_eq!(
        non_root.len(),
        1,
        "precondition: the shape has one member `a`; got {:?}",
        non_root
    );
    let adopts: Vec<(Region, Region)> = edges.store.values().flatten().copied().collect();
    assert!(
        adopts.is_empty(),
        "a member separated from its owner's free by a branch must emit no adopt — the \
         subtree stays Shared (root r{}, member {:?}); got {:?}",
        root.0,
        non_root,
        adopts,
    );
}

#[test]
fn adopt_edges_claims_interior_cycle_member_by_root() {
    // A Fresh container `root` directly holds two members `a` and `b`, which in turn
    // reference EACH OTHER (the interior cycle `a ⊇ b`, `b ⊇ a`). The whole
    // {root, a, b} is an externally-unique Owned subtree (nothing escapes), and the
    // shared-container cut adopts every member DIRECTLY by the root — a flat star — so
    // the root's single decref subtree-drops the cycle, which per-region RC could never
    // collect (region/rules.md Rule 8). The interior a↔b edges carry no adopt.
    //
    // The push order makes `root` the LAST region used (its `%array-push`es come after
    // the cycle is built), so its `decref_point` post-dominates a and b — the lifetime
    // obligation compute_adopt_edges enforces; an interior-outlives-root order would be
    // (correctly) refused.
    //
    // Counterfactual: the prior flat cut refused any subtree with an interior edge whose
    // target is not the root (the a→b / b→a cycle edges), emitting ZERO adopts and
    // leaving the cycle to leak — so this assertion was RED before the cut.
    let (_, info, edges) = adopt_edges(
        "(begin (let [root (@array) a (@array) b (@array)] \
                  (begin (%array-push a b) (%array-push b a) \
                         (%array-push root a) (%array-push root b) nil)) \
                nil)",
    );
    // Precondition: the stores are genuinely funnel calls — they record no
    // `cross_region_refs` containment; only `containment_edges` carries it.
    assert!(
        !info
            .cross_region_refs
            .iter()
            .any(|(site, _, _)| !info.hard_edge_sites.contains(site)),
        "precondition: the funnel stores record no cross_region_refs containment; \
         got {:?}",
        info.cross_region_refs,
    );
    let (root, members) = container_root_and_members(&info);
    let non_root: Vec<Region> = members.into_iter().filter(|m| *m != root).collect();
    assert_eq!(
        non_root.len(),
        2,
        "the shape has two members a, b; got {:?}",
        non_root
    );
    let adopts: Vec<(Region, Region)> = edges.store.values().flatten().copied().collect();
    assert!(
        !adopts.is_empty(),
        "the interior-cycle subtree must emit adopt edges; got none (root r{}, members \
         {:?})",
        root.0,
        non_root
    );
    for m in &non_root {
        assert!(
            adopts.contains(&(*m, root)),
            "member r{} must be adopted directly by the container root r{}; adopts={:?}",
            m.0,
            root.0,
            adopts,
        );
    }
    // A flat star: every adopt's parent IS the root — the interior member↔member cycle
    // edges produce no member-as-parent adopt (which would tangle the owner graph).
    for &(child, parent) in &adopts {
        assert_eq!(
            parent, root,
            "adopt of r{} names parent r{}, but the shared-container cut adopts every \
             member directly by the root r{}; adopts={:?}",
            child.0, parent.0, root.0, adopts,
        );
    }
    // Each adopt is keyed at a funnel call site — the site the value-resolved
    // `AdoptRegion` emits at (`emit_increfs_for` on that Call node; no store opcode
    // exists to key it on).
    for site in edges.store.keys() {
        assert!(
            info.funnel_store_sites.contains_key(site),
            "adopt site {site:?} must be a funnel store call site; funnel sites: {:?}",
            info.funnel_store_sites.keys(),
        );
    }
    // No capture edges in this shape.
    assert!(
        edges.capture.is_empty(),
        "a pure store shape emits no capture adopts; got {:?}",
        edges.capture,
    );
}

#[test]
fn adopt_edges_claims_deep_nesting_by_actual_parent() {
    // Deep nesting: `root` directly holds `a`, and `a` holds `b` — but `root` does NOT
    // directly hold `b` (`root ⊇ a ⊇ b`, no `root ⊇ b`). `b` has no `member → root` edge,
    // so it cannot be adopted directly by the root; instead it is adopted by its ACTUAL
    // parent `a` (`AdoptRegion(a, b)`), and `a` is adopted by the root. The root's single
    // decref subtree-drops the whole chain recursively (`free_runtime_region_pages` walks
    // `owned_children`, pinned by `subtree_drop_is_recursive`), so `b` reclaims with the
    // subtree instead of leaking.
    //
    // Counterfactual: the prior flat cut refused any subtree with a member lacking a
    // direct root edge, emitting ZERO adopts — so this assertion was RED before the
    // deep-nesting cut.
    let (_, info, edges) = adopt_edges(
        "(begin (let [root (@array) a (@array) b (@array)] \
                  (begin (%array-push a b) (%array-push root a) nil)) \
                nil)",
    );
    // Identify the three regions by their edge roles among the funnel-recovered
    // containment edges: root is target-only (`root ⊇ a`), `a` is both a target
    // (`a ⊇ b`) and a source (`root ⊇ a`), `b` is source-only (`a ⊇ b`).
    let mut srcs = rustc_hash::FxHashSet::default();
    let mut dsts = rustc_hash::FxHashSet::default();
    for &(_site, src, dst) in &info.containment_edges {
        srcs.insert(src);
        dsts.insert(dst);
    }
    let root = *dsts
        .iter()
        .find(|r| !srcs.contains(r))
        .expect("target-only root");
    let b = *srcs
        .iter()
        .find(|r| !dsts.contains(r))
        .expect("source-only leaf b");
    let a = *srcs
        .iter()
        .find(|r| **r != b && dsts.contains(r))
        .expect("intermediate a (both source and target)");
    let adopts: rustc_hash::FxHashSet<(Region, Region)> =
        edges.store.values().flatten().copied().collect();
    assert_eq!(
        adopts,
        [(a, root), (b, a)]
            .into_iter()
            .collect::<rustc_hash::FxHashSet<_>>(),
        "deep nesting must adopt `a` by the root and `b` by its actual parent `a` \
         (root r{}, a r{}, b r{}); got {:?}",
        root.0,
        a.0,
        b.0,
        adopts,
    );
}

#[test]
fn adopt_edges_refuses_ambiguous_multiparent() {
    // Soundness boundary (a guard the deep-nesting cut must NOT cross): `b` is held by
    // BOTH `a` and `c` (`a ⊇ b`, `c ⊇ b`), and neither is the root — so `b` has two
    // candidate owners and no `member → root` edge to break the tie. A single-owner
    // forest cannot name which of `a`/`c` frees `b`; adopting `b` by one would leave the
    // other's interior edge un-owned (and a naive "pick any parent" choice is a coin
    // flip on lifetime). So the whole subtree is refused to Shared, the always-legal
    // baseline. (Multi-parent deep nesting is a later cut — it needs an owner that
    // dominates every holder, the same machinery as the cross-fiber step-3 case.)
    let (_, info, edges) = adopt_edges(
        "(begin (let [root (@array) a (@array) c (@array) b (@array)] \
                  (begin (%array-push a b) (%array-push c b) \
                         (%array-push root a) (%array-push root c) nil)) \
                nil)",
    );
    let _ = container_root_and_members(&info);
    let adopts: Vec<(Region, Region)> = edges.store.values().flatten().copied().collect();
    assert!(
        adopts.is_empty(),
        "a member held by two non-root containers (ambiguous owner) must emit no adopt — \
         the subtree stays Shared; got {:?}",
        adopts,
    );
}

#[test]
fn adopt_edges_claims_captured_value_by_closure() {
    // The capture ADOPT cut. A pair `p` captured by a LOCAL closure `c`
    // (called in place, discarded) is interior to `c`'s Owned subtree
    // (`owned_subtree_unifies_local_capture_into_closure`). With **tight last-use for a
    // captured-and-owned value** — `p`'s demise bounded by its owning closure's last CALL
    // (`(c)`, order 8), not the structural capture-use position one step past it (the inner
    // `Let` node, order 9, where the lambda is bound) — the ownership lifetime obligation
    // `member_dp ≤ root_dp` admits the subtree, and `p` is adopted by the closure at its
    // construction site (a CAPTURE adopt: capture records no `cross_region_refs` store site,
    // so the adopt is keyed by the Lambda and rides `AdoptEdges::capture`). The closure's
    // subtree drop then frees `p` at the closure's true death — prompter than the
    // per-region-RC baseline, without extending the closure.
    //
    // Counterfactual: before the tight-last-use fix `p`'s `decref_point` over-extended one
    // step past `c` (the alloc-loop reads the locked-high init last-use; the binding-chain
    // tight value cannot lower it), tripping `member_dp ≤ root_dp`, and the subtree was
    // refused (no adopt). So this `contains((p, closure))` assertion was RED.
    let (hir, info, edges) =
        adopt_edges("(begin (let [p (%pair 1 2)] (let [c (fn [] (length p))] (c))) nil)");
    let p = sole_pair_region(&hir, &info);
    // The capturing closure's region — the sole Lambda's alloc region.
    let mut lam: Option<HirId> = None;
    fn first_lambda(h: &Hir, out: &mut Option<HirId>) {
        if matches!(&h.kind, HirKind::Lambda { .. }) && out.is_none() {
            *out = Some(h.id);
        }
        h.for_each_child(|c| first_lambda(c, out));
    }
    first_lambda(&hir, &mut lam);
    let closure_r = *info
        .alloc_region
        .get(&lam.expect("a Lambda node"))
        .expect("the closure has an alloc region");
    let capture_adopts: Vec<(Region, Region)> = edges.capture.values().flatten().copied().collect();
    assert!(
        capture_adopts.contains(&(p, closure_r)),
        "the captured pair r{} must be adopted by its capturing closure r{} (a CAPTURE \
         adopt) once tight last-use admits the subtree; capture adopts={:?}",
        p.0,
        closure_r.0,
        capture_adopts,
    );
    // A member is adopted through exactly one of the two maps; the capture has no
    // `cross_region_refs` site, so `p` must NOT also ride the store map.
    let store_adopts: Vec<(Region, Region)> = edges.store.values().flatten().copied().collect();
    assert!(
        !store_adopts.iter().any(|&(child, _)| child == p),
        "the captured pair r{} must be adopted via the capture map only, not the store \
         map; store adopts={:?}",
        p.0,
        store_adopts,
    );
}

#[test]
fn adopt_edges_refuses_captured_value_used_after_closure() {
    // The soundness boundary the tight-last-use fix must NOT cross (never extend a captured
    // value's lifetime). `p` is captured by `c` AND used DIRECTLY after the closure dies (`(begin (c)
    // (length p))`). Here `p`'s TRUE last use really is past the closure: the tight last-use
    // is the max of (the closure's last call, the direct outer `(length p)`), which lands
    // after `c`'s death, so the lifetime obligation `member_dp ≤ root_dp` correctly REFUSES
    // the adopt and leaves `p` Shared (the always-legal baseline).
    //
    // Counterfactual against an over-greedy tightening that caps EVERY captured value at its
    // closure's death regardless of later direct uses: that would (wrongly) adopt `p` here
    // and free it under the still-pending outer `(length p)` — a use-after-free. The tight
    // last-use is the binding's resolved last-use over ALL its uses (capture AND direct), so
    // a later direct use keeps the obligation honest. This pin bites the greedy mistake.
    let (hir, info, edges) = adopt_edges(
        "(begin (let [p (%pair 1 2)] (let [c (fn [] (length p))] (begin (c) (length p)))) nil)",
    );
    let p = sole_pair_region(&hir, &info);
    let all_adopts: Vec<(Region, Region)> = edges
        .store
        .values()
        .chain(edges.capture.values())
        .flatten()
        .copied()
        .collect();
    assert!(
        !all_adopts.iter().any(|&(child, _)| child == p),
        "a captured value r{} used after its closure dies must NOT be adopted — its true \
         last use is past the closure, so the lifetime obligation refuses it to Shared; \
         got {:?}",
        p.0,
        all_adopts,
    );
}
