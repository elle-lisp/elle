use super::super::*;
use rustc_hash::FxHashSet;

/// The regions a value escapes its activation/fiber **frontier** through — the
/// Shared-seed set. Such a region cannot be Owned. See the module doc for why this
/// is the frontier crossings (return, and both fiber halves — emit and send), never
/// the containment facets (store, capture) of the full `binding_escapes_activation`.
///
/// The verdict is escape's; this only **projects** it onto regions through the
/// solver's `alloc_region` / `binding_source_regions` maps
/// (`super::super::escape::shared_seed_regions`). The region solver holds no escape
/// logic of its own — there is no parallel escape judgment to read.
///
/// Consumed by `compute_owned_subtrees` (the external-uniqueness walk) and pinned
/// directly by the `shared_seed_*` pins (`region::infer::tests`). The chain is reached in a
/// shipping build through `compute_adopt_edges`, which `analyze_regions_with` calls
/// by the ownership pass.
pub(in crate::hir::region::infer) fn compute_shared_seeds(
    info: &RegionInfo,
    escape: &crate::hir::EscapeInfo,
) -> FxHashSet<Region> {
    super::super::escape::shared_seed_regions(escape, info)
}
