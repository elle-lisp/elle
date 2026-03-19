//! Binding handle for the HIR phase.
//!
//! A `Binding` is a `u32` index into a `BindingArena`. It is 4 bytes, Copy,
//! and has no heap allocation. Identity is integer equality.
//!
//! Binding metadata is stored in `BindingArena` (in `arena.rs`). All reads
//! and mutations go through the arena: `arena.get(b).field` to read,
//! `arena.get_mut(b).field = value` to mutate.

use std::fmt;

/// A compile-time binding handle. Index into a `BindingArena`.
/// 4 bytes, Copy, no heap allocation.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Binding(pub(crate) u32);

impl fmt::Debug for Binding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Binding({})", self.0)
    }
}

/// Information about a captured variable in a closure
#[derive(Debug, Clone)]
pub struct CaptureInfo {
    /// The binding being captured
    pub binding: Binding,
    /// How to access this capture from the parent scope
    pub kind: CaptureKind,
}

/// How a capture is accessed from the enclosing scope
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureKind {
    /// Capture from parent's local slot (resolved by lowerer via binding_to_slot)
    Local,
    /// Capture from parent's capture (transitive capture)
    Capture { index: u16 },
}
