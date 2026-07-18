//! Region types for per-value region inference.
//!
//! Every allocation site gets its own unique region, assigned by a single
//! forward walk (no constraint solver, no widening). Each region's
//! `decref_point` is the program point at which the lowerer frees it.
//!
//! `RegionInfo` is region inference's output: per-allocation region
//! assignments and the set of regions that contain live allocations.
//! The lowerer queries `scope_has_local_allocs(hir_id)` to decide a
//! scope's region handling.

mod classify;
mod data;
mod id;
mod info;
mod stats;

// Re-export at the crate::hir::region root so every path that resolved as
// `crate::hir::region::<Item>` before the split still resolves, and so the
// test module's `use super::*;` keeps seeing these names.
pub use classify::CallClassification;
pub use data::{OutlivesConstraint, Region, RegionData};
pub use id::{MappedRegion, RuntimeRegion, StaticRegion};
pub use info::RegionInfo;
pub use stats::RegionStats;

#[cfg(test)]
mod tests;
