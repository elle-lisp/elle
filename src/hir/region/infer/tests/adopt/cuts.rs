use super::*;

// ── ownership inference: the activation-owner cut (capture-back-edge SCC) ───────
//
// The m↔c capture-back-edge SCC — a container captured by a closure it holds — is the
// cycle neither region-rooted mode can own (the owner-aware lifetime refusal above; the
// group walk's closure refusal). `compute_activation_adopts` claims it for the executing
// activation's owner node: `RegionInfo::activation_adopt_sites` maps the SCC's
// enclosing-scope adopt site to its members, and BOTH members' own decrefs are
// suppressed, the node's completion release being their sole demise
// (docs/impl/region/owner.md § "Owner nodes" — "The capture-back-edge SCC"). These pins
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
    // Precondition: the `m ⊇ c` store is an opaque funnel call — the containment is
    // funnel-recovered (no `cross_region_refs` edge exists at the store site), and the
    // signature's store half must count it (the emit needs no store opcode: the adopt
    // is value-resolved at the enclosing-scope site).
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
    // The park split (docs/impl/region/owner.md § "Owner nodes" — "The park
    // split"): a park inside the adopt scope, AFTER the members' allocations,
    // keeps the SCC admitted — the walk keys a SECOND, earlier adopt after the
    // last member allocation, so the members are Owned (riding the parked
    // frame's owner node) before the park can strand them. The scope-exit site
    // stays beside it (the channel is idempotent on an Owned member).
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
    // single-binding lets (hir/AGENTS.md invariant 10), so this park sits in
    // an ENCLOSING scope's init, outside the adopt site's subtree: the SCC
    // stays admitted with the single scope-exit site, exactly as with no park.
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
    // the shape and the SCC refuses to Shared — no site, no suppression (the
    // members keep their baseline releases).
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

// ── ownership inference: the transferred-returned-subtree cut ───────────────
//
// A producer lambda builds an externally-unique cyclic subtree and returns its
// root; the consumer discards it. No region root can own it (the root crosses
// the return frontier) and per-region RC cannot collect the cycle, so
// `compute_transfer_adopts` claims it for the CONSUMING activation's owner
// node: the producer's interior owner edges merge into the ordinary adopt maps
// and each consumer site's call-result region lands in
// `RegionInfo::transfer_adopt_regions`, whose release the lowerer replaces
// with `AdoptIntoActivation` (docs/impl/region/owner.md § "Owner nodes" — "The
// transferred returned subtree"). These pins read the fully-analyzed `RegionInfo`
// (the lowerer's view), so they pin the wiring — not just the walk.

/// The two mutually-referencing cycle regions of a compiled shape — the
/// endpoints of its funnel-recovered `containment_edges` (the `%`-stores are
/// opaque funnel calls, so no `cross_region_refs` edge names them).
fn cycle_pair(info: &RegionInfo) -> rustc_hash::FxHashSet<Region> {
    let mut endpoints: rustc_hash::FxHashSet<Region> = rustc_hash::FxHashSet::default();
    for &(_site, s, d) in &info.containment_edges {
        endpoints.insert(s);
        endpoints.insert(d);
    }
    endpoints
}

#[test]
fn transfer_adopts_returned_cycle_to_consumer() {
    // The call face: a let-bound producer returning an a↔b cycle, one discarded
    // consumer site. The interior stores are opaque funnel calls, so the
    // containment is funnel-recovered and the interior adopt is keyed at the
    // funnel CALL site (the value-resolved adopt needs no store opcode).
    let (_, info) = analyze_full(
        "(begin (let [mk (fn [] (let [a (@array) b (@array)] \
                  (begin (%array-push a b) (%array-push b a) a)))] \
                  (begin (mk) nil)) \
                nil)",
    );
    assert!(
        !info.containment_edges.is_empty(),
        "precondition: the interior stores are funnel-recovered containment; \
         containment={:?}",
        info.containment_edges,
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
        // The read feeds a branch condition so it cannot be eliminated; the
        // `%type-of` match arm proves `r`'s container family for the `%get`
        // (prove-or-reject, typeinfer/contract.rs).
        "(begin (let [mk (fn [] (let [a (@array) b (@array)] \
           (begin (%array-push a b) (%array-push b a) a)))] \
           (let [r (mk)] (match (%type-of r) :@array (if (%get r 0) 1 2) _ 2))) \
         nil)",
        // (a') the same read as a bare statement — the result-flow holder gate
        // must see the intrinsic read regardless of what consumes its value.
        "(begin (let [mk (fn [] (let [a (@array) b (@array)] \
           (begin (%array-push a b) (%array-push b a) a)))] \
           (let [r (mk)] (begin (match (%type-of r) :@array (%get r 0) _ nil) nil))) \
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
