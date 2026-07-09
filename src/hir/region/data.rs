//! The small value types region inference emits alongside the big
//! `RegionInfo`: the [`Region`] id itself, an outlives constraint, and the
//! per-region metadata carried in `RegionInfo::region_data`.

use crate::hir::expr::HirId;

/// A region identifier assigned by the solver.
///
/// Every allocation site gets a unique Region from region inference.
/// Region IDs start at 1. Region(0) is invalid — it means "no region
/// assigned" and must panic if encountered in an allocation path.
///
/// There are no special-cased region constants. The outermost region
/// of a compilation unit is just the first region inference creates.
/// All regions are treated uniformly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Region(pub u32);

/// An outlives constraint: `shorter` must be widened to at least
/// `longer` in the region tree.
#[derive(Debug)]
pub struct OutlivesConstraint {
    /// Region variable that must live at least as long
    pub longer: u32,
    /// Region variable that may need widening
    pub shorter: u32,
    /// HIR node that generated this constraint (for diagnostics)
    pub source: HirId,
}

/// Per-region metadata produced by region inference.
///
/// `decref_point` is the HirId at which the lowerer emits the region's
/// compiler-owned `DecrefRegion`. Every region has exactly one
/// `decref_point` — there is no `Option<HirId>`. If the region's value
/// has no use anywhere, `decref_point` equals the allocation HirId
/// itself (decref fires immediately after the alloc).
#[derive(Debug, Clone)]
pub struct RegionData {
    pub decref_point: HirId,
}
