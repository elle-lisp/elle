// audited: 2026-11-21
//! The runtime helpers compiled code calls to build, read and test heap data, re-exported flat for the vtable.
//!
//! src/jit/overview.md
//!
//! Four submodules hold the helpers: `build` (constructors and type
//! predicates), `closure` (the `MakeClosure` helper), `destructure`
//! (pattern-binding accessors), and `cell` (capture cells and the rest list).
//! This root re-exports them, so the vtable's symbol table and the tests name
//! each one as `dispatch::<fn>`, and it holds the region decoder they share.

use crate::hir::region::RuntimeRegion;

// The `tests` submodule reaches `Value` and `JitValue` through `use super::*`.
#[cfg(test)]
use crate::jit::value::JitValue;
#[cfg(test)]
use crate::value::Value;

mod build;
mod cell;
mod closure;
mod destructure;

pub use build::*;
pub use cell::*;
pub use closure::*;
pub use destructure::*;

/// Decode a raw JIT alloc-region id (resolved by `elle_jit_resolve_alloc_region`)
/// into a `RuntimeRegion`. Always a mortal region (≥ 2) by the emitter invariant.
#[inline]
fn region_of_raw(region: u32) -> RuntimeRegion {
    RuntimeRegion::new(region).expect("JIT alloc region id is a live mortal region")
}

#[cfg(test)]
mod tests;
