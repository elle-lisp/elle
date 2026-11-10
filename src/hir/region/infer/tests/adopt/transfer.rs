// audited: 2026-10-21
//! The transferred-returned-subtree cut: a producer's returned cycle, adopted by the activation that consumes it.
//!
//! docs/impl/region/owner.md

use super::*;

// ── ownership inference: the transferred-returned-subtree cut ───────────────
//
// A producer builds an externally-unique cyclic subtree and returns its root;
// the consumer discards it. No region root can own it and per-region RC cannot
// collect the cycle. So `compute_transfer_adopts` claims it for the CONSUMING
// activation's owner node. The producer's interior owner edges merge into the
// ordinary adopt maps, and each consumer's release lands in
// `RegionInfo::transfer_adopt_regions`, which the lowerer replaces with
// `AdoptIntoActivation`. The producer is a lambda on the call and fiber faces,
// and a binding's initializer on the inline face. These pins read the
// fully-analyzed `RegionInfo` (the lowerer's view), so they pin the wiring —
// not just the walk.

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
        // (prove-or-reject, src/hir/typeinfer/contract.rs).
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

/// The inline face: a grafted producer leaves its `let` in the caller as a
/// binding's initializer, so no call site is left for the call face. The
/// initializer is the producer and the binding is the consumer, and the root's
/// own release is the one the lowerer replaces with `AdoptIntoActivation`.
///
/// The counter-factual: without the inline face no cut admits the shape. The
/// root has two holders, the `let`'s own binding and the consumer's, so the
/// subtree and group cuts refuse it, and the a↔b cycle leaks two regions per run.
#[test]
fn transfer_adopts_inline_returned_cycle() {
    let (_, info) = analyze_full(
        "(begin (let [x (let [a (@array) b (@array)] \
                  (begin (%array-push a b) (%array-push b a) a))] \
                  nil) \
                nil)",
    );
    let pair = cycle_pair(&info);
    assert_eq!(
        info.transfer_adopt_regions.len(),
        1,
        "the inline root must be transfer-adopted; got {:?}",
        info.transfer_adopt_regions,
    );
    let &root = info.transfer_adopt_regions.iter().next().unwrap();
    assert!(
        pair.contains(&root),
        "the transfer-adopted region r{} is the cycle's root itself (pair {pair:?})",
        root.0,
    );
    let adopts: Vec<(Region, Region)> =
        info.owned_adopt_edges.values().flatten().copied().collect();
    assert_eq!(
        adopts,
        pair.iter()
            .copied()
            .filter(|&m| m != root)
            .map(|m| (m, root))
            .collect::<Vec<_>>(),
        "exactly one interior owner edge: the other cycle member, owned by the root",
    );
}

/// The inline face's refusals, each to the always-legal baseline: no transfer
/// region and no interior adopt.
#[test]
fn transfer_adopt_refuses_unsafe_inline_shapes() {
    let shapes = [
        // (a) a READ consumer: the holder feeds an opcode read, whose alias could
        // outlive the node's horizon.
        "(begin (let [x (let [a (@array) b (@array)] \
           (begin (%array-push a b) (%array-push b a) a))] \
           (match (%type-of x) :@array (if (%get x 0) 1 2) _ 2)) \
         nil)",
        // (b) a STORED consumer: the holder goes into an outer container, so an
        // edge from outside names the root.
        "(begin (let [keep (@array)] \
           (let [x (let [a (@array) b (@array)] \
             (begin (%array-push a b) (%array-push b a) a))] \
             (begin (%array-push keep x) nil))) \
         nil)",
        // (c) an ESCAPING MEMBER: `b` is also stored into an outer container.
        "(begin (let [keep (@array)] \
           (let [x (let [a (@array) b (@array)] \
             (begin (%array-push a b) (%array-push b a) (%array-push keep b) a))] \
             nil)) \
         nil)",
        // (d) an ACYCLIC subtree: the RC cascade reclaims it promptly.
        "(begin (let [x (let [a (@array) b (@array)] \
           (begin (%array-push a b) a))] \
           nil) \
         nil)",
    ];
    for src in shapes {
        let (_, info) = analyze_full(src);
        assert!(
            info.transfer_adopt_regions.is_empty(),
            "an unsafe inline shape must refuse; got {:?} for src={src}",
            info.transfer_adopt_regions,
        );
        let adopts: Vec<(Region, Region)> =
            info.owned_adopt_edges.values().flatten().copied().collect();
        assert!(
            adopts.is_empty(),
            "a refused inline shape must emit no interior adopts; got {adopts:?} \
             for src={src}",
        );
    }
}
