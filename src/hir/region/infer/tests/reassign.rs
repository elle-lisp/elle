// audited: 2026-09-22
//! The mutable-reassign 1-slot-container gate, one file per question it decides.
//!
//! docs/impl/region/bindings.md
//!
//! The sole-held question is asked exactly where the model claims a reference
//! UNCOUNTED — the module-scope cell's every region (which must also not be
//! returned: two static owners of one reference is a double-free), and the
//! fn-local cell's init. A fn-local cell's STORED values ask only that the
//! store run once per binding of every name the store reads: the counted store
//! claims nothing from anyone, and the pin rule's maximum orders each producer
//! release after every alias's read.
//!
//! Runtime-counted escapes — container stores, captures, opaque-call cliques —
//! are value-based-balanced and MUST keep the gate: refusing them regresses
//! region-mutable-reassign-{selfref,branch,flow} and
//! region-toplevel-{mutable-reassign,reassign-thunk-uaf} straight into UAFs.
//! The `keeps`/`counts` tests are the counterfactual pins against that
//! over-exclusion.

use super::*;

// One file per question, each holding its own counterfactual beside the
// admission it bounds.
mod chain;
mod counted;
mod gate;
mod pins;
mod reads;
mod returned;

/// The heap-carrying reassigned binding of a shape and its assign sites. Every
/// loop-carried-cell shape below also reassigns an immediate loop counter, whose
/// own gate always succeeds (it holds no region to be double-claimed); asserting
/// over every reassign site would read the counter's verdict instead of the
/// cell's.
fn heap_carrying_reassign(hir: &Hir, info: &RegionInfo) -> (Binding, Vec<HirId>) {
    let sites = find_reassign_sites(hir);
    let b = sites
        .iter()
        .map(|(_, b)| *b)
        .find(|b| {
            info.binding_source_regions
                .get(b)
                .is_some_and(|rs| !rs.is_empty())
        })
        .expect("shape must contain a heap-carrying reassign");
    let ids = sites
        .iter()
        .filter(|(_, s)| *s == b)
        .map(|(id, _)| *id)
        .collect();
    (b, ids)
}

/// Two sequential loops over one binding, with an extra `keep` alias or a
/// trailing read spliced in by the caller. Every shape below is the same chain:
/// functionalization gives the source name one version per loop and initializes
/// each from the previous, so the middle version carries a cell of its own
/// (docs/impl/region/bindings.md § "A chain of forwarding edges hands one
/// reference along, so the fold follows it whole").
fn two_loop_chain(between: &str, tail: &str) -> (Hir, RegionInfo) {
    let (hir, _, info) = pipeline(&format!(
        "(def @h (fn (n)\n\
           (begin (var last (array 0 0))\n\
                  (var i 0)\n\
                  (while (%lt i n)\n\
                    (begin (assign last (array i 7))\n\
                           (assign i (%add i 1))))\n\
                  {between}\n\
                  (while (%lt i (%mul n 2))\n\
                    (begin (assign last (array i 9))\n\
                           (assign i (%add i 1))))\n\
                  {tail})))\n\
         (h 3)"
    ));
    (hir, info)
}

/// The chain's links, upstream first — the heap-carrying reassigned bindings, in
/// tree order. The immediate loop counter carries no region and is not one.
fn chain_links(hir: &Hir, info: &RegionInfo) -> Vec<Binding> {
    let mut links: Vec<Binding> = Vec::new();
    for (_, b) in find_reassign_sites(hir) {
        let heap = info
            .binding_source_regions
            .get(&b)
            .is_some_and(|rs| !rs.is_empty());
        if heap && !links.contains(&b) {
            links.push(b);
        }
    }
    links
}
