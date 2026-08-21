//! The producer summary: one summarized returned subtree — its interior owner
//! edges by emit site and kind, plus the subtree's root region.

use super::*;

/// One producer summary: interior owner edges by emit site and kind, plus the
/// returned subtree's root.
pub(super) struct Summary {
    pub root: Region,
    /// `(emit site, member, owner)` — store/funnel-site edges.
    pub store_edges: Vec<(HirId, Region, Region)>,
    /// `(closure-construction site, member, owner)` — capture edges.
    pub capture_edges: Vec<(HirId, Region, Region)>,
}
