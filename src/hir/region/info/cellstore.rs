// audited: 2026-10-11
//! The stores into a reassigned binding, and the 1-slot container the solver
//! models one as.
//!
//! docs/impl/region/bindings.md

use super::super::Region;
use crate::hir::expr::HirId;

/// One store into a reassigned binding: the `Assign`/`SetCell` site, and the
/// regions of the value stored THERE. The pairing is what lands a stored
/// value's producer release at the store that took it.
#[derive(Clone)]
pub struct CellStore {
    pub site: HirId,
    pub value_regions: Vec<Region>,
}

/// Every store into one reassigned binding, in walk order.
///
/// A newtype rather than a bare `Vec`, so the two projections a consumer wants —
/// the sites alone, and every stored region — are named once here instead of
/// spelled out at each site, and so recording a store cannot forget to look for
/// the site's existing entry.
#[derive(Default, Clone)]
pub struct CellStores(Vec<CellStore>);

impl CellStores {
    /// Record `value_regions` as stored at `site`, merging into that site's
    /// existing entry. The solver re-walks an inlinable callee's body at the call
    /// site to collect its edges, so one `assign` can be visited more than once;
    /// merging keeps one entry per site whatever the visit count.
    pub fn record(&mut self, site: HirId, value_regions: &[Region]) {
        let at = match self.0.iter().position(|s| s.site == site) {
            Some(at) => at,
            None => {
                self.0.push(CellStore {
                    site,
                    value_regions: Vec::new(),
                });
                self.0.len() - 1
            }
        };
        let entry = &mut self.0[at];
        for &r in value_regions {
            if !entry.value_regions.contains(&r) {
                entry.value_regions.push(r);
            }
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &CellStore> + '_ {
        self.0.iter()
    }

    /// The store sites alone — what drop-on-overwrite is marked at.
    pub fn sites(&self) -> impl Iterator<Item = HirId> + '_ {
        self.0.iter().map(|s| s.site)
    }

    /// Every region stored into the cell, whichever site took it. A region two
    /// stores both take is yielded once per store, since the iterator's consumers
    /// are pins and sets; [`Self::value_region_set`] is the deduplicated form.
    pub fn value_regions(&self) -> impl Iterator<Item = Region> + '_ {
        self.0.iter().flat_map(|s| s.value_regions.iter().copied())
    }

    /// Every region stored into the cell, each named once — the form the gate
    /// reads, its questions being asked of a binding's whole set at once.
    pub fn value_region_set(&self) -> Vec<Region> {
        let mut out: Vec<Region> = Vec::new();
        for r in self.value_regions() {
            if !out.contains(&r) {
                out.push(r);
            }
        }
        out
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// A reassigned mutable that took the 1-slot-container gate with a COUNTED
/// store. The cell holds exactly ONE counted reference to its current content,
/// released through two channels: drop-on-overwrite at each store for the
/// displaced prior, and the content drop at `demise` for the final value. A
/// fn-local cell keeps that drop; a forwarding link hands its final content to
/// the next link, and a module-scope cell leaves it to the file-letrec frame
/// teardown. Every cell carries the store-site pin and the binding-chain
/// hold-back, whichever way its final content goes.
pub struct CellContainer {
    /// The stores into the cell, each with the regions of the value it took.
    pub stores: CellStores,
    /// The node whose exit is the cell's scope demise — the enclosing scope
    /// node, so a loop-carried cell drops once after the loop and a cell bound
    /// inside a loop body drops once per iteration. The content drop lands here
    /// only when [`Self::drops_content`] holds; the placement passes compute it
    /// for every cell, and the uncounted-read and return extensions read it as
    /// the point past which the cell no longer protects a borrow.
    pub demise: HirId,
    /// Whether the cell emits a content drop for its final content at `demise`.
    drops_content: bool,
}

impl CellContainer {
    /// Whether the cell emits a content drop for its final content. A
    /// forwarding link hands that content to the next link instead, and a
    /// module-scope cell leaves it to the file-letrec frame teardown.
    pub fn drops_content(&self) -> bool {
        self.drops_content
    }

    /// A fn-local cell that keeps both channels: drop-on-overwrite for each
    /// displaced prior, and the content drop at `demise` for the final one.
    pub fn new(stores: CellStores, demise: HirId) -> Self {
        Self {
            stores,
            demise,
            drops_content: true,
        }
    }

    /// A cell whose final content is handed to the next link of a forwarding
    /// chain, so the content drop belongs to that link and not to this one.
    pub fn forwarding(stores: CellStores, demise: HirId) -> Self {
        Self {
            drops_content: false,
            ..Self::new(stores, demise)
        }
    }

    /// A module-scope cell that counts a call result's store. Its final content
    /// is the file-letrec frame teardown's, so it emits no content drop; the
    /// container is recorded for the store-site pin and the binding-chain
    /// hold-back alone.
    pub fn module_scope(stores: CellStores, demise: HirId) -> Self {
        Self {
            drops_content: false,
            ..Self::new(stores, demise)
        }
    }
}
