// audited: 2026-09-23
//! Per-value region inference: the types, and the walk that assigns them.
//!
//! Every allocation site gets its own unique region, assigned by a single
//! forward walk (no constraint solver, no widening). Each region's
//! `decref_point` is the program point at which the lowerer frees it.
//!
//! `RegionInfo` is region inference's output: per-allocation region
//! assignments and the set of regions that contain live allocations.
//! The lowerer queries `scope_has_local_allocs(hir_id)` to decide a
//! scope's region handling.
//!
//! The types live in the submodules here; the walk that produces them lives
//! in [`infer`].
//!
//! docs/impl/region/model.md

mod classify;
mod data;
mod id;
pub mod infer;
mod info;
mod order;
mod stats;

// Re-exported at this root so a consumer names `crate::hir::region::<Item>`
// rather than whichever submodule happens to define it, and so the test
// module's `use super::*;` sees them.
pub use classify::{CallClassification, EMIT_PAYLOAD_ARG};
pub use data::{Region, RegionData};
pub use id::{MappedRegion, RuntimeRegion, StaticRegion};
pub use info::{CellContainer, CellStore, CellStores, RegionInfo, RestCollection, TailCalleeFacts};
pub use order::{PinDecref, ProgramOrder};
pub use stats::RegionStats;

#[cfg(test)]
mod tests;
