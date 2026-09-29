// audited: 2026-09-29
//! Joining a region: allocating into a region that already exists, under one
//! reference the joining site gives back where a fresh mint's would be released.
//!
//! docs/impl/region/colocation.md

use super::*;

impl RegionStore {
    /// Take one reference on `region` for a site that allocates into it instead
    /// of minting its own. `None` refuses the join, and the site mints fresh.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "no caller until the value mints join")
    )]
    pub(crate) fn join(&mut self, region: RuntimeRegion) -> Option<RuntimeRegion> {
        Some(region)
    }
}
