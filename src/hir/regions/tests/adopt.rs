use super::*;

// ── ownership inference: adopt-edge emission (compute_adopt_edges, step 4) ──────
//
// `regions::ownership::compute_adopt_edges` is the map the lowerer consumes: for each
// externally-unique Owned subtree (the lifetime obligation + no merge overlap), it
// emits one `AdoptRegion(owner, member)` per non-root member at the member's
// `member → owner` containment store site. Each member is adopted by its **actual
// parent**: the root when a direct `member → root` edge exists (a flat star, the common
// case — an interior member↔member cycle among root's direct children rides along,
// reclaimed by the root's subtree drop with no adopt of its own), else the single
// interior container that holds it (multi-level nesting `root ⊇ a ⊇ b`: `a` adopts `b`,
// the root adopts `a`, and the root's recursive subtree drop frees the whole chain). A
// member with NO interior `cross_region_refs` container edge (capture/funnel-recovered
// containment, no store site) or with two-or-more non-root containers and no root edge
// (an ambiguous single owner) refuses the whole subtree to Shared (the always-legal
// baseline). These pins are written from that definition.

// ── ownership inference: the lifetime obligation is STRUCTURAL, not numeric ──────
//
// The root's single decref subtree-drops every member, so it must post-dominate every
// member's last use. That is decided by structural post-dominance over the scope tree
// (`regions::postdom::drop_post_dominates`, `EmitMode::Adopt`), NOT by a numeric
// `ord(member) <= ord(root)` (region-model.md § "The lifetime obligation the root carries"):
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
    // collect (region-rules.md Rule 8). The interior a↔b edges carry no adopt.
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
    // Identify the three regions by their edge roles among the non-hard containment
    // edges: root is target-only (`root ⊇ a`), `a` is both a target (`a ⊇ b`) and a
    // source (`root ⊇ a`), `b` is source-only (`a ⊇ b`).
    let mut srcs = rustc_hash::FxHashSet::default();
    let mut dsts = rustc_hash::FxHashSet::default();
    for &(site, src, dst) in &info.cross_region_refs {
        if info.hard_edge_sites.contains(&site) {
            continue;
        }
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
fn adopt_edges_claims_funnel_recovered_subtree_checked_on() {
    // The funnel face of the store-keyed adopt (region-model.md § "The funnel adopt —
    // the checked-on store face"). On the checked-on (native-Call) production path the
    // interior-cycle shape's stores are opaque `Funnel` calls recording NO
    // `cross_region_refs` edge — the containment reaches the walk only as site-keyed
    // funnel-recovered `containment_edges` — and the cut must admit it exactly as the
    // intrinsic path does: every member adopted directly by the container root (the
    // flat star), each adopt keyed at the funnel CALL site that stored it (the
    // value-resolved emit needs no store opcode).
    //
    // Counterfactual: before the funnel face, a funnel-only member had no emittable
    // owner edge, `compute_adopt_edges` refused the whole subtree to Shared, and the
    // store map was empty — the containment assertions below were RED.
    let (_, info, edges) = adopt_edges_checked_on(
        "(begin (let [root (@array) a (@array) b (@array)] \
                  (begin (%array-push a b) (%array-push b a) \
                         (%array-push root a) (%array-push root b) nil)) \
                nil)",
    );
    // Precondition: genuinely the funnel path — the stores record no non-hard
    // `cross_region_refs` containment; only `containment_edges` carries it.
    assert!(
        !info
            .cross_region_refs
            .iter()
            .any(|(site, _, _)| !info.hard_edge_sites.contains(site)),
        "precondition: checked-on, the funnel stores record no cross_region_refs \
         containment; got {:?}",
        info.cross_region_refs,
    );
    assert!(
        !info.containment_edges.is_empty(),
        "precondition: the containment is funnel-recovered",
    );
    // Identify the regions by their containment-edge roles: root is target-only,
    // a and b are sources (the funnel analog of `container_root_and_members`).
    let mut srcs = rustc_hash::FxHashSet::default();
    let mut dsts = rustc_hash::FxHashSet::default();
    for &(_site, src, dst) in &info.containment_edges {
        srcs.insert(src);
        dsts.insert(dst);
    }
    let roots: Vec<Region> = dsts.iter().copied().filter(|r| !srcs.contains(r)).collect();
    assert_eq!(
        roots.len(),
        1,
        "the shape has one container root; got {roots:?}"
    );
    let root = roots[0];
    let members: Vec<Region> = srcs.iter().copied().filter(|r| *r != root).collect();
    assert_eq!(members.len(), 2, "two members a, b; got {members:?}");
    let adopts: Vec<(Region, Region)> = edges.store.values().flatten().copied().collect();
    for m in &members {
        assert!(
            adopts.contains(&(*m, root)),
            "member r{} must be adopted directly by the container root r{} on the \
             checked-on path; adopts={adopts:?}",
            m.0,
            root.0,
        );
    }
    // Each adopt is keyed at a funnel call site — the site the value-resolved
    // `AdoptRegion` emits at (`emit_increfs_for` on that Call node).
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
fn adopt_edges_refuses_loop_enclosed_member_checked_on() {
    // The lifetime obligation holds on the funnel face: the same loop-enclosed shape
    // `adopt_edges_refuses_loop_enclosed_member` pins on the intrinsic path must refuse
    // checked-on too — the root's free re-runs every iteration and a funnel-adopted
    // member keeps its own decref (store-adopted semantics), the cross-iteration UAF
    // the `EmitMode::Adopt` loop clause exists for. An admission here would be a
    // soundness regression the funnel face must not introduce.
    let (_, info, edges) = adopt_edges_checked_on(
        "(let [c 1] (while c (let [root (@array) a (@array)] \
           (begin (%array-push root a) (%array-push root 7) nil))))",
    );
    assert!(
        !info.containment_edges.is_empty(),
        "precondition: the loop-enclosed containment is funnel-recovered",
    );
    let adopts: Vec<(Region, Region)> = edges.store.values().flatten().copied().collect();
    assert!(
        adopts.is_empty(),
        "a funnel member whose owner's free is re-run by an enclosing loop must emit \
         no adopt — the subtree stays Shared; got {adopts:?}",
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

// ── ownership inference: a `Fresh` native's embed declaration ────────────────────
//
// A `Fresh` native whose result EMBEDS an argument declares which args it embeds
// (`PrimitiveDef::embeds`, region-effects.md § "Native region effects"). The walk's
// `Fresh` arm then records `result ⊇ arg` in `containment_edges` — the compile-time
// analog of the runtime alloc-scan (`find_object_cross_refs`) that counts the same
// embedding. Without it the forest cannot see a captured value flow OUT through an
// escaping result, so it wrongly folds the value into the capturing closure's Owned
// subtree. `with-traits` is the canonical embedder: it clones its arg-0 value with the
// arg-1 struct attached as the `traits` side-field, so it embeds arg 1 (`embeds: &[1]`).

/// The single region captured by the lambda whose `alloc_region` is `closure` — its
/// sole capture's binding's sole source region. The embed-declaration probe's shape
/// has exactly one capture (the trait table); panics otherwise.
fn sole_captured_region(hir: &Hir, info: &RegionInfo, closure: Region) -> Region {
    fn walk(h: &Hir, info: &RegionInfo, closure: Region, out: &mut Vec<Region>) {
        if let HirKind::Lambda { captures, .. } = &h.kind {
            if info.alloc_region.get(&h.id) == Some(&closure) {
                for c in captures {
                    if let Some(rs) = info.binding_source_regions.get(&c.binding) {
                        out.extend(rs.iter().copied().filter(|&r| r != closure));
                    }
                }
            }
        }
        h.for_each_child(|c| walk(c, info, closure, out));
    }
    let mut out = Vec::new();
    walk(hir, info, closure, &mut out);
    out.sort_by_key(|r| r.0);
    out.dedup();
    assert_eq!(
        out.len(),
        1,
        "shape must have exactly one captured region; got {out:?}",
    );
    out[0]
}

#[test]
fn with_traits_embed_refuses_adopt_of_captured_escaping_table() {
    // Fixture shape (tests/integration/fixtures/region-traits-capture-adopt-uaf.lisp):
    // a closure `make` CAPTURES a struct `shared-tbl` and, in its body, attaches it as a
    // trait table with `with-traits`. The traited RESULT escapes `make` (returned from
    // its body) with its `traits` side-field still referencing the captured table.
    //
    // `with-traits` is a `Fresh` NATIVE that embeds arg 1 (the table) into the fresh
    // result's `traits` side-field — declared by `PrimitiveDef::embeds = &[1]`. The walk
    // records the containment edge `result ⊇ table`, so external uniqueness sees the
    // table referenced from OUTSIDE make's subtree (by the escaping result region) and
    // REFUSES to fold it in: the table stays Shared (per-region RC), reclaimed under the
    // live result's reference.
    //
    // Counterfactual (RED before the embed declaration): with no `result ⊇ table` edge
    // the forest judged the captured table externally unique to `make`, capture-adopted
    // it, and make's subtree drop freed it under the escaped result's `traits` field — a
    // use-after-free (`UpdateCapture` under `--trace=guardfree`; the fixture's SIGSEGV).
    let src = "(begin (let [shared-tbl {:type :my-type}] \
                        (let [make (fn (data) (with-traits [data] shared-tbl))] \
                          (make 1))) \
                      nil)";
    let (hir, info, edges) = adopt_edges(src);
    let make_r = sole_closure_region(&hir, &info);
    let tbl = sole_captured_region(&hir, &info, make_r);
    // Precondition: `make` genuinely captures the table (so absent the embed edge the
    // forest would fold it into make's Owned subtree — the counterfactual's premise).
    assert!(
        closure_captures_region(&hir, &info, tbl, make_r),
        "precondition: the closure r{} must capture the table r{}",
        make_r.0,
        tbl.0,
    );
    // The invariant: the captured table, embedded into an escaping result, is adopted by
    // NOBODY — it stays Shared (per-region RC). Asserted FIRST so the counterfactual
    // fails here (the table IS capture-adopted before the fix), on the real defect.
    let adopts: Vec<(Region, Region)> = edges
        .store
        .values()
        .chain(edges.capture.values())
        .flatten()
        .copied()
        .collect();
    assert!(
        !adopts.iter().any(|&(m, _)| m == tbl),
        "the captured table r{} embedded into an escaping result must NOT be adopted \
         (it stays Shared) — got adopts {:?}",
        tbl.0,
        adopts,
    );
    let (_, _, owned) = owned_subtrees_with_effects(src);
    assert!(
        !in_some_owned_subtree(&owned, tbl),
        "the captured-and-embedded table r{} must be in no Owned subtree; owned={:?}",
        tbl.0,
        owned,
    );
    // The mechanism: the fix records the with-traits FRESH result ⊇ the table region.
    let (_, embed_src, result) = info
        .containment_edges
        .iter()
        .copied()
        .find(|&(_, src, _)| src == tbl)
        .unwrap_or_else(|| {
            panic!(
                "with-traits (Fresh, embeds arg 1) must record `result ⊇ table` for the \
                 captured table r{}; containment={:?}",
                tbl.0, info.containment_edges,
            )
        });
    assert_eq!(embed_src, tbl, "the embed's contained member is the table");
    assert_ne!(
        result, tbl,
        "the embed's container is the with-traits result"
    );
    assert!(
        info.fresh_result_regions.contains(&result),
        "the embed container r{} is the with-traits FRESH result",
        result.0,
    );
}

// ── ownership inference: combined store + capture + deep-nesting subtrees ───────
//
// The pins above exercise each emit mode in isolation (a flat/deep store star, a lone
// local capture). These probe the modes COMBINED in one externally-unique subtree, where
// `compute_adopt_edges`'s owner assignment must thread store edges and re-derived capture
// edges through the same `containers_of` graph. Each assertion is written from the spec
// ("Both feed owner assignment … each member adopted by its actual parent") — a member's
// owner is the root when it has a direct root edge, else its unique interior container,
// regardless of whether the edge is a store or a capture.

/// All `%pair` site regions in `hir` (the seed shapes have one; the two-capture shape has
/// two), in program order. A local generalization of [`sole_pair_region`] for the
/// multi-pair probes below.
fn all_pair_regions(hir: &Hir, info: &RegionInfo) -> Vec<Region> {
    let mut pairs = Vec::new();
    find_all_pairs_helper(hir, &mut pairs);
    pairs
        .iter()
        .filter_map(|id| info.alloc_region.get(id).copied())
        .collect()
}

/// The region of the first (outermost) `Lambda` node in `hir` — the closure region the
/// combined-shape probes capture into. (Each probe has exactly one closure.)
fn sole_closure_region(hir: &Hir, info: &RegionInfo) -> Region {
    fn first_lambda(h: &Hir, out: &mut Option<HirId>) {
        if matches!(&h.kind, HirKind::Lambda { .. }) && out.is_none() {
            *out = Some(h.id);
        }
        h.for_each_child(|c| first_lambda(c, out));
    }
    let mut lam = None;
    first_lambda(hir, &mut lam);
    *info
        .alloc_region
        .get(&lam.expect("a Lambda node"))
        .expect("the closure has an alloc region")
}

#[test]
fn adopt_edges_chains_store_and_capture_in_one_subtree() {
    // Combined probe: a Fresh container `root` holds a local capturing closure `c`, and
    // `c` captures the pair `p`. One externally-unique Owned subtree {root, c, p} chains
    // the two emit modes — `c` is adopted by `root` through a STORE edge
    // (`%array-push root c` records a `cross_region_refs` containment edge `root ⊇ c`),
    // and `p` is adopted by `c` through a re-derived CAPTURE edge `c ⊇ p`. The owner
    // assignment must thread both kinds: `c`'s actual parent is the root (direct store
    // edge), `p`'s actual parent is the non-root container `c`. The root's recursive
    // subtree drop then reclaims the whole chain.
    //
    // `(c)` is called BEFORE the push so the root (last used at the push) post-dominates
    // the closure's last use — the lifetime obligation `member_dp ≤ root_dp` an
    // owner-dies-before-member order would (correctly) refuse.
    //
    // Spec (compute_adopt_edges doc, "Both feed owner assignment"): store adopt (c→root),
    // capture adopt (p→c); each member rides exactly one map.
    let (hir, info, edges) = adopt_edges(
        "(begin (let [p (%pair 1 2) root (@array)] \
                  (let [c (fn [] (%first p))] \
                    (begin (c) (%array-push root c) nil))) \
                nil)",
    );
    let p = sole_pair_region(&hir, &info);
    let c = sole_closure_region(&hir, &info);
    // The container root: the target of the non-hard store edge whose source is `c`.
    let root = info
        .cross_region_refs
        .iter()
        .find(|(site, src, _)| !info.hard_edge_sites.contains(site) && *src == c)
        .map(|&(_, _, dst)| dst)
        .expect("precondition: a store edge root ⊇ c");
    let store: Vec<(Region, Region)> = edges.store.values().flatten().copied().collect();
    let capture: Vec<(Region, Region)> = edges.capture.values().flatten().copied().collect();
    assert!(
        store.contains(&(c, root)),
        "the closure r{} must be adopted by the container root r{} via a STORE edge; \
         store={:?}, capture={:?}",
        c.0,
        root.0,
        store,
        capture,
    );
    assert!(
        capture.contains(&(p, c)),
        "the captured pair r{} must be adopted by its closure r{} via a CAPTURE edge; \
         store={:?}, capture={:?}",
        p.0,
        c.0,
        store,
        capture,
    );
    // Each member rides exactly one map — `p` via capture only, `c` via store only.
    assert!(
        !store.iter().any(|&(m, _)| m == p),
        "the captured pair r{} must not also ride the store map; store={:?}",
        p.0,
        store,
    );
    assert!(
        !capture.iter().any(|&(m, _)| m == c),
        "the stored closure r{} must not also ride the capture map; capture={:?}",
        c.0,
        capture,
    );
}

#[test]
fn adopt_edges_chains_deep_nesting_with_capture_leaf() {
    // Combined probe: deep store nesting with a CAPTURE at the leaf. `root ⊇ mid` and
    // `mid ⊇ c` are store edges, `c ⊇ p` a capture edge — so the subtree {root, mid, c, p}
    // mixes two store levels and a capture in one chain. Each member is adopted by its
    // ACTUAL parent: `mid` by the root, `c` by its interior container `mid`, `p` by its
    // capturing closure `c`. The root's recursive subtree drop walks `owned_children`
    // three levels deep.
    //
    // Spec: store adopts {(mid→root), (c→mid)}; capture adopt {(p→c)}.
    let (hir, info, edges) = adopt_edges(
        "(begin (let [p (%pair 1 2) mid (@array) root (@array)] \
                  (let [c (fn [] (%first p))] \
                    (begin (c) (%array-push mid c) (%array-push root mid) nil))) \
                nil)",
    );
    let p = sole_pair_region(&hir, &info);
    let c = sole_closure_region(&hir, &info);
    // mid = the target of the store edge sourced at `c`; root = the target of the store
    // edge sourced at `mid` (both non-hard).
    let store_target_of = |src: Region| -> Region {
        info.cross_region_refs
            .iter()
            .find(|(site, s, _)| !info.hard_edge_sites.contains(site) && *s == src)
            .map(|&(_, _, dst)| dst)
            .unwrap_or_else(|| panic!("precondition: a store edge sourced at r{}", src.0))
    };
    let mid = store_target_of(c);
    let root = store_target_of(mid);
    let store: rustc_hash::FxHashSet<(Region, Region)> =
        edges.store.values().flatten().copied().collect();
    let capture: Vec<(Region, Region)> = edges.capture.values().flatten().copied().collect();
    assert_eq!(
        store,
        [(mid, root), (c, mid)].into_iter().collect(),
        "deep nesting + capture must adopt `mid` by the root r{} and `c` by its actual \
         parent `mid` r{} via STORE edges (root r{}, mid r{}, c r{}); store={:?}",
        root.0,
        mid.0,
        root.0,
        mid.0,
        c.0,
        store,
    );
    assert!(
        capture.contains(&(p, c)),
        "the captured leaf pair r{} must be adopted by its closure r{} via a CAPTURE \
         edge; capture={:?}",
        p.0,
        c.0,
        capture,
    );
}

#[test]
fn adopt_edges_claims_two_captures_in_one_closure() {
    // Combined probe: ONE closure captures TWO local values `p` and `q`. The subtree
    // {c, p, q} is rooted at the closure `c` (the top container — no store edge holds it),
    // and BOTH captured pairs are adopted by `c` through capture edges. Exercises the
    // owner-assignment loop over multiple capture members sharing one owner.
    //
    // Spec: capture adopts {(p→c), (q→c)}; no store adopts.
    let (hir, info, edges) = adopt_edges(
        "(begin (let [p (%pair 1 2) q (%pair 3 4)] \
                  (let [c (fn [] (begin (%first p) (%first q)))] (c))) \
                nil)",
    );
    let pairs = all_pair_regions(&hir, &info);
    assert_eq!(
        pairs.len(),
        2,
        "precondition: two %pair members; got {:?}",
        pairs
    );
    let c = sole_closure_region(&hir, &info);
    let capture: rustc_hash::FxHashSet<(Region, Region)> =
        edges.capture.values().flatten().copied().collect();
    for &pr in &pairs {
        assert!(
            capture.contains(&(pr, c)),
            "captured pair r{} must be adopted by the closure root r{} via a CAPTURE \
             edge; capture={:?}",
            pr.0,
            c.0,
            capture,
        );
    }
    let store: Vec<(Region, Region)> = edges.store.values().flatten().copied().collect();
    assert!(
        store.is_empty(),
        "a pure two-capture subtree has no store edges, so no store adopts; got {:?}",
        store,
    );
}

#[test]
fn adopt_edges_refuses_nested_capture_on_lifetime_overextension() {
    // Boundary pin + step-3 frontier (NOT a bug — a sound conservative refusal). A closure
    // capturing a CLOSURE capturing a value — `c1 ⊇ c2 ⊇ p`, all CAPTURE edges, the shape a
    // closure-web is built from. `compute_owned_subtrees` ADMITS {c1, c2, p} as externally
    // unique (nothing escapes), but `compute_adopt_edges` REFUSES it on the lifetime
    // obligation, so it stays Shared (the always-legal baseline) — no adopt is emitted.
    //
    // Why it refuses: the tight last-use (`binding_last_use`) bounds a captured value by its
    // owning closure's last CALL. For a NESTED capture the owning closure
    // is itself captured, so its "last call" is already the over-extended one-step-past
    // position, and the bound cascades one structural step PER nesting level. Measured here:
    // the root `c1` dies at its `decref_point`, but `p`'s tight last-use lands one step LATER
    // (`p`'s owner `c2` is captured by `c1`, so `c2`'s last-call bound is already at `c1`'s
    // death, and `p` inherits one step past that). The obligation `member_dp ≤ root_dp` then
    // cannot prove `p` dies before the root, so it refuses — sound, since adopting would risk
    // freeing `p` under a still-live reference the analysis cannot rule out.
    //
    // At runtime `p` IS dead when `(c1)` returns (c1/c2/p all discarded together), so a
    // tighter transitive-through-capture last-use would let the forest claim this — that
    // improvement is part of the cross-fiber owner = activation cut (which claims the
    // closure-web). When it lands, this pin flips and forces the author to
    // confirm the adopt then emits. Until then the refusal is the correct boundary.
    let src = "(begin (let [p (%pair 1 2)] \
                        (let [c2 (fn [] (%first p))] \
                          (let [c1 (fn [] (c2))] (c1)))) \
                      nil)";
    let (hir, info, edges) = adopt_edges(src);
    let p = sole_pair_region(&hir, &info);
    // (1) The subtree is admitted as externally unique — the refusal is at the adopt stage,
    // not the subtree stage (so a fix lands in `compute_adopt_edges`/the last-use, not the
    // external-uniqueness walk).
    let (_, _, owned) = owned_subtrees_with_effects(src);
    let root = owned
        .iter()
        .find(|(_, members)| members.contains(&p))
        .map(|(&r, _)| r)
        .expect("compute_owned_subtrees must admit the externally-unique {c1, c2, p} subtree");
    // (2) Pin the REASON: `p`'s tight last-use over-extends past the root's decref_point —
    // the exact condition the lifetime obligation refuses on. A tighter last-use that lowers
    // `p` to ≤ root would flip both this and the emptiness assertion below.
    let order = compute_order(&hir);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let root_dp = ord(info
        .region_data
        .get(&root)
        .expect("root has region_data")
        .decref_point);
    let p_tight = info
        .binding_last_use
        .get(&p)
        .map(|&id| ord(id))
        .expect("the captured pair has a tight last-use");
    assert!(
        p_tight > root_dp,
        "precondition (the refusal cause): the nested-captured pair r{}'s tight last-use \
         ({p_tight}) must over-extend past the subtree root r{}'s decref_point ({root_dp}); \
         if it no longer does, the lifetime obligation now admits the chain and the \
         emptiness assertion below must be replaced by the positive adopt expectation",
        p.0,
        root.0,
    );
    // (3) So no adopt is emitted for the whole program — the chain stays Shared.
    assert!(
        edges.store.is_empty() && edges.capture.is_empty(),
        "the nested-capture chain must emit NO adopts (refused to Shared on the lifetime \
         obligation); got store={:?}, capture={:?}",
        edges.store,
        edges.capture,
    );
}

#[test]
fn adopt_edges_capture_root_with_store_child() {
    // Combined probe (mirror of `adopt_edges_chains_store_and_capture_in_one_subtree`):
    // the CLOSURE is the root and the store subtree hangs beneath it. A closure `c`
    // captures a CONTAINER `m`, and `m` holds a value `v`: `c ⊇ m` (capture), `m ⊇ v`
    // (store). So `m` is a capture-adopt member that is itself a store-adopt PARENT.
    //
    // Spec: capture adopt (m→c); store adopt (v→m).
    let (hir, info, edges) = adopt_edges(
        "(begin (let [v (@array) m (@array)] \
                  (let [c (fn [] (%first m))] \
                    (begin (%array-push m v) (c) nil))) \
                nil)",
    );
    let c = sole_closure_region(&hir, &info);
    let capture: rustc_hash::FxHashSet<(Region, Region)> =
        edges.capture.values().flatten().copied().collect();
    // `m` is the container captured by `c` (the capture child whose owner is `c`).
    let m = capture
        .iter()
        .find(|(_, o)| *o == c)
        .map(|&(child, _)| child)
        .unwrap_or_else(|| {
            panic!(
                "the closure r{} must capture-adopt the container m; capture={:?}",
                c.0, capture
            )
        });
    // `v` is the store child of `m`.
    let store: rustc_hash::FxHashSet<(Region, Region)> =
        edges.store.values().flatten().copied().collect();
    let v = store
        .iter()
        .find(|(_, owner)| *owner == m)
        .map(|&(child, _)| child)
        .unwrap_or_else(|| {
            panic!(
                "the container m r{} must store-adopt its value v; store={:?}",
                m.0, store
            )
        });
    assert_eq!(
        capture,
        [(m, c)].into_iter().collect(),
        "the container m r{} must be capture-adopted by the closure root c r{}, and nothing \
         else; capture={:?}",
        m.0,
        c.0,
        capture,
    );
    assert_eq!(
        store,
        [(v, m)].into_iter().collect(),
        "the value v r{} must be store-adopted by its container m r{}, and nothing else; \
         store={:?}",
        v.0,
        m.0,
        store,
    );
}

#[test]
fn adopt_edges_refuses_captured_store_member_on_lifetime() {
    // Boundary pin + UAF regression (NOT a bug — a sound conservative refusal, the
    // owner-aware lifetime obligation). A two-level subtree whose interior cycle back-edge
    // is a CAPTURE: `root` holds `m` (store `root ⊇ m`), `m` holds a closure `c` (store
    // `m ⊇ c`), and `c` captures `m` back (capture `c ⊇ m`) — the m↔c reference cycle. The
    // subtree {root, m, c} is admitted as externally unique, and `m`'s owner is the root
    // (its direct store edge, preferred over the capture container `c`).
    //
    // The hazard: `m` is ALSO a captured value (captured by `c`), and a captured value's
    // `decref_point` is over-extended one structural step past its owning closure.
    // `m` is STORE-adopted (owner = root), so its own `DecrefValueRegion`
    // is NOT suppressed — it fires at that over-extended position, AFTER the root's decref.
    // `@array` regions are Fresh call-results released value-based, so the root's drop frees
    // `m`, and `m`'s own later decref-value then `result_region_of`s a freed page: a
    // use-after-free (the original failure this pin guards — `region_ownership` SIGSEGV'd
    // under guardfree, runtime test `region_ownership_capture_back_edge_cycle_runs_sound`).
    //
    // The owner-aware obligation bounds a store-adopted member by its STRUCTURAL
    // `decref_point` (where its live decref fires), not the tight last-use, so `m`'s
    // over-extension refuses the subtree — no REGION root owns it (this refusal is
    // permanent and correct). The m↔c SCC itself is claimed by the ACTIVATION cut
    // instead (`activation_adopts_capture_back_edge_scc`), whose owner-node release
    // post-dominates the whole activation. The `(c)` call is present so `c` is genuinely
    // used; it does not change the refusal.
    let src = "(begin (let [root (@array) m (@array)] \
                        (let [c (fn [] (length m))] \
                          (begin (%array-push m c) (c) (%array-push root m) nil))) \
                      nil)";
    let (hir, info, edges) = adopt_edges(src);
    // `m` is the @array that is BOTH a store source (`m → root`) and a store target
    // (`c → m`); `root` is target-only, `c` source-only.
    let mut srcs = rustc_hash::FxHashSet::default();
    let mut dsts = rustc_hash::FxHashSet::default();
    for &(site, s, d) in &info.cross_region_refs {
        if !info.hard_edge_sites.contains(&site) {
            srcs.insert(s);
            dsts.insert(d);
        }
    }
    let m = *srcs
        .iter()
        .find(|r| dsts.contains(r))
        .expect("precondition: `m` is both a store source and target");
    let root = *dsts
        .iter()
        .find(|r| !srcs.contains(r))
        .expect("precondition: a target-only container root");
    // (1) The subtree is admitted as externally unique — the refusal is at the adopt stage,
    // not the subtree stage.
    let (_, _, owned) = owned_subtrees_with_effects(src);
    assert!(
        owned.values().any(|s| s.contains(&m) && s.contains(&root)),
        "compute_owned_subtrees must admit the externally-unique {{root, m, c}} subtree \
         (root r{}, m r{}); owned={:?}",
        root.0,
        m.0,
        owned,
    );
    // (2) Pin the REASON: `m`'s STRUCTURAL decref_point (where its live store-member
    // decref-value fires) over-extends past the root's — the exact condition the owner-aware
    // obligation refuses on. A future cut that suppresses or tightens `m`'s decref would flip
    // this, forcing the author to confirm the runtime then reclaims it soundly.
    let order = compute_order(&hir);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let root_dp = ord(info
        .region_data
        .get(&root)
        .expect("root region_data")
        .decref_point);
    let m_dp = ord(info
        .region_data
        .get(&m)
        .expect("m region_data")
        .decref_point);
    assert!(
        m_dp > root_dp,
        "precondition (the refusal cause): the captured-and-store-owned member r{}'s \
         structural decref_point ({m_dp}) must over-extend past the root r{}'s ({root_dp}); \
         if not, the obligation now admits it and this refusal pin must become a positive \
         reclamation expectation",
        m.0,
        root.0,
    );
    // (3) So the subtree is refused — no adopt emitted, the m↔c cycle stays Shared.
    assert!(
        edges.store.is_empty() && edges.capture.is_empty(),
        "the captured-store-member subtree must emit NO adopts (refused to Shared on the \
         owner-aware lifetime obligation); got store={:?}, capture={:?}",
        edges.store,
        edges.capture,
    );
}

// ── ownership inference: the capture-adopt contract (suppress ⊆ adopt) ──────────
//
// `analyze_regions_with` suppresses the own-decref of every `capture_adopt_edges` member
// (it is reclaimed solely by its closure's subtree drop), and `lower_lambda_expr` emits
// the matching `AdoptRegion` by reloading the captured value through the capture's own
// access path — a binding slot for a direct local, the constructing function's
// environment for an upvalue or transitive capture (region-model.md § "The capture
// adopt"). The contract is therefore held by EMIT CAPABILITY: an edge is emittable iff
// the closure genuinely captures a binding holding the member's region, which is true of
// every capture containment edge by construction — the `debug_assert` at the emit seam
// in `lower_lambda_expr` is the backstop. What bounds ADMISSION is the lifetime
// obligation alone; for the cross-activation (upvalue) family it refuses by
// construction — the forwarding capture pins the member's tight last-use at/past the
// enclosing lambda's own node, after a nested root's in-body drop — and genuinely must
// (a nested root's region is per-call of its encloser; claiming a member that survives
// across calls would free it under the encloser's live env and re-adopt an Owned region
// on the next call). These pins lock both halves at the inference level.

#[test]
fn capture_adopt_edges_are_emittable() {
    // Every capture adopt edge `compute_adopt_edges` emits must be EMITTABLE: the owning
    // closure's Lambda must capture a binding whose source regions include the member, so
    // `lower_lambda_expr`'s reload (slot or env) can find the value for the
    // value-resolved adopt (suppress ⊆ adopt). Checked across the shape that DOES adopt a
    // capture and the web/upvalue shapes that are refused (vacuous there, but they
    // exercise the path that would regress). Counterfactual: a `compute_adopt_edges`
    // change that chose an owner-edge no capture realizes — e.g. keying a member onto a
    // closure that merely reaches it transitively — fails this directly, before the
    // suppressed-yet-unadopted leak ever reaches the runtime.
    let shapes = [
        // The simple local capture that IS adopted (the one capture edge that fires today).
        "(begin (let [p (%pair 1 2)] (let [c (fn [] (%first p))] (c))) nil)",
        // Closure-webs (refused on the lifetime obligation): mutually-recursive closures
        // over a shared captured value, and a nested closure capturing an outer binding
        // as an upvalue.
        "(begin (let [b (%pair 1 2)] (letrec [f (fn [] (begin (g) (%first b))) g (fn [] (%first b))] (f))) nil)",
        "(begin (let [b (%pair 1 2)] (let [outer (fn [] (let [inner (fn [] (%first b))] (inner)))] (outer))) nil)",
    ];
    for src in shapes {
        let (hir, info, edges) = adopt_edges(src);
        for (member, closure) in edges.capture.values().flatten().copied() {
            assert!(
                closure_captures_region(&hir, &info, member, closure),
                "capture adopt edge (member r{}, closure r{}) is NOT emittable: no capture \
                 of the closure holds the member's region, so analyze suppresses its decref \
                 but the lowerer cannot adopt it — a leak. src={src}",
                member.0,
                closure.0,
            );
        }
    }
}

/// Does the lambda whose `alloc_region` is `closure` capture a binding whose
/// source regions include `member` — i.e. can `lower_lambda_expr` reload the
/// member's value (from a slot or the env) for the value-resolved adopt?
fn closure_captures_region(hir: &Hir, info: &RegionInfo, member: Region, closure: Region) -> bool {
    fn walk(h: &Hir, info: &RegionInfo, member: Region, closure: Region, found: &mut bool) {
        if let HirKind::Lambda { captures, .. } = &h.kind {
            if info.alloc_region.get(&h.id) == Some(&closure)
                && captures.iter().any(|c| {
                    info.binding_source_regions
                        .get(&c.binding)
                        .is_some_and(|rs| rs.contains(&member))
                })
            {
                *found = true;
            }
        }
        h.for_each_child(|c| walk(c, info, member, closure, found));
    }
    let mut found = false;
    walk(hir, info, member, closure, &mut found);
    found
}

#[test]
fn owned_subtree_upvalue_capture_owner_refused_on_lifetime() {
    // The cross-activation boundary (region-model.md § "The capture adopt"): a nested
    // closure `o` (inside `e`) captures `e` (the letrec recursion) AND the pair `m`, and
    // `e` ALSO captures `m` (the forward every upvalue implies). With the capture edge
    // re-pointed through the cell (`closure ⊇ cell ⊇ content`), external uniqueness ADMITS
    // the subtree `{o, cell_e, e, m}` with `o` as root — the containment is now visible —
    // and the refusal MUST hold at the lifetime obligation: `o`'s region is minted per
    // CALL of `e`, so adopting `m`/`e` (which survive across calls) would free them under
    // `e`'s still-live references and re-adopt an already-Owned region on the next call.
    // The obligation refuses structurally: the forwarding capture resolves `m`'s tight
    // last-use through `e`'s binding chain to a position at/past the enclosing lambda node,
    // which post-dates `o`'s in-body drop in post-order. Three halves:
    //   (1) the subtree is admitted, and the root's capture of `m` is env-loaded (an
    //       upvalue) — the shape genuinely exercises the cross-activation path;
    //   (2) the obligation's refusal CAUSE is pinned: `m`'s tight last-use over-extends
    //       past the root's decref_point (the exact condition it refuses on);
    //   (3) no adopt is emitted — the family stays Shared (the always-legal baseline)
    //       until an owner that outlives every capturer (the activation/fiber node) exists.
    let src = "(begin (let [m (%pair 1 2)] \
                        (letrec [e (fn [] (let [o (fn [] (begin (e) (%first m)))] (o)))] (e))) \
                      nil)";
    let (hir, info, owned) = owned_subtrees_with_effects(src);
    let m = sole_pair_region(&hir, &info);
    // (1) admitted, with an env-loaded (upvalue) owner-capture of `m`.
    let owner = owned
        .iter()
        .find(|(_, s)| s.contains(&m))
        .map(|(r, _)| *r)
        .expect("compute_owned_subtrees must admit the externally-unique upvalue subtree");
    assert!(
        !capture_is_local_slot_loaded(&hir, &info, m, owner),
        "precondition: the pair r{} must be captured by its subtree root r{} through the \
         ENV (an upvalue capture); if slot-loaded, the shape no longer exercises the \
         cross-activation boundary",
        m.0,
        owner.0,
    );
    // (2) the refusal cause: the tight last-use over-extends past the root's drop. A
    // change that makes this pass — a tighter transitive last-use, or a root whose drop
    // moves out of the enclosing body — flips this precondition and forces the author to
    // re-derive the soundness argument (the per-call-root double-adopt) before admitting
    // the family.
    let order = compute_order(&hir);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let root_dp = ord(info
        .region_data
        .get(&owner)
        .expect("root has region_data")
        .decref_point);
    let m_tight = info
        .binding_last_use
        .get(&m)
        .map(|&id| ord(id))
        .expect("the captured pair has a tight last-use");
    assert!(
        m_tight > root_dp,
        "precondition (the refusal cause): the upvalue-captured member r{}'s tight \
         last-use ({m_tight}) must over-extend past the per-call root r{}'s decref_point \
         ({root_dp}); if it no longer does, the lifetime obligation would admit a per-call \
         root owning a cross-call member — re-derive the soundness argument before flipping \
         the assertion below",
        m.0,
        owner.0,
    );
    // (3) so no adopt is emitted for the whole program — the family stays Shared.
    let (_, _, edges) = adopt_edges(src);
    assert!(
        edges.store.is_empty() && edges.capture.is_empty(),
        "the upvalue-owner family must emit NO adopts (refused to Shared on the lifetime \
         obligation); got store={:?}, capture={:?}",
        edges.store,
        edges.capture,
    );
}

// ── ownership inference: the capture-cell clique (closure ⊇ cell ⊇ content) ─────
//
// A local `letrec`/`def` closure that captures a sibling's forward cell forms the chain
// `closure ⊇ cell ⊇ content` — the capture edge re-pointed through the cell
// (`capture_containment_edges`) and the walk's `cell ⊇ content` edge together. With both
// visible, `compute_owned_subtrees` admits the local, non-escaping `{closure, cell,
// content}` clique as externally unique (the modeling's headline — the invisible cell
// containment is now seen). It refuses the moment the clique is NOT externally unique: the
// closure escapes (a Shared seed), or the cell is captured by TWO siblings (an interior
// region referenced from outside any single-root subtree). These pin that admission and
// its two boundaries.

/// The sole compiled cell region and its content region (the `(site, content, cell)` walk
/// edge) of a single-cell shape — the clique's interior two members.
fn sole_cell_and_content(info: &RegionInfo) -> (Region, Region) {
    let cells: Vec<Region> = info
        .begin_cell_regions
        .values()
        .flatten()
        .map(|&(_, r)| r)
        .collect();
    assert_eq!(
        cells.len(),
        1,
        "shape must mint exactly one compiled cell; got {cells:?}",
    );
    let cell = cells[0];
    let content = info
        .containment_edges
        .iter()
        .find(|&&(_, _, c)| c == cell)
        .map(|&(_, content, _)| content)
        .unwrap_or_else(|| {
            panic!(
                "the cell r{} must carry a `cell ⊇ content` edge; containment={:?}",
                cell.0, info.containment_edges,
            )
        });
    (cell, content)
}

#[test]
fn owned_subtrees_admits_local_capture_cell_clique() {
    // A local `letrec`: `drive` captures the forward cell of `leaf` (`(fn [m] m)`), which
    // holds its closure. Neither escapes (`(drive n)`'s result — an int — is what leaves).
    // With `closure ⊇ cell` re-pointed and `cell ⊇ content` recorded, the clique
    // `{drive_closure, leaf_cell, leaf_content}` is externally unique → admitted.
    //
    // Counter-factual: before the modeling the `closure ⊇ content` mis-pointing left the
    // content a free-standing top container the scan could not bound through its cell, so no
    // subtree formed over it (the invisible-containment hole this closes).
    let (_hir, info, owned) = owned_subtrees_with_effects(
        "(defn build [n] \
           (letrec [drive (fn [m] (leaf m)) \
                    leaf (fn [m] m)] \
             (%add (drive n) 0)))",
    );
    let (cell, content) = sole_cell_and_content(&info);
    let subtree = owned
        .values()
        .find(|s| s.contains(&cell))
        .unwrap_or_else(|| {
            panic!(
                "compute_owned_subtrees must admit a subtree containing the cell r{} \
                 (the `closure ⊇ cell ⊇ content` clique); owned={:?}",
                cell.0, owned,
            )
        });
    assert!(
        subtree.contains(&content),
        "the admitted subtree must contain the cell's content r{} too (the whole chain \
         `closure ⊇ cell ⊇ content` reclaims as a unit); subtree={:?}",
        content.0,
        subtree.iter().map(|r| r.0).collect::<Vec<_>>(),
    );
}

#[test]
fn owned_subtrees_refuses_escaping_capture_cell_clique() {
    // The escape boundary: the capturing closure `drive` is RETURNED, so it crosses the
    // return frontier (a Shared seed) and its whole `closure ⊇ cell ⊇ content` chain must
    // stay Shared. `compute_owned_subtrees` admits NO subtree over the cell.
    // (region-repeated-call-adopt-uaf.lisp is the runtime witness of this boundary — an
    // escaping top-level chain must never be subtree-dropped.)
    let (_hir, info, owned) = owned_subtrees_with_effects(
        "(defn build [n] \
           (letrec [drive (fn [m] (leaf m)) \
                    leaf (fn [m] m)] \
             drive))",
    );
    let (cell, _content) = sole_cell_and_content(&info);
    assert!(
        !in_some_owned_subtree(&owned, cell),
        "an escaping capturing closure must leave its cell r{} Shared (in no owned \
         subtree); owned={:?}",
        cell.0,
        owned,
    );
}

#[test]
fn owned_subtrees_refuses_two_sibling_captured_cell() {
    // The external-uniqueness boundary: `leaf`'s forward cell is captured by TWO siblings
    // `a` and `b`. For a subtree rooted at either, the OTHER closure holds the cell from
    // outside it (`outside_ref_in`), so neither is externally unique — the cell stays
    // Shared. This is the "captured by two siblings" refusal the modeling must keep.
    let (_hir, info, owned) = owned_subtrees_with_effects(
        "(defn build [n] \
           (letrec [a (fn [m] (leaf m)) \
                    b (fn [m] (leaf m)) \
                    leaf (fn [m] m)] \
             (%add (%add (a n) (b n)) 0)))",
    );
    let (cell, _content) = sole_cell_and_content(&info);
    assert!(
        !in_some_owned_subtree(&owned, cell),
        "a cell r{} captured by two siblings must be Shared (no single-root subtree is \
         externally unique); owned={:?}",
        cell.0,
        owned,
    );
}

// ── ownership inference: the re-storable-cell gate (§3 loop hazard) ─────────────
//
// An `@`-mutable captured cell is re-stored every loop iteration; its release is hoisted
// once past the loop, so each stored content's lifetime is `[store, next-rebind]` —
// SHORTER than the cell's. Adopting it into the cell's subtree would free a displaced
// prior under a live cell (`region-capture-cell-loop-uaf.lisp`). So a re-storable cell's
// content is NOT adoptable: `capture_containment_edges` skips the capture (the cell stays
// a borrow), and `compute_adopt_edges`'s `adoptable_cell` refuses its `cell ⊇ content`
// edge even when the walk records it (for external-uniqueness *counting*, §3). The
// immutable letrec cell in the same clique is handled the other way — re-pointed and
// adoptable. These pin both halves of the gate.

#[test]
fn capture_edge_skips_restorable_cell_admits_immutable_in_one_clique() {
    // One clique, both cell kinds: `holder` captures `@acc` (an `@`-mutable local — a
    // re-storable cell) AND `leaf` (an immutable letrec forward cell). The immutable
    // capture is re-pointed `closure ⊇ cell`; the re-storable capture yields NO owner edge
    // — its content reclaims on the per-region-RC baseline, never adopted under the cell.
    let (hir, arena, info, edges) = capture_edges(
        "(defn build [] \
           (letrec [@acc (list 1 2) \
                    leaf (fn [] 1) \
                    holder (fn [] (begin acc (leaf)))] \
             (holder)))",
    );
    // Find `holder`'s two captures by their re-storable classification, so the shape is
    // proven to genuinely mix both kinds (the "one clique with both" premise).
    let mut restorable_binding: Option<Binding> = None;
    let mut immutable_binding: Option<Binding> = None;
    fn find_caps(
        h: &Hir,
        arena: &BindingArena,
        restorable: &mut Option<Binding>,
        immutable: &mut Option<Binding>,
    ) {
        if let HirKind::Lambda { captures, .. } = &h.kind {
            for c in captures {
                let bi = arena.get(c.binding);
                if bi.is_restorable_capture_cell() {
                    *restorable = Some(c.binding);
                } else if bi.needs_capture() {
                    *immutable = Some(c.binding);
                }
            }
        }
        h.for_each_child(|c| find_caps(c, arena, restorable, immutable));
    }
    find_caps(
        &hir,
        &arena,
        &mut restorable_binding,
        &mut immutable_binding,
    );
    let restorable = restorable_binding.expect("holder must capture a re-storable @acc");
    let immutable = immutable_binding.expect("holder must capture an immutable leaf cell");
    // The immutable cell IS re-pointed: `single_cell_region_of(leaf)` names a compiled
    // cell, and a `closure ⊇ cell` edge points at it.
    let leaf_cell = info
        .single_cell_region_of(immutable)
        .expect("the immutable leaf must have a compiled cell");
    assert!(
        edges.iter().any(|&(_, src, _)| src == leaf_cell),
        "the immutable leaf cell r{} must get a `closure ⊇ cell` capture edge; edges={edges:?}",
        leaf_cell.0,
    );
    // The re-storable @acc yields NO owner edge: none of its source regions is a capture
    // edge source (the `is_restorable_capture_cell` skip — the §3 loop hazard). @acc takes
    // the `populate_env` route so it has no compiled cell of its own.
    assert_eq!(
        info.single_cell_region_of(restorable),
        None,
        "the re-storable @acc must have no compiled cell (the populate_env borrow route)",
    );
    let restorable_regions = info
        .binding_source_regions
        .get(&restorable)
        .cloned()
        .unwrap_or_default();
    for r in &restorable_regions {
        assert!(
            !edges.iter().any(|&(_, src, _)| src == *r),
            "the re-storable @acc's region r{} must appear in NO capture edge — its content \
             stays a borrow (the §3 loop hazard); edges={edges:?}",
            r.0,
        );
    }
    // Neither is capture-adopted through the cell-content path (the re-storable is refused;
    // the immutable is reached via its holder's subtree, not a cell-store adopt).
    assert!(
        !info.cell_content_adopt_bindings.contains(&restorable),
        "the re-storable @acc must never be a cell-content adopt binding",
    );
}

#[test]
fn restorable_compiled_cell_records_content_edge_but_is_not_adopted() {
    // The `compute_adopt_edges` half: a TOP-LEVEL `@acc` is an `@`-mutable captured binding,
    // so its cell is a re-storable COMPILED cell. The walk STILL records its `cell ⊇ content`
    // edge (for external-uniqueness counting — the cell holds *a* content, §3), but
    // `adoptable_cell` refuses it, so the cell's binding never reaches
    // `cell_content_adopt_bindings`. The re-storable content is therefore never linked into
    // the cell's subtree — it keeps its own per-rebind release, the safe borrow.
    let (_hir, arena, info, edges) = capture_edges(
        "(def @acc (list 1 2)) \
         (def reader (fn [] acc)) \
         (%add (%length (reader)) 0)",
    );
    // Precondition: `@acc` is a re-storable compiled cell.
    let restorable_cells: Vec<(Binding, Region)> = info
        .begin_cell_regions
        .values()
        .flatten()
        .copied()
        .filter(|&(b, _)| arena.get(b).is_restorable_capture_cell())
        .collect();
    assert_eq!(
        restorable_cells.len(),
        1,
        "precondition: exactly one re-storable compiled cell (@acc); got {restorable_cells:?}",
    );
    let (acc_binding, acc_cell) = restorable_cells[0];
    // The walk DID record its `cell ⊇ content` edge (the cell holds the list) ...
    assert!(
        info.containment_edges
            .iter()
            .any(|&(_, _, cell)| cell == acc_cell),
        "the re-storable cell r{} must still carry a recorded `cell ⊇ content` edge \
         (external-uniqueness counting, §3); containment={:?}",
        acc_cell.0,
        info.containment_edges,
    );
    // ... yet `adoptable_cell` refuses it: the binding is not a cell-content adopt, and its
    // capture is skipped (no owner edge). The re-storable content stays a borrow.
    assert!(
        !info.cell_content_adopt_bindings.contains(&acc_binding),
        "the re-storable cell's content must NOT be adopted (adoptable_cell refuses) — \
         cell_content_adopt_bindings={:?}",
        info.cell_content_adopt_bindings,
    );
    assert!(
        !edges.iter().any(|&(_, src, _)| src == acc_cell),
        "the re-storable cell r{} must be captured by no `closure ⊇ cell` edge (the \
         capture skip); edges={edges:?}",
        acc_cell.0,
    );
}

// ── ownership inference: the activation-owner cut (capture-back-edge SCC) ───────
//
// The m↔c capture-back-edge SCC — a container captured by a closure it holds — is the
// cycle neither region-rooted mode can own (the owner-aware lifetime refusal above; the
// group walk's closure refusal). `compute_activation_adopts` claims it for the executing
// activation's owner node: `RegionInfo::activation_adopt_sites` maps the SCC's
// enclosing-scope adopt site to its members, and BOTH members' own decrefs are
// suppressed, the node's completion release being their sole demise
// (docs/impl/region-model.md § "Owner nodes" — "The capture-back-edge SCC"). These pins
// read the flag-on `RegionInfo` (the lowerer's view), so they pin the wiring —
// admission, suppression, and the map — not just the walk.

/// The regions of the sole activation-adopt site, asserting there is exactly one
/// site; returns (site members as a set).
fn sole_activation_site(info: &RegionInfo) -> rustc_hash::FxHashSet<Region> {
    assert_eq!(
        info.activation_adopt_sites.len(),
        1,
        "expected exactly one activation-adopt site; got {:?}",
        info.activation_adopt_sites,
    );
    info.activation_adopt_sites
        .values()
        .flatten()
        .copied()
        .collect()
}

#[test]
fn activation_adopts_capture_back_edge_scc() {
    // The ROOTED shape (the runtime pin's): `root ⊇ m` (store), `m ⊇ c` (store),
    // `c ⊇ m` (capture). The m↔c SCC is admitted to the activation node — root is the
    // hull (it dies in-activation, keeping its own baseline release) and is NOT a
    // member. Both members' own decrefs are suppressed.
    let (hir, info) = analyze_full(
        "(begin (let [root (@array) m (@array)] \
                  (let [c (fn [] (length m))] \
                    (begin (%array-push m c) (c) (%array-push root m) nil))) \
                nil)",
    );
    let c = sole_closure_region(&hir, &info);
    let members = sole_activation_site(&info);
    assert!(
        members.contains(&c),
        "the capturing closure r{} must be an activation-adopt member; got {members:?}",
        c.0,
    );
    // `m` is the store target of the interior `m ⊇ c` edge.
    let m = info
        .cross_region_refs
        .iter()
        .find(|(site, s, _)| !info.hard_edge_sites.contains(site) && *s == c)
        .map(|&(_, _, d)| d)
        .expect("precondition: a store edge m ⊇ c");
    assert!(
        members.contains(&m),
        "the captured container r{} must be an activation-adopt member; got {members:?}",
        m.0,
    );
    assert_eq!(members.len(), 2, "exactly the m↔c SCC; got {members:?}");
    for &r in &[m, c] {
        assert!(
            info.suppressed_decref_regions.contains(&r),
            "member r{}'s own decref must be suppressed (the suppress ⊆ adopt \
             contract) — the node's completion release is its sole demise",
            r.0,
        );
    }
    // The hull container `root` keeps its baseline: not a member, not suppressed.
    let root = info
        .cross_region_refs
        .iter()
        .find(|(site, s, _)| !info.hard_edge_sites.contains(site) && *s == m)
        .map(|&(_, _, d)| d)
        .expect("precondition: a store edge root ⊇ m");
    assert!(
        !members.contains(&root) && !info.suppressed_decref_regions.contains(&root),
        "the hull container r{} keeps its own baseline release",
        root.0,
    );

    // The BARE shape (no root): the SCC alone is externally unique — same admission.
    let (hir, info) = analyze_full(
        "(begin (let [m (@array)] \
                  (let [c (fn [] (length m))] \
                    (begin (%array-push m c) (c) nil))) \
                nil)",
    );
    let c = sole_closure_region(&hir, &info);
    let members = sole_activation_site(&info);
    assert!(
        members.contains(&c) && members.len() == 2,
        "the bare m↔c SCC must be admitted whole; got {members:?}",
    );
}

#[test]
fn activation_adopts_funnel_recovered_scc_checked_on() {
    // The F-b face: on the checked-on (native-Call) production path the store
    // `m ⊇ c` is an opaque `Funnel` call recording NO `cross_region_refs` edge —
    // the containment reaches the inference only as a funnel-recovered
    // `containment_edges` entry. The signature's store half must count it, and
    // the emit needs no store site (the adopt is value-resolved), so the SCC is
    // admitted exactly as on the intrinsic path.
    let (hir, info) = analyze_full_checked_on(
        "(begin (let [m (@array)] \
                  (let [c (fn [] (length m))] \
                    (begin (%array-push m c) (c) nil))) \
                nil)",
    );
    let c = sole_closure_region(&hir, &info);
    assert!(
        info.containment_edges.iter().any(|&(_, s, _)| s == c),
        "precondition: checked-on, the m ⊇ c store must be funnel-recovered \
         containment (no cross_region_refs edge); containment={:?}",
        info.containment_edges,
    );
    let members = sole_activation_site(&info);
    assert!(
        members.contains(&c) && members.len() == 2,
        "the funnel-recovered m↔c SCC must be admitted on the checked-on path; \
         got {members:?}",
    );
}

#[test]
fn activation_adopt_excludes_other_mechanisms() {
    // Disjointness (the one-owner invariant at the emit level): shapes owned by the
    // OTHER mechanisms must admit nothing here.
    //
    // (a) The letrec closure-cycle MERGE's shape (a capture-only SCC — no interior
    // store edge): the signature refuses, the merge keeps sole ownership.
    let (_, info) = analyze_full(
        "(begin (letrec [ping (fn [n] (if (%lt n 1) :done (pong (%sub n 1)))) \
                         pong (fn [n] (ping n))] \
                  (ping 3)) \
                nil)",
    );
    assert!(
        info.activation_adopt_sites.is_empty(),
        "a capture-only letrec SCC belongs to the closure-cycle merge, not the \
         activation cut; got {:?}",
        info.activation_adopt_sites,
    );
    // (b) The co-owned group's shape (a store-only bare @array cycle — no capture
    // edge): the signature refuses, the group free keeps sole ownership.
    let (_, info) = analyze_full(
        "(begin (let [a (@array) b (@array)] \
                  (begin (%array-push a b) (%array-push b a) nil)) \
                nil)",
    );
    assert!(
        info.activation_adopt_sites.is_empty(),
        "a store-only bare cycle belongs to the co-owned group free, not the \
         activation cut; got {:?}",
        info.activation_adopt_sites,
    );
    assert!(
        !info.owned_group_members.is_empty(),
        "precondition: the bare cycle IS claimed by the group cut",
    );
    // (c) The upvalue closure-web family (capture-only edges through nested
    // closures): the signature refuses — the family stays on the baseline until
    // its own cut (class 4 admission / class 6).
    let (_, info) = analyze_full(
        "(begin (let [m (%pair 1 2)] \
                  (letrec [e (fn [] (let [o (fn [] (begin (e) (%first m)))] (o)))] (e))) \
                nil)",
    );
    assert!(
        info.activation_adopt_sites.is_empty(),
        "the upvalue closure-web family must not be claimed by the activation cut; \
         got {:?}",
        info.activation_adopt_sites,
    );
}

#[test]
fn activation_adopt_refuses_escaping_hull() {
    // The hull gate: the SCC's members free at the activation's completion, so
    // every region referencing INTO the SCC must itself die in-activation. Here
    // the holding container `root` is RETURNED — it flows to the program tail
    // (the return frontier), so it outlives the activation and freeing m at the
    // activation's completion would leave root's contents dangling for the
    // caller. The SCC must refuse to Shared.
    let (_, info) = analyze_full(
        "(let [root (@array) m (@array)] \
           (let [c (fn [] (length m))] \
             (begin (%array-push m c) (c) (%array-push root m) root)))",
    );
    assert!(
        info.activation_adopt_sites.is_empty(),
        "an SCC whose holder escapes (a returned root) must refuse the activation \
         adopt; got {:?}",
        info.activation_adopt_sites,
    );
}

// ── ownership inference: the transferred-returned-subtree cut ───────────────
//
// A producer lambda builds an externally-unique cyclic subtree and returns its
// root; the consumer discards it. No region root can own it (the root crosses
// the return frontier) and per-region RC cannot collect the cycle, so
// `compute_transfer_adopts` claims it for the CONSUMING activation's owner
// node: the producer's interior owner edges merge into the ordinary adopt maps
// and each consumer site's call-result region lands in
// `RegionInfo::transfer_adopt_regions`, whose release the lowerer replaces
// with `AdoptIntoActivation` (docs/impl/region-model.md § "Owner nodes" — "The
// transferred returned subtree"). These pins read the fully-analyzed `RegionInfo`
// (the lowerer's view), so they pin the wiring — not just the walk.

/// The two mutually-referencing cycle regions of a compiled shape — the
/// endpoints of its non-hard `cross_region_refs` edges (intrinsic face) or its
/// funnel-recovered `containment_edges` (checked-on face).
fn cycle_pair(info: &RegionInfo) -> rustc_hash::FxHashSet<Region> {
    let mut endpoints: rustc_hash::FxHashSet<Region> = rustc_hash::FxHashSet::default();
    for &(site, s, d) in &info.cross_region_refs {
        if !info.hard_edge_sites.contains(&site) {
            endpoints.insert(s);
            endpoints.insert(d);
        }
    }
    for &(_site, s, d) in &info.containment_edges {
        endpoints.insert(s);
        endpoints.insert(d);
    }
    endpoints
}

#[test]
fn transfer_adopts_returned_cycle_to_consumer() {
    // The call face, intrinsic (`--checked-intrinsics=off` test default) path:
    // a let-bound producer returning an a↔b cycle, one discarded consumer site.
    let (_, info) = analyze_full(
        "(begin (let [mk (fn [] (let [a (@array) b (@array)] \
                  (begin (%array-push a b) (%array-push b a) a)))] \
                  (begin (mk) nil)) \
                nil)",
    );
    assert_eq!(
        info.transfer_adopt_regions.len(),
        1,
        "the one discarded consumer site's result region must be transfer-adopted; \
         got {:?}",
        info.transfer_adopt_regions,
    );
    let &r = info.transfer_adopt_regions.iter().next().unwrap();
    assert!(
        info.call_result_regions.contains(&r),
        "the transfer-adopted region r{} is the consumer's call-result placeholder",
        r.0,
    );
    // The producer's interior owner edge: the non-root cycle member adopted by
    // the returned root — exactly one edge, its endpoints the cycle pair.
    let adopts: Vec<(Region, Region)> =
        info.owned_adopt_edges.values().flatten().copied().collect();
    let pair = cycle_pair(&info);
    assert_eq!(
        adopts.len(),
        1,
        "exactly one interior owner edge (member → returned root); got {adopts:?}",
    );
    let (m, owner) = adopts[0];
    assert!(
        m != owner && pair.contains(&m) && pair.contains(&owner),
        "the interior adopt links the two cycle regions (pair {pair:?}); got \
         ({}, {})",
        m.0,
        owner.0,
    );
    // A store-adopted interior member keeps its own (no-op) release — the
    // suppress ⊆ adopt contract applies only to capture members.
    assert!(
        !info.suppressed_decref_regions.contains(&m),
        "a store-edge interior member keeps its own release (a structural no-op \
         on an Owned region)",
    );
}

#[test]
fn transfer_adopts_returned_cycle_checked_on() {
    // The F-b face: checked-on, the interior store is an opaque `Funnel` call —
    // the containment is funnel-recovered and the interior adopt is keyed at
    // the funnel CALL site (the value-resolved adopt needs no store opcode), so
    // the cut admits the production path exactly as the intrinsic path.
    let (_, info) = analyze_full_checked_on(
        "(begin (let [mk (fn [] (let [a (@array) b (@array)] \
                  (begin (%array-push a b) (%array-push b a) a)))] \
                  (begin (mk) nil)) \
                nil)",
    );
    assert!(
        !info.containment_edges.is_empty(),
        "precondition: checked-on, the interior stores are funnel-recovered \
         containment; containment={:?}",
        info.containment_edges,
    );
    assert_eq!(
        info.transfer_adopt_regions.len(),
        1,
        "the consumer site must be transfer-adopted on the checked-on path; got {:?}",
        info.transfer_adopt_regions,
    );
    let adopts: Vec<(Region, Region)> =
        info.owned_adopt_edges.values().flatten().copied().collect();
    assert_eq!(
        adopts.len(),
        1,
        "the interior owner edge must be emittable at its funnel call site; got \
         {adopts:?}",
    );
}

#[test]
fn transfer_adopts_fiber_terminal_cycle() {
    // The fiber face: a silent body's terminal value is the returned cycle; the
    // completing resume hands it to the consumer, whose site is gated exactly
    // like a call-face site.
    let (_, info) = analyze_full(
        "(begin (let [f (fiber/new (fn [] (let [a (@array) b (@array)] \
                  (begin (%array-push a b) (%array-push b a) a))) 1)] \
                  (begin (fiber/resume f) nil)) \
                nil)",
    );
    assert_eq!(
        info.transfer_adopt_regions.len(),
        1,
        "the discarded resume site's result region must be transfer-adopted; got {:?}",
        info.transfer_adopt_regions,
    );
    let adopts: Vec<(Region, Region)> =
        info.owned_adopt_edges.values().flatten().copied().collect();
    assert_eq!(
        adopts.len(),
        1,
        "the fiber body's interior owner edge must be admitted; got {adopts:?}",
    );
}

#[test]
fn transfer_adopt_refuses_unsafe_shapes() {
    // Each gate refuses to the always-legal baseline: no transfer region, no
    // interior adopt. One inadmissible consumer site refuses the whole callee.
    let shapes = [
        // (a) a USED consumer: the holder is read outside the Immediate-native
        // allowance (an extraction alias could outlive the node's horizon).
        // The read feeds a branch condition so it cannot be eliminated.
        "(begin (let [mk (fn [] (let [a (@array) b (@array)] \
           (begin (%array-push a b) (%array-push b a) a)))] \
           (let [r (mk)] (if (%first r) 1 2))) \
         nil)",
        // (a') the same read as a bare statement — the result-flow holder gate
        // must see the intrinsic read regardless of what consumes its value.
        "(begin (let [mk (fn [] (let [a (@array) b (@array)] \
           (begin (%array-push a b) (%array-push b a) a)))] \
           (let [r (mk)] (begin (%first r) nil))) \
         nil)",
        // (b) a RETURNED consumer: the site's result crosses the return
        // frontier (the tail call in `outer`), refusing every site of mk.
        "(begin (let [mk (fn [] (let [a (@array) b (@array)] \
           (begin (%array-push a b) (%array-push b a) a)))] \
           (let [outer (fn [] (mk))] (begin (outer) nil))) \
         nil)",
        // (c) an ESCAPING MEMBER: `b` is also stored into an outer container,
        // so the subtree is not externally unique.
        "(begin (let [keep (@array)] \
           (let [mk (fn [] (let [a (@array) b (@array)] \
             (begin (%array-push a b) (%array-push b a) (%array-push keep b) a)))] \
             (begin (mk) nil))) \
         nil)",
        // (d) an ACYCLIC returned subtree: the RC cascade already reclaims it
        // promptly; adopting would only trade promptness away.
        "(begin (let [mk (fn [] (let [a (@array) b (@array)] \
           (begin (%array-push a b) a)))] \
           (begin (mk) nil)) \
         nil)",
        // (e) a YIELDING fiber body: a resume can deliver a non-terminal value,
        // so the fiber face's signal gate refuses.
        "(begin (let [f (fiber/new (fn [] (begin (emit :yield 0) \
           (let [a (@array) b (@array)] \
             (begin (%array-push a b) (%array-push b a) a)))) 3)] \
           (begin (fiber/resume f) (fiber/resume f) nil)) \
         nil)",
    ];
    for src in shapes {
        let (_, info) = analyze_full(src);
        assert!(
            info.transfer_adopt_regions.is_empty(),
            "an unsafe transfer shape must refuse (no consumer adopt); got {:?} \
             for src={src}",
            info.transfer_adopt_regions,
        );
        let adopts: Vec<(Region, Region)> =
            info.owned_adopt_edges.values().flatten().copied().collect();
        assert!(
            adopts.is_empty(),
            "a refused transfer shape must emit no interior adopts; got {adopts:?} \
             for src={src}",
        );
    }
}

#[test]
fn closure_web_capture_not_yet_claimed() {
    // Boundary lock: a closure-web — mutually-recursive closures over a shared captured
    // value, the scheduler in miniature — is NOT yet claimed as an Owned subtree: the
    // shared value's tight last-use resolves through the sibling capture chain one step
    // past the root closure's drop, so the lifetime obligation refuses it. The emit side
    // is ready (the capture adopt reloads through slot or env alike); admission awaits
    // the owner = nearest dominating activation cut, whose node outlives every capturer.
    // When that cut claims the web this assertion changes, forcing the author to confirm
    // the claimed members reclaim soundly (the `lower_lambda_expr` debug_assert and the
    // one-owner runtime assert are the backstops).
    let (_, _, edges) = adopt_edges(
        "(begin (let [b (%pair 1 2)] \
                  (letrec [f (fn [] (begin (g) (%first b))) g (fn [] (%first b))] (f))) \
                nil)",
    );
    assert!(
        edges.capture.is_empty(),
        "a closure-web is not yet an Owned subtree — expected no capture adopt edges \
         (the owner = activation cut will claim it); got {:?}",
        edges.capture,
    );
}
