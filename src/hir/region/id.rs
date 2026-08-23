//! The two disjoint region id-spaces: the compile-time static slot baked into
//! bytecode (`StaticRegion`) and the runtime physical region minted per
//! allocation execution (`RuntimeRegion`). They never meet as the same type —
//! docs/impl/region/model.md § "Two id-spaces".

use std::num::NonZeroU32;

/// A compile-time region *slot*: the per-function region id the solver bakes
/// into bytecode (minted by `new_static_region`). A static slot is **not** a
/// live region — the VM resolves it to a [`RuntimeRegion`] per activation
/// through the activation_region_map; it is never indexed into `RegionStore`
/// (docs/impl/region/model.md § "Two id-spaces").
///
/// `NonZeroU32` by construction: a real slot is always ≥ 1, so there is no
/// "slot 0". "Region not applicable to this instruction" is encoded *structurally*
/// — the instruction variant simply has no region field — never as a sentinel
/// 0 nor an `Option` bolted uniformly onto every instruction. Once an LIR
/// instruction carries a `StaticRegion`, that region is mandatory; you cannot
/// build the instruction without one.
///
/// `StaticRegion` lives in the typed LIR layer only. Serialized into bytecode it
/// becomes a raw `u32`, and the VM decodes that `u32` slot and resolves it to a
/// `RuntimeRegion` — the two never meet as the same type.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
pub struct StaticRegion(NonZeroU32);

impl StaticRegion {
    /// Wrap a raw slot id, returning `None` for 0 (there is no slot 0).
    #[inline]
    pub const fn new(id: u32) -> Option<StaticRegion> {
        match NonZeroU32::new(id) {
            Some(n) => Some(StaticRegion(n)),
            None => None,
        }
    }

    /// The raw slot id (always ≥ 1) for bytecode encoding.
    #[inline]
    pub const fn get(self) -> u32 {
        self.0.get()
    }
}

impl std::fmt::Display for StaticRegion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.get())
    }
}

/// A runtime *physical* region id: a real, pages-owning region in a per-heap
/// `RegionStore`, minted per allocation *execution* (and recycled on free).
///
/// `NonZeroU32` by construction, because docs/impl/region/rules.md Rule 1 says **there is
/// no region 0** — an unassigned region is `Option::None`, never
/// `RuntimeRegion(0)`. This is the *only* type the runtime RC paths and
/// `RegionStore` are indexed by; a compile-time static slot (`StaticRegion`)
/// cannot be passed where a `RuntimeRegion` is expected (a compile error),
/// which is the type-level form of the "never index a static id into
/// RegionStore" invariant.
///
/// Region ids are minted fresh per allocation *execution* (not per static
/// site), so a long-running program churns through many ids even though the
/// *live* count stays bounded by recycling. `u32` gives ample headroom.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
pub struct RuntimeRegion(NonZeroU32);

impl RuntimeRegion {
    /// Wrap a raw physical id, returning `None` for id < 2.
    ///
    /// **Freeable by construction.** Ids 0 and 1 are reserved and not
    /// representable here, so every `RuntimeRegion` is a real, RC-reclaimable
    /// region — "this region can be decref'd / freed" is the type's invariant.
    #[inline]
    pub const fn new(id: u32) -> Option<RuntimeRegion> {
        if id < 2 {
            return None;
        }
        match NonZeroU32::new(id) {
            Some(n) => Some(RuntimeRegion(n)),
            None => None,
        }
    }

    /// The raw physical id (always ≥ 2).
    #[inline]
    pub const fn get(self) -> u32 {
        self.0.get()
    }
}

impl std::fmt::Display for RuntimeRegion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.get())
    }
}

/// An activation-region-map entry: the physical [`RuntimeRegion`] a static slot
/// currently resolves to in one activation, tagged with the region's generation
/// **at the moment the slot was established** (docs/impl/region/generations.md
/// § "Region generations").
///
/// The generation is what tells a live mapping from a dead leftover. A slot's
/// entry is inserted at alloc and cleared only by the matching slot-based
/// `DecrefRegion`; a region freed any other way (value-based `DecrefValueRegion`/
/// `DecrefCellRegion`, a cross-region cascade, a subtree drop) leaves the entry
/// behind, and the physical id it names is recycled to an unrelated region. Such
/// a leftover is harmless while the activation runs (a re-alloc overwrites it),
/// but a park snapshots the whole map (`record_region_borrows`): recording the
/// *current* generation of a recycled id would forge a live borrow of a region
/// the activation never owned, and the resume-time uncounted-borrow check would
/// then panic spuriously when that unrelated incarnation is freed. Carrying
/// `gen` lets the snapshot record the generation the slot was established at, so
/// an entry whose region has since moved on (`gen != current`) is recognized as
/// a dead leftover and skipped, while a genuine borrow freed *while parked* still
/// trips the check (docs/impl/region/generations.md § "Uncounted-borrow check").
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MappedRegion {
    /// The physical region the slot resolves to.
    pub region: RuntimeRegion,
    /// The region's generation when this mapping was established.
    pub gen: u32,
}

impl MappedRegion {
    #[inline]
    pub const fn new(region: RuntimeRegion, gen: u32) -> Self {
        MappedRegion { region, gen }
    }
}
