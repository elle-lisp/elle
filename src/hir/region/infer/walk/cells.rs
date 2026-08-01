//! Capture-cell containment edges: the `cell ⊇ content` fact the walk records
//! for every compiled `MakeCaptureCell`. Split out of the `walk` dispatcher so
//! the reason a cell's uncounted store still feeds ownership inference lives
//! next to nothing else.

use super::*;

impl RegionInference {
    /// Record `cell ⊇ content` containment edges for every compiled capture cell this
    /// scope minted (`begin_cell_regions[scope_id]`), keyed at the scope's HirId — the
    /// cell's structural mint site. A `MakeCaptureCell` holds its stored value by an
    /// **uncounted** compiled store (`StoreCaptureCell` at mint, the rebind funnel on
    /// re-store), so the containment records **no** `cross_region_refs` edge and is
    /// invisible to the external-uniqueness scan otherwise. This re-supplies it into
    /// `containment_edges` (feeding ONLY the ownership inference, never an `IncrefRegion`
    /// — the runtime alloc-scan over the cell already counts the store), so external
    /// uniqueness sees the CELL, not the content, as the container a capturing closure
    /// holds. Only compiled cells get the edge; the `populate_env` env-cell route mints
    /// no `begin_cell_regions` entry and stays a borrow. Emit is idempotent per structural
    /// walk — the callers gate it on the same condition that minted the cells.
    pub(super) fn record_cell_content_edges(&mut self, scope_id: HirId) {
        // Collect first: reading `begin_cell_regions` and `binding_regions` borrows
        // `self` immutably, while the push borrows it mutably.
        let mut edges: Vec<(Region, Region)> = Vec::new();
        if let Some(cells) = self.begin_cell_regions.get(&scope_id) {
            for &(b, cell_r) in cells {
                if let Some(contents) = self.binding_regions.get(&b) {
                    for &content_r in contents {
                        if content_r != cell_r {
                            edges.push((content_r, cell_r));
                        }
                    }
                }
            }
        }
        for (content_r, cell_r) in edges {
            self.containment_edges.push((scope_id, content_r, cell_r));
        }
    }
}
