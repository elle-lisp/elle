// audited: 2026-10-21
//! The activation-owner cut: a capture-back-edge cycle, adopted by the activation that runs it.
//!
//! docs/impl/region/owner.md

use super::*;

// ── ownership inference: the activation-owner cut (capture-back-edge SCC) ───────
//
// The m↔c capture-back-edge SCC, a container captured by a closure it holds, is the
// cycle neither region-rooted mode can own. `compute_activation_adopts` claims it for
// the executing activation's owner node. `RegionInfo::activation_adopt_sites` maps the
// SCC's enclosing-scope adopt site to its members, and BOTH members' own decrefs are
// suppressed, because the node's completion release is their sole demise. These pins
// read the whole `RegionInfo` (the lowerer's view), so they pin the wiring
// (admission, suppression, and the map), not just the walk.

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
    // Precondition: the `m ⊇ c` store is an opaque funnel call, so the containment is
    // funnel-recovered (no `cross_region_refs` edge exists at the store site). The
    // signature's store half must count it. The emit needs no store opcode, because the
    // adopt is value-resolved at the enclosing-scope site.
    let m = info
        .containment_edges
        .iter()
        .find(|(_, s, _)| *s == c)
        .map(|&(_, _, d)| d)
        .expect("precondition: a funnel-recovered store edge m ⊇ c");
    let members = sole_activation_site(&info);
    assert!(
        members.contains(&c),
        "the capturing closure r{} must be an activation-adopt member; got {members:?}",
        c.0,
    );
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
        .containment_edges
        .iter()
        .find(|(_, s, _)| *s == m)
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
    assert!(
        info.containment_edges.iter().any(|&(_, s, _)| s == c),
        "precondition: the m ⊇ c store must be funnel-recovered containment (no \
         cross_region_refs edge); containment={:?}",
        info.containment_edges,
    );
    let members = sole_activation_site(&info);
    assert!(
        members.contains(&c) && members.len() == 2,
        "the bare m↔c SCC must be admitted whole; got {members:?}",
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
    // closures): the signature refuses, and the family stays on the baseline.
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
    // the holding container `root` is RETURNED: it flows to the program tail
    // (the return frontier), so it outlives the activation. Freeing m at the
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

/// The Emit node of a shape carrying exactly one `(emit …)` — the park whose
/// position the park-split pins compare against.
fn sole_emit(hir: &Hir) -> HirId {
    fn walk(h: &Hir, out: &mut Vec<HirId>) {
        if matches!(h.kind, HirKind::Emit { .. }) {
            out.push(h.id);
        }
        h.for_each_child(|c| walk(c, out));
    }
    let mut emits = Vec::new();
    walk(hir, &mut emits);
    assert_eq!(emits.len(), 1, "shape must carry exactly one emit");
    emits[0]
}

/// The allocation HirId of `r`, from the walk's `alloc_region` inversion.
fn alloc_site_of(info: &RegionInfo, r: Region) -> HirId {
    info.alloc_region
        .iter()
        .find(|(_, &reg)| reg == r)
        .map(|(&h, _)| h)
        .expect("member has an allocation site")
}

#[test]
fn activation_adopt_sites_ahead_of_park() {
    // The park split (docs/impl/region/owner.md): a park inside the adopt scope,
    // AFTER the members' allocations, keeps the SCC admitted. The walk keys a
    // SECOND, earlier adopt after the last member allocation. So the members are
    // Owned, riding the parked frame's owner node, before the park can strand
    // them. The scope-exit site stays beside it (the channel is idempotent on an
    // Owned member).
    let (hir, info) = analyze_full(
        "(begin (let [root (@array) m (@array)] \
                  (let [c (fn [] (length m))] \
                    (begin (%array-push m c) (c) (%array-push root m) \
                           (emit :yield 0) nil))) \
                nil)",
    );
    let c = sole_closure_region(&hir, &info);
    let m = info
        .containment_edges
        .iter()
        .find(|(_, s, _)| *s == c)
        .map(|&(_, _, d)| d)
        .expect("precondition: a funnel-recovered store edge m ⊇ c");
    assert_eq!(
        info.activation_adopt_sites.len(),
        2,
        "a parking adopt scope carries the early key AND the scope-exit site; \
         got {:?}",
        info.activation_adopt_sites,
    );
    let expected: rustc_hash::FxHashSet<Region> = [m, c].into_iter().collect();
    for members in info.activation_adopt_sites.values() {
        let set: rustc_hash::FxHashSet<Region> = members.iter().copied().collect();
        assert_eq!(
            set, expected,
            "both sites adopt the whole m↔c SCC (the second adopt is a no-op \
             on an Owned member)",
        );
    }
    for &r in &[m, c] {
        assert!(
            info.suppressed_decref_regions.contains(&r),
            "member r{}'s own decref stays suppressed under the park split",
            r.0,
        );
    }
    // The ordering that closes the leak: every member allocation completes at
    // or before the early key, and the early key precedes the park; the
    // scope-exit site post-dominates the park. Post-order agrees with
    // execution order here — the compared nodes sit in sibling constituents.
    let order = compute_order(&hir);
    let ord = |id: HirId| order.get(&id).copied().expect("ordered node");
    let emit = ord(sole_emit(&hir));
    let mut sites: Vec<u32> = info
        .activation_adopt_sites
        .keys()
        .map(|&s| ord(s))
        .collect();
    sites.sort_unstable();
    let (early, late) = (sites[0], sites[1]);
    assert!(
        early < emit && emit < late,
        "the early key must precede the park and the scope-exit site must \
         follow it (early={early}, emit={emit}, late={late})",
    );
    for &r in &[m, c] {
        assert!(
            ord(alloc_site_of(&info, r)) <= early,
            "member r{}'s allocation must complete by the early key",
            r.0,
        );
    }

    // A park ordered BEFORE every member allocation strands nothing (no member
    // exists when it parks). Multi-binding `let` desugars to nested
    // single-binding lets (src/hir/AGENTS.md), so this park sits in an
    // ENCLOSING scope's init, outside the adopt site's subtree. The SCC stays
    // admitted with the single scope-exit site, exactly as with no park.
    let (_, info) = analyze_full(
        "(begin (let [z (emit :yield 0) root (@array) m (@array)] \
                  (let [c (fn [] (length m))] \
                    (begin (%array-push m c) (c) (%array-push root m) nil))) \
                nil)",
    );
    assert_eq!(
        info.activation_adopt_sites.len(),
        1,
        "a park before every member allocation must not refuse or double-site \
         the SCC; got {:?}",
        info.activation_adopt_sites,
    );
}

#[test]
fn activation_adopt_refuses_park_between_allocations() {
    // A park BETWEEN two members' allocations: no adopt point covers the
    // already-allocated member across the park, so the park split cannot order
    // the shape. The SCC refuses to Shared: no site and no suppression, so the
    // members keep their baseline releases.
    let (hir, info) = analyze_full(
        "(begin (let [m (@array)] \
                  (begin (emit :yield 0) \
                         (let [c (fn [] (length m))] \
                           (begin (%array-push m c) (c) nil)))) \
                nil)",
    );
    assert!(
        info.activation_adopt_sites.is_empty(),
        "a park between the members' allocations must refuse the SCC; got {:?}",
        info.activation_adopt_sites,
    );
    let c = sole_closure_region(&hir, &info);
    assert!(
        !info.suppressed_decref_regions.contains(&c),
        "a refused SCC leaves no suppression behind (suppress ⊆ adopt)",
    );

    // The control twin — the same nesting with no park — admits, so the
    // refusal above is attributable to the park alone.
    let (_, info) = analyze_full(
        "(begin (let [m (@array)] \
                  (begin nil \
                         (let [c (fn [] (length m))] \
                           (begin (%array-push m c) (c) nil)))) \
                nil)",
    );
    assert!(
        !info.activation_adopt_sites.is_empty(),
        "precondition: the no-park twin is admitted",
    );
}

#[test]
fn closure_web_capture_not_yet_claimed() {
    // Boundary lock: a closure-web is NOT claimed as an Owned subtree. That is
    // mutually-recursive closures over a shared captured value, the scheduler in
    // miniature. The shared value's tight last-use resolves through the sibling capture
    // chain one step past the root closure's drop, so the lifetime obligation refuses it.
    // The emit side could adopt it, because the capture adopt reloads through slot or env
    // alike. Only an owner that outlives every capturer could own the web. When a cut
    // claims it, this assertion changes, and the author must confirm the claimed members
    // reclaim soundly. The `lower_lambda_expr` debug_assert and the one-owner runtime
    // assert are the backstops.
    let (_, _, edges) = adopt_edges(
        "(begin (let [b (%pair 1 2)] \
                  (letrec [f (fn [] (begin (g) (%first b))) g (fn [] (%first b))] (f))) \
                nil)",
    );
    assert!(
        edges.capture.is_empty(),
        "a closure-web is not an Owned subtree: expected no capture adopt edges; got {:?}",
        edges.capture,
    );
}
