// audited: 2026-11-24
//! The [`Region`] id region inference assigns, and the per-region release data in `RegionInfo::region_data`.
//!
//! docs/impl/region/window.md

use crate::hir::expr::HirId;

/// A region identifier assigned by the solver.
///
/// Every allocation site gets a unique Region from region inference.
/// Region IDs start at 1. Region(0) is the sentinel parent of the outermost
/// region, and no allocation is ever assigned it.
///
/// There are no special-cased region constants. The outermost region
/// of a compilation unit is just the first region inference creates.
/// All regions are treated uniformly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Region(pub u32);

/// Per-region metadata produced by region inference.
///
/// `decref_point` is the HirId at which the lowerer emits the region's
/// compiler-owned `DecrefRegion`. Every region has a `decref_point`, so the
/// field is no `Option<HirId>`. If the region's value has no use anywhere,
/// `decref_point` equals the allocation HirId itself, and the release runs
/// right after the allocation.
#[derive(Debug, Clone)]
pub struct RegionData {
    pub decref_point: HirId,
    /// Where the region's release sits by the structural last-use rule alone —
    /// i.e. before the branch-arm release window re-anchored it onto a branch
    /// (`region::infer::analyze::decref`).
    ///
    /// The two differ only for a region that window moved, and reading the wrong
    /// one frees a live member. The anchor is a **placement** fact — where the
    /// release is emitted so that every arm reaches it — never a claim that the
    /// value is still live there. The ownership and merge cuts admit a subtree when the
    /// root's drop **post-dominates** a member's last use, which is a *lifetime*
    /// question, so they read this field while the lowerer emits at
    /// `decref_point`. Reading the moved anchor there admits cuts the region's
    /// real lifetime does not support, and the subtree drop then frees a member
    /// under a live borrow.
    pub lifetime_point: HirId,
    /// Where a region a loop makes and a `break` carries out is released a
    /// second time, past the block. Such a region's `decref_point` is its pass
    /// release, inside the loop (docs/impl/region/breakloop.md).
    pub exit_point: Option<HirId>,
    /// While the decref passes run, the post-order interval of the loop body
    /// whose pins move `decref_point`. A pin outside it moves `exit_point`.
    /// Only a region with a second release has one, and the passes clear it.
    pub pass_scope: Option<(u32, u32)>,
}

impl RegionData {
    /// A region whose release sits at `at` by the structural last-use rule —
    /// both the emitted point and the lifetime it stands for.
    pub fn at(at: HirId) -> Self {
        RegionData {
            decref_point: at,
            lifetime_point: at,
            exit_point: None,
            pass_scope: None,
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
