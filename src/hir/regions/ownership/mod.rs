//! Ownership inference: classifying regions Owned vs Shared for the region forest.
//!
//! The forest's end-state frees an intra-fiber **Owned** subtree by a single
//! subtree drop — including any reference cycle interior to it — and reference-counts
//! only the regions that genuinely escape their fiber (**Shared**). The analysis
//! that draws that line is *escape*, computed over the canonical IR
//! ([escape.md](../../../docs/impl/escape.md), `crate::hir::EscapeInfo`).
//!
//! This module is the **front edge** of that inference: the *Shared-seed* set — the
//! regions a value escapes the activation/fiber **frontier** through, which
//! therefore cannot be Owned. Everything downstream (the externally-unique subtree
//! walk, owner = nearest dominating activation, the adopt/merge emit) is computed
//! relative to these seeds, so a correct, conservative seed set is the foundation.
//!
//! ## What is a seed — frontier crossings, not containment
//!
//! A seed is a **frontier crossing**: a region whose value leaves the
//! activation/fiber it was born in — by **return** (to the caller) or across a
//! **fiber boundary** (emit / channel send). It is deliberately **not** every
//! escape `EscapeInfo` records. The full `binding_escapes_activation` also folds in
//! two *containment* facets — **store** (the value goes into an aggregate's region)
//! and **capture** (the value goes into a closure's env region) — and those are
//! **edges that build the subtree**, not crossings that leave it. A region stored
//! into, or captured by, a *local* aggregate/closure stays inside the owning
//! subtree and reclaims with it; whether it is ultimately Shared is decided by
//! whether its *container* crosses a frontier — which the step-2 external-uniqueness
//! walk propagates over the containment graph (`cross_region_refs`), not this seed
//! pass. Why containment must not seed: a value captured by a *local* closure is
//! interior to that closure's subtree and reclaims with it, so seeding capture would
//! make every such value Shared and leak — the exact failure the forest exists to
//! avoid. Any program that builds a local capturing closure exercises this (a
//! mutually-recursive closure web is the general shape). So the binding-level facet
//! projected here is the **return** facet (`binding_escapes_via_return`), never the
//! full escape set.
//!
//! ## Conservative direction — over-mark, never under-mark
//!
//! Marking a region Shared when it is actually local is sound: a Shared region keeps
//! the unchanged per-region RC baseline. The danger is the reverse — letting a
//! genuinely-escaping region be claimed Owned would free it by subtree drop while a
//! reference still lives (a use-after-free). So a *consumer* of this set may claim a
//! region Owned only when it is provably outside every escape the set expresses; a
//! facet missing here is a reason a consumer must refuse the un-analyzed shape.
//!
//! ## Current coverage
//!
//! Both frontiers are projected to regions from escape's verdict (`regions::escape`,
//! `shared_seed_regions`):
//!
//! - **return**, two ways — binding-level (`binding_escapes_via_return` projected
//!   through `binding_source_regions`) and allocation-site
//!   (`escapes_return_frontier` projected through `alloc_region`, which catches an
//!   atomless tail return / returned lambda no binding holds).
//! - **fiber / emit** — escape's fiber facet: a value handed to the resumer by
//!   `(yield v)` / `(emit :sig v)` (its binding via `escapes_fiber`, an atomless
//!   `(yield (%pair …))` via `escapes_fiber_frontier`). The solver records no
//!   `cross_region_refs` edge at an `Emit` (the runtime incref in `handle_emit`
//!   keeps it alive), so the fiber crossing is purely escape's.
//! - **fiber / send** — escape's fiber facet, send half: a `Sends` native's message
//!   (`chan/send`) crosses to the receiving fiber. The dedicated `RegionEffect::Sends`
//!   is what distinguishes this fiber crossing from an ordinary `Stores`
//!   *containment* edge (`ffi/callback`, `%pair`/`%put`) — which must NOT seed (it
//!   builds the subtree, resolved by external uniqueness in step 2). Filled only
//!   under a real `CallClassification`; the default empty effects treat `chan/send`
//!   as an opaque user fn.
//!
//! ## The consumer — externally-unique Owned subtrees
//!
//! [`compute_owned_subtrees`] is the consumer of the seed set (external uniqueness;
//! docs/impl/region/ownership.md § "Adoption and subtree drop"). It walks the region containment graph
//! outward from each candidate root and reports the subtrees that are **externally
//! unique**: no value inside crosses a frontier (none is a Shared seed) and no region
//! *outside* the subtree references one *inside*. Such a subtree is an Owned candidate
//! — once the emit/runtime forest lands it frees as a unit by subtree drop, its
//! interior reference cycles reclaiming with it.
//!
//! The containment graph is **three edge sources**, because no single existing set
//! carries all of containment:
//!
//! 1. the **stores** in `RegionInfo::cross_region_refs` (`target ⊇ source`) — the
//!    `%pair` car/cdr edges recorded by the intrinsic walk arm (`%pair` lowers as
//!    an inline `Intrinsic` opcode, so its embedding is visible to the walk);
//! 2. **capture edges** (a closure's region ⊇ each value it captures), deliberately
//!    *absent* from `cross_region_refs` — the RC double-count fix records the runtime
//!    auto-incref over the closure env instead of a static `IncrefRegion`
//!    (`capture_records_no_cross_region_edge`, `regions::tests`; the Lambda arm of
//!    `regions::walk`), so the ownership walk re-derives them from the HIR (the
//!    closure's `alloc_region` and its captures' `binding_source_regions`). Without
//!    them a value captured by a *local* closure would be claimed an independent Owned
//!    singleton while the closure env still references it — a double-free at emit;
//! 3. **funnel-store containment** in `RegionInfo::containment_edges` — the
//!    `%array-push`/`%put` containment: the storing ops lower as opaque `Funnel`
//!    native calls (`IntrinsicOp::routes_native_funnel()`) that record NO
//!    `cross_region_refs` edge (the funnel counts the store at runtime; a compile-time
//!    edge would double-count — region/effects.md § `Funnel`). The walk recovers the
//!    containment from the container argument's `RetType`
//!    (`MutableArray`/`MutableStruct`), with no incref. The
//!    same `containment_edges` vector ALSO carries a `Fresh` native's **embed**
//!    containment `result ⊇ arg` (`PrimitiveDef::embeds`, recorded by the walk's `Fresh`
//!    arm — `with-traits`'s `traits` side-field), the compile-time analog of the runtime
//!    alloc-scan; consumed here identically to the funnel-recovered edges
//!    (region/effects.md § `Fresh`).
//!
//! [`compute_owned_subtrees`] is wrapped by [`compute_adopt_edges`], which
//! `analyze_regions_with` calls unconditionally to populate
//! `RegionInfo::owned_adopt_edges`, and the lowerer emits an `AdoptRegion` per edge
//! (`regionemit.rs::emit_increfs_for`). A shape the walk cannot prove externally unique
//! stays Shared, so no adopt edge is emitted for it and its emission is the per-region-RC
//! baseline by construction. The cuts consumed today are the **shared/deep-container**
//! subtree (a Fresh container owning its members, members adopted by their actual
//! parent — flat star or multi-level nesting) and the **capture** subtree
//! (a value captured by a local closure: the member's tight last-use admits it, the
//! lowerer adopts it at the closure-construction site through `capture_adopt_edges`, and
//! `analyze_regions_with` suppresses its decref so the closure's subtree drop is its
//! single reclamation path). The capture emit covers **every** capture kind — a direct
//! local reloaded from its binding slot, an upvalue/transitive capture from the
//! constructing function's environment (region/adopt.md § "The capture adopt") — so no
//! lowerability refusal exists; the lifetime obligation alone bounds admission, and it
//! refuses the cross-activation (upvalue) family by construction until an owner that
//! outlives every capturer exists. The store-keyed path is the **funnel adopt**: a
//! funnel-recovered `containment_edges` edge (site-keyed at its funnel call) is an
//! emittable interior owner-edge wherever the site is a retaining store recording
//! the member, so a funnel-built subtree adopts just like a constructor-built one
//! (region/adopt.md § "The funnel adopt"). The owner-node cuts sit beside
//! these region-rooted modes: the capture-back-edge SCC (`compute_activation_adopts`)
//! and the transferred returned subtree (`compute_transfer_adopts` — a
//! callee/fiber-body-built cyclic subtree handed across the return frontier, owned by
//! the consuming activation's node; its interior owner edges ride these same adopt
//! maps, the funnel-recovered edges included). Still a later cut: an owner that
//! outlives every capturer for the upvalue closure-web family.

mod activation;
mod adopt;
mod capture;
mod inputs;
mod seeds;
mod subtree;
mod transfer;

// The forest emit (`analyze_regions_with`) consumes these four. Each takes the shared
// `OwnershipInputs` (the containment graph + candidate set + capture edges) by reference,
// built ONCE per compile in `apply_ownership` (`ownership_inputs`) rather than rebuilt per
// pass — the inputs derive only from fields unchanged across the ownership passes.
pub(in crate::hir::regions) use activation::compute_activation_adopts;
pub(in crate::hir::regions) use adopt::compute_adopt_edges;
pub(in crate::hir::regions) use inputs::ownership_inputs;
pub(in crate::hir::regions) use subtree::compute_owned_region_groups;
pub(in crate::hir::regions) use transfer::compute_transfer_adopts;
// The Shared-seed set (frontier crossings: return / emit / send — NOT capture) is the
// closure-cycle merge's non-escape gate: a captured-but-not-frontier-crossing closure
// is mergeable, which `lambda_escapes_definition` (which also folds in the capture facet
// — a value captured by an escaping closure, propagated around the SCC's mutual captures)
// would wrongly refuse.
pub(super) use seeds::compute_shared_seeds;

// The remaining stages are exercised directly by the `regions::tests` harness; the
// re-export keeps their `ownership::NAME` path stable for it without dangling in a
// normal build (each is reached internally via its own submodule path).
#[cfg(test)]
pub(super) use {
    adopt::AdoptEdges, capture::capture_containment_edges, subtree::compute_owned_subtrees,
};
