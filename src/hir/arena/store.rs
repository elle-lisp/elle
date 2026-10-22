// audited: 2026-10-21
//! The arena that owns a compilation unit's bindings, indexed by `Binding`.
//!
//! docs/impl/hir.md

use super::{BindingInner, BindingScope};
use crate::hir::binding::Binding;
use crate::value::SymbolId;

/// Arena for compile-time bindings.
///
/// Bindings are allocated during analysis (`&mut self`) and read during
/// lowering (`&self`). The arena is dropped at the end of the compilation
/// unit — no leaks.
///
/// A `Binding(u32)` index is only valid for the arena that created it.
#[derive(Debug)]
pub struct BindingArena {
    bindings: Vec<BindingInner>,
}

impl BindingArena {
    /// Create an empty arena.
    pub fn new() -> Self {
        Self {
            bindings: Vec::new(),
        }
    }

    /// Allocate a new binding. Analysis phase only.
    pub fn alloc(&mut self, name: SymbolId, scope: BindingScope) -> Binding {
        self.alloc_from(BindingInner::new(name, scope))
    }

    /// Allocate a binding from ready-made metadata. The graft of an
    /// [`HirFragment`](crate::hir::fragment::HirFragment) enters here: the
    /// fragment holds a whole `BindingInner` per binding its body introduces,
    /// so re-hosting reproduces the binding rather than describing it again.
    pub fn alloc_from(&mut self, inner: BindingInner) -> Binding {
        let index = self.bindings.len() as u32;
        self.bindings.push(inner);
        Binding(index)
    }

    /// Allocate a synthetic binding with no source-level identity.
    /// Used by compiler passes that need temporaries (for example, phi-insertion
    /// condition bindings). The name is set to `SymbolId::SYNTHETIC`.
    pub fn gensym(&mut self) -> Binding {
        let b = self.alloc(SymbolId::SYNTHETIC, BindingScope::Local);
        self.get_mut(b).is_synthetic = true;
        b
    }

    /// Read-only access. Available in both analysis and lowering phases.
    pub fn get(&self, binding: Binding) -> &BindingInner {
        &self.bindings[binding.0 as usize]
    }

    /// Mutable access. Analysis phase only (requires `&mut self`).
    pub fn get_mut(&mut self, binding: Binding) -> &mut BindingInner {
        &mut self.bindings[binding.0 as usize]
    }

    /// Number of bindings in the arena.
    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    /// Returns true if the arena contains no bindings.
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }
}

impl Default for BindingArena {
    fn default() -> Self {
        Self::new()
    }
}
