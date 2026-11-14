// audited: 2026-11-14
//! What a scope holds for one name: a binding, the scope set it binds with, and what a lookup reads of it.
//!
//! docs/bindings.md
//! docs/macros.md

use super::{Analyzer, Binding};
use crate::syntax::ScopeId;
use crate::value::SymbolId;

/// A binding with its scope set for hygienic resolution.
#[derive(Debug, Clone)]
pub(super) struct ScopedBinding {
    pub(super) scopes: Vec<ScopeId>,
    pub(super) binding: Binding,
    pub(super) state: EntryState,
}

/// What a scope entry answers to a lookup (`scopes.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EntryState {
    /// The binding a read of its name finds.
    Visible,
    /// A body's first `def` of the name, before the analysis reaches it: a
    /// read inside a lambda finds it, and a direct read in a function body
    /// reads past it (`owned.rs`).
    Forward,
    /// A `Forward` entry whose `def` has been analyzed. The `def` registered
    /// a `Visible` entry of its own.
    Spent,
    /// A later `def` of the name, while its value is analyzed: a read inside a
    /// lambda finds it, and a direct read reads past it. It turns `Visible`
    /// once the value is analyzed.
    Defining,
}

impl ScopedBinding {
    pub(super) fn new(scopes: &[ScopeId], binding: Binding, state: EntryState) -> Self {
        ScopedBinding {
            scopes: scopes.to_vec(),
            binding,
            state,
        }
    }
}

impl<'a> Analyzer<'a> {
    /// Register a body's first `def` of a name before the analysis reaches
    /// it (`EntryState::Forward`).
    pub(super) fn register_forward(&mut self, name: &str, scopes: &[ScopeId], binding: Binding) {
        let entry = ScopedBinding::new(scopes, binding, EntryState::Forward);
        self.push_entry(SymbolId::of(name), entry);
    }

    /// Register a later `def` of a name before its value is analyzed
    /// (`EntryState::Defining`). A direct read is one at this depth.
    pub(super) fn register_defining(&mut self, name: &str, scopes: &[ScopeId], binding: Binding) {
        self.arena.get_mut(binding).prebind_fn_depth = self.fn_depth;
        let entry = ScopedBinding::new(scopes, binding, EntryState::Defining);
        self.push_entry(SymbolId::of(name), entry);
    }

    /// Turn the current scope's `Forward` entry for `binding` into a `Spent`
    /// one: its `def` has been analyzed.
    pub(super) fn spend(&mut self, name: &str, binding: Binding) {
        self.move_entry(name, binding, EntryState::Forward, EntryState::Spent);
    }

    /// Turn the current scope's `Defining` entry for `binding` into a
    /// `Visible` one: its value has been analyzed.
    pub(super) fn reveal(&mut self, name: &str, binding: Binding) {
        self.move_entry(name, binding, EntryState::Defining, EntryState::Visible);
    }

    fn move_entry(&mut self, name: &str, binding: Binding, from: EntryState, to: EntryState) {
        let Some(scope) = self.scopes.last_mut() else {
            return;
        };
        if let Some(candidates) = scope.bindings.get_mut(&SymbolId::of(name)) {
            for entry in candidates.iter_mut() {
                if entry.binding == binding && entry.state == from {
                    entry.state = to;
                }
            }
        }
    }
}
