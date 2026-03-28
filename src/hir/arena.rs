//! Arena-backed binding storage for the compilation pipeline.
//!
//! `BindingArena` owns all `BindingInner` values for a compilation unit.
//! It is created by the pipeline entry point, borrowed mutably by the
//! `Analyzer`, and borrowed immutably by the `Lowerer`.
//!
//! `BindingInner` and `BindingScope` are the same types that previously lived
//! in `value/heap.rs`. They are compile-time-only data and do not belong in
//! the runtime value system.

use super::binding::Binding;
use crate::value::SymbolId;

/// Where a binding lives at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingScope {
    /// Lambda parameter
    Parameter,
    /// Local variable (let-bound, define inside function)
    Local,
}

/// Internal binding metadata.
///
/// Field names and semantics are identical to the previous `BindingInner`
/// in `value/heap.rs`.
#[derive(Debug)]
pub struct BindingInner {
    /// Original symbol name (for error messages and global lookup)
    pub name: SymbolId,
    /// Where this binding lives
    pub scope: BindingScope,
    /// Whether this binding has been mutated via assign
    pub is_mutated: bool,
    /// Whether this binding is captured by a nested closure
    pub is_captured: bool,
    /// Whether this binding is immutable (def)
    pub is_immutable: bool,
    /// Whether this binding was pre-created before its initializer runs
    /// (begin pass 1, letrec pass 1). Pre-bound immutable locals still
    /// need cells because they may be captured before initialization
    /// (self-recursion, forward references).
    pub is_prebound: bool,
}

impl BindingInner {
    /// A binding needs a cell if captured (for locals) or mutated (for params).
    ///
    /// Immutable locals skip cell wrapping — they are captured by value.
    /// Exception: pre-bound immutable locals still need cells because they
    /// may be captured before their initializer runs (self-recursion,
    /// forward references).
    pub fn needs_lbox(&self) -> bool {
        match self.scope {
            BindingScope::Local => self.is_captured && (!self.is_immutable || self.is_prebound),
            BindingScope::Parameter => self.is_mutated,
        }
    }
}

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
        let index = self.bindings.len() as u32;
        self.bindings.push(BindingInner {
            name,
            scope,
            is_mutated: false,
            is_captured: false,
            is_immutable: false,
            is_prebound: false,
        });
        Binding(index)
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
