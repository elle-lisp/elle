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
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct Binding(pub(crate) u32);

impl fmt::Debug for Binding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Binding({})", self.0)
    }
}

impl Binding {
    /// Opaque per-binding identity for the symbol index (`crate::symbols`).
    ///
    /// The index keeps `hir` at arm's length (it is pipeline-agnostic), so the
    /// arena index is wrapped in a `DefId` rather than exposing `Binding`.
    /// Valid as an index only for the arena that created this binding; the
    /// `DefId` it yields is used purely for identity once extraction is done.
    pub fn def_id(self) -> crate::symbols::DefId {
        crate::symbols::DefId::new(self.0)
    }
}

/// Information about a captured variable in a closure
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CaptureInfo {
    /// The binding being captured
    pub binding: Binding,
    /// How to access this capture from the parent scope
    pub kind: CaptureKind,
}

/// How a capture is accessed from the enclosing scope
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CaptureKind {
    /// Capture from parent's local slot (resolved by lowerer via binding_to_slot)
    Local,
    /// Capture from parent's capture (transitive capture)
    Capture { index: u16 },
    /// A self-reference: the closure captures its **own** enclosing `letrec`/`def`
    /// binding (`binding`) — the same-binding self-edge in the enclosing SCC. The
    /// self-edge does **not** mark the binding captured (`hir/arena.rs::mark_captured`
    /// is skipped for it), so a binding captured only by self-references is cell-free
    /// (`needs_capture() == false`) and its self-reference resolves to the
    /// currently-executing closure (`LoadSelf` in value position, a self-call
    /// re-dispatch in call position — `lir/lower/expr.rs`), never a cell load. The
    /// lowerer reads this classified fact directly (via `current_self_binding`) instead
    /// of re-deriving the self-edge from a `current_function_binding` heuristic. Carries
    /// **no** escape authority — the self-edge is inert in the escape fixpoint
    /// (docs/impl/escape.md). Where the binding also keeps a cell — because a *sibling*
    /// closure captures it (mutual recursion / a forward reference) — that cell is
    /// reached through the binding's own slot as a `Local`/`Capture`, never this
    /// self-slot, so the self-edge stays cell-free even then.
    Recursive { binding: Binding },
}
