// audited: 2026-11-25
//! The working sets a free fills, kept on the store between frees so a warm free allocates nothing.
//!
//! docs/impl/region/ownership.md

use super::*;

/// What one wholesale free works in. A free takes these off the store, clears
/// each before it fills it, and puts them back, so their capacity carries over
/// to the next free.
#[derive(Default)]
pub(in crate::value::fiberheap::regionstore) struct FreeSets {
    /// The regions whose count reached zero in this free's cascade, each with
    /// the root of the set whose frontier released it, to tear down alone.
    pub(super) cascade: Vec<(RuntimeRegion, Option<RuntimeRegion>)>,
    /// The walk of the owned subtree: each region, and the owner that listed it.
    pub(super) stack: Vec<(RuntimeRegion, Option<RuntimeRegion>)>,
    /// Every region the walk reached, in walk order, with the owner that listed it.
    pub(super) candidates: Vec<(RuntimeRegion, Option<RuntimeRegion>)>,
    /// The ids of the candidates that still die after the rescue.
    pub(super) dying: rustc_hash::FxHashSet<u32>,
    /// The dying regions, each with the entry taken out of the table.
    pub(super) members: Vec<(RuntimeRegion, RegionEntry)>,
    /// The references into surviving regions the members held, one per count.
    pub(super) frontier: Vec<u32>,
    /// The incoming mirror entries to retire: source, target and count.
    pub(super) retire: Vec<(RuntimeRegion, RuntimeRegion, u32)>,
    /// The debug oracle's content scan of the members.
    #[cfg(debug_assertions)]
    pub(super) scanned: Vec<u32>,
    /// The debug oracle's sorted copy of the recorded frontier.
    #[cfg(debug_assertions)]
    pub(super) recorded: Vec<u32>,
}
