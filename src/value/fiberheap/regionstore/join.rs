// audited: 2026-09-29
//! Joining a region: allocating into one that exists, under a reference given back where a fresh mint's would be.
//!
//! docs/impl/region/colocation.md

use super::*;

impl RegionStore {
    /// Take one reference on `region` for a site that allocates into it instead
    /// of minting its own. `None` refuses the join, and the site mints fresh.
    ///
    /// Only a live `Counted` region admits a join. An `Owned` region has no
    /// count, so a reference on it holds nothing, and an id with no entry names
    /// no region at all. A joined region is marked, so no adopt later takes the
    /// count every joining site shares.
    pub(crate) fn join(&mut self, region: RuntimeRegion) -> Option<RuntimeRegion> {
        let entry = self.regions.get_mut(region.get() as usize)?.as_mut()?;
        if !matches!(entry.reclaim, Reclaim::Counted(_)) {
            return None;
        }
        entry.joined = true;
        self.incref(region);
        Some(region)
    }

    /// Join the region a mint handed out, materializing it first if nothing has
    /// allocated into it yet. The materialized entry's birth reference is the
    /// minting site's own, so the join still takes a reference of its own.
    ///
    /// The generation check refuses a mint whose id lived and died since. The id
    /// may be free, or it may name another live region by now.
    pub(crate) fn join_minted(&mut self, mint: RegionMint) -> Option<RuntimeRegion> {
        let id = mint.region();
        if self.generation_raw(id.get()) != mint.gen {
            return None;
        }
        self.ensure(id);
        self.join(id)
    }
}
