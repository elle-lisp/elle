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
    /// Where the region's release sits by the structural last-use rule alone —
    /// i.e. before the branch-arm release window re-anchored it onto a branch
    /// (`regions::analyze::decref`, docs/impl/region/mechanism.md § "A release
    /// inside one arm is not a release on the other arms").
    ///
    /// The two differ only for a region that window moved, and the distinction is
    /// load-bearing. The anchor is a **placement** fact — where the one release is
    /// emitted so that every arm reaches it — never a claim that the value is
    /// still live there. The ownership and merge cuts admit a subtree when the
    /// root's drop **post-dominates** a member's last use, which is a *lifetime*
    /// question, so they read this field while the lowerer emits at
    /// `decref_point`. Reading the moved anchor there admits cuts the region's
    /// real lifetime does not support, and the subtree drop then frees a member
    /// under a live borrow.
    pub lifetime_point: HirId,
}

impl RegionData {
    /// A region whose release sits at `at` by the structural last-use rule —
    /// both the emitted point and the lifetime it stands for.
    pub fn at(at: HirId) -> Self {
        RegionData {
            decref_point: at,
            lifetime_point: at,
        }
    }

    /// Extend the release to `at`: a genuine last-use fact, so it moves the
    /// lifetime with the emitted point. The one caller that must NOT use this is
    /// the branch-arm window, which moves only where the release is emitted.
    pub fn extend_to(&mut self, at: HirId) {
        self.decref_point = at;
        self.lifetime_point = at;
    }
}
