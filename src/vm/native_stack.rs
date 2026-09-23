// audited: 2026-09-22
//! How much of the running thread's native stack is left, for the calls that
//! still nest on it: re-entry from Rust, and compiled code.
//!
//! docs/impl/vm.md
#![allow(dead_code)]

/// Below this much native stack, a call does not enter compiled code.
pub(crate) const COMPILED_CALL_RESERVE: usize = 512 * 1024;

/// Below this much native stack, a re-entry into the interpreter halts.
pub(crate) const REENTRY_RESERVE: usize = 256 * 1024;

/// The bytes left between the current stack position and the bottom of the
/// thread's stack, or `None` where the platform does not report its bounds.
pub(crate) fn remaining() -> Option<usize> {
    None
}

/// Whether less than `reserve` bytes of native stack remain.
pub(crate) fn below(_reserve: usize) -> bool {
    false
}

#[cfg(test)]
mod tests;
