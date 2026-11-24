// audited: 2026-11-23
//! A non-tail call to the running closure with immediate arguments: whether a
//! call is admitted, the guards it runs, and the direct call.
//!
//! docs/impl/jit-selfcall.md

/// The byte offset of `Closure::squelch_mask` from the address a closure
/// value's payload holds.
#[allow(dead_code)]
pub(crate) const SQUELCH_IN_CLOSURE_OBJECT: i32 = 0;

/// The byte offset of the running fiber's `call_depth` from the start of a
/// `VM`.
#[allow(dead_code)]
pub(crate) const CALL_DEPTH_IN_VM: i32 = 0;

/// The byte offset of `RuntimeConfig::max_depth` from the start of a `VM`.
#[allow(dead_code)]
pub(crate) const MAX_DEPTH_IN_VM: i32 = 0;
