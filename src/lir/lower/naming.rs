// audited: 2026-09-05
//! How a release names what it frees: the address space a value slot belongs
//! to, and the process-global counter that mints a static region id.
//!
//! docs/impl/region/mechanism.md
//! docs/impl/region/model.md

use super::*;
use std::sync::atomic::{AtomicU32, Ordering};

/// Global region ID counter. IDs 0 (invalid) and 1 are reserved; minting starts at 2.
/// Used by the lowerer for solver-assigned regions and by the compilation
/// pipeline for transient compile-time regions.
static NEXT_STATIC_REGION: AtomicU32 = AtomicU32::new(2);

/// Mint a fresh **static** region id — a compile-time, globally-unique slot
/// number baked into bytecode. A static id is a per-function slot, NOT a live
/// region: each activation remaps it to a freshly-minted `new_runtime_region`
/// via its `activation_region_map`. Never index a static id into the
/// `RegionStore` (see docs/impl/region/model.md).
pub fn new_static_region() -> StaticRegion {
    let id = NEXT_STATIC_REGION.fetch_add(1, Ordering::Relaxed);
    assert!(
        id >= 2,
        "static region id counter wrapped or hit reserved range"
    );
    StaticRegion::new(id).expect("static region id counter is >= 2, hence nonzero")
}

/// Where a value-route release reads the value whose region it means.
///
/// `allocate_slot_routed` mints binding slots from two disjoint address spaces,
/// both indexed by `u16`: an in-lambda captured binding gets an ENV index (the
/// index `LoadCapture`/`StoreCapture` address, backed by the `populate_env`
/// cell), and every other binding gets a STACK index (`LoadLocal`/`StoreLocal`).
/// Nothing about the number says which, so a bare `u16` in `region_to_slot` lets
/// an env index be read back as a stack slot — naming whichever local happens to
/// sit at that index and releasing it under its holder
/// (`tests/elle/region-def-in-lambda-capture.lisp`). Carrying the space with the
/// index makes that unrepresentable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ValueSlot {
    /// A stack-frame local. `LoadLocal { slot }` yields the value itself.
    Local(u16),
    /// An env-cell index. `LoadCapture { index }` UNWRAPS the cell and yields
    /// its content — the value whose region a release means. (The cell's own
    /// region is a separate concern, released through `LoadCaptureRaw` +
    /// `DecrefCellRegion` for a `cell_release_regions` member.)
    Env(u16),
}

impl ValueSlot {
    /// The raw index, for the sites that only need to dedupe or report it.
    pub(crate) fn index(self) -> u16 {
        match self {
            ValueSlot::Local(i) | ValueSlot::Env(i) => i,
        }
    }

    /// The stack slot, or `None` for an env index. Use at sites whose emission
    /// is stack-only (`AdoptRegion`, `FreeRegionGroup`, the branch-arm
    /// compensations' value route): skipping an env-celled region there leaves it
    /// independently reference-counted, which is each of those cuts' documented
    /// always-legal fallback. A `cell_release_regions` member is the one env-indexed
    /// release those compensations do emit, and it reads [`Self::index`] instead —
    /// it names the cell BOX, which `LoadCaptureRaw` reaches by index alone.
    pub(crate) fn local(self) -> Option<u16> {
        match self {
            ValueSlot::Local(i) => Some(i),
            ValueSlot::Env(_) => None,
        }
    }
}
