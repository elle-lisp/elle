//! Data structure and cell helpers for JIT-compiled code
//!
//! This root holds the shared alloc-region decoder and re-exports the three
//! concern submodules so every `crate::jit::data::<fn>` path (and the
//! `dispatch::*` re-export that the vtable symbol table and tests rely on) still
//! resolves: `build` (constructors + type predicates), `destructure`
//! (pattern-binding accessors), and `cell` (capture-cell/box operations).

use crate::hir::region::RuntimeRegion;

// In scope for the `tests` submodule, which reaches `Value`/`JitValue` through
// `use super::*` (as it did when the helpers lived in this root). `cfg(test)`
// because that child is the only consumer left in this root.
#[cfg(test)]
use crate::jit::value::JitValue;
#[cfg(test)]
use crate::value::Value;

mod build;
mod cell;
mod destructure;

// Flat re-export: these helpers are addressed as `data::<fn>` (and via
// `dispatch::*`), so keep them all reachable from this root unchanged.
pub use build::*;
pub use cell::*;
pub use destructure::*;

/// Decode a raw JIT alloc-region id (resolved by `elle_jit_resolve_alloc_region`)
/// into a `RuntimeRegion`. Always a mortal region (≥ 2) by the emitter invariant.
///
/// Stays private: a private parent item is visible to the `build` and `cell`
/// child submodules that call it via `super::region_of_raw`.
#[inline]
fn region_of_raw(region: u32) -> RuntimeRegion {
    RuntimeRegion::new(region).expect("JIT alloc region id is a live mortal region")
}

#[cfg(test)]
mod tests;
