//! The region-merge seeds (`super` = `hir::regions`).
//!
//! Computes `RegionInfo::merged_parent`: the `child → parent` forest of regions
//! the lowerer may collapse onto one physical region. Two seeds populate the same
//! forest and ride the same `merged_root` canonicalization in the lowerer:
//!
//!  - [`builder`] — the builder-idiom seed: a freshly-built child aggregate merged
//!    into the parent `%pair` it is stored into (docs/impl/region/merging.md § Merging).
//!  - [`cycle`] — the `letrec` closure-cycle merge: an SCC of mutually-recursive
//!    closures ∪ their prebound capture cells, collapsed onto one arena and freed by a
//!    single `DecrefRegion` at the cycle's binding scope — or, where the letrec hands a
//!    member out, where that member's own release already sits
//!    (docs/impl/region/letrec.md § The letrec closure-cycle merge).
//!
//! The lowerer consumes both through `static_slot`'s `merged_root` canonicalization:
//! every member of a merge tree resolves to the root's slot, so members allocate into
//! one physical region, interior `DecrefRegion`s are suppressed, and interior store
//! edges' `IncrefRegion`s are dropped. With no merge (`merged_parent` empty)
//! `merged_root` is the identity and the lowerer's behaviour is the unmerged baseline.

mod builder;
mod cycle;

// Re-exported at `crate::hir::regions::merge::*` so every path that previously
// resolved here still does; visibility matches the pre-split `pub(super)`. The
// entry points are consumed by `super::analyze`. `ClosureCycleMerge` is that
// analyzer's result type — reached through `compute_closure_cycle_merges`' return
// value rather than named, so the bare re-export carries no direct user; it is
// retained (with the lint waived) to preserve the `merge::ClosureCycleMerge` path.
pub(super) use builder::compute_merges;
pub(super) use cycle::compute_closure_cycle_merges;
#[allow(unused_imports)]
pub(super) use cycle::ClosureCycleMerge;
