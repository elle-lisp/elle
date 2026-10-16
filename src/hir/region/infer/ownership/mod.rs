// audited: 2026-10-14
//! Ownership inference: which regions an Owned subtree frees as one unit, and which stay Shared on per-region RC.
//!
//! docs/impl/region/ownership.md
//! docs/impl/region/adopt.md
//! docs/impl/escape.md
//!
//! The passes, in the order `analyze::adopt::apply_ownership` runs them:
//!
//! - `seeds` — the Shared seeds: the regions a value crosses the return or
//!   fiber frontier through. A seed is a frontier crossing, never a containment
//!   edge. A value stored into or captured by a local aggregate stays inside
//!   it, and whether that aggregate crosses a frontier decides the value's fate.
//! - `capture` — the capture containment edges, re-derived from the HIR because
//!   the walk records no `cross_region_refs` edge for a capture.
//! - `inputs` — the containment graph, candidate set and capture edges every
//!   pass reads, built once per compile.
//! - `subtree` — the externally-unique subtrees, and the co-owned-cycle groups.
//! - `aliases` — the alias relations the lifetime obligation closes over.
//! - `adopt` — the adopt edges per emit site, under the lifetime obligation.
//! - `transfer` — the transferred-returned-subtree cut.
//! - `activation` — the activation-owner cut over a capture-back-edge SCC.
//!
//! Every pass may over-mark Shared and never under-mark it. A Shared region
//! keeps the per-region RC baseline, and an escaping region claimed Owned is
//! freed under a live reference. A facet no pass covers is a shape every pass
//! refuses.

mod activation;
mod adopt;
mod aliases;
mod capture;
mod inputs;
mod seeds;
mod subtree;
mod transfer;

// The forest emit (`analyze_regions_with`) consumes these four. Each takes the shared
// `OwnershipInputs` (the containment graph + candidate set + capture edges) by reference,
// built ONCE per compile in `apply_ownership` (`ownership_inputs`) rather than rebuilt per
// pass — the inputs derive only from fields unchanged across the ownership passes.
pub(in crate::hir::region::infer) use activation::compute_activation_adopts;
pub(in crate::hir::region::infer) use adopt::compute_adopt_edges;
pub(in crate::hir::region::infer) use inputs::ownership_inputs;
pub(in crate::hir::region::infer) use subtree::compute_owned_region_groups;
pub(in crate::hir::region::infer) use transfer::compute_transfer_adopts;
// The remaining stages are exercised directly by the `region::infer::tests` harness; the
// re-export keeps their `ownership::NAME` path stable for it without dangling in a
// normal build (each is reached internally via its own submodule path).
//
// `compute_shared_seeds` (frontier crossings: return / emit / send — NOT capture) is one
// of them: it reaches a shipping build through `ownership_inputs`, and the harness pins
// the projection itself (`shared_seed_*`). The closure-cycle merge deliberately does NOT
// read the combined set — it reads the two halves separately, because it admits the
// return facet and refuses only the fiber one (docs/impl/region/letrec.md § The frontier
// gate).
#[cfg(test)]
pub(super) use {
    adopt::AdoptEdges, capture::capture_containment_edges, seeds::compute_shared_seeds,
    subtree::compute_owned_subtrees,
};
