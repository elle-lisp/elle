// audited: 2026-09-29
//! The pending join a `JoinRegion` leaves on the fiber, and the next value mint
//! that consumes it.
//!
//! docs/impl/region/colocation.md

use super::*;

impl VM {
    /// Record that the next value mint for `slot` joins the region `partner`
    /// lives in. The record holds no reference, so a join nothing consumes costs
    /// nothing.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "no caller until JoinRegion executes")
    )]
    pub(crate) fn set_pending_join(&mut self, _slot: StaticRegion, _partner: Value) {}
}

#[cfg(test)]
mod tests;
