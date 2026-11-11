// audited: 2026-11-09
//! What a scope holds for one name: a binding, the scope set it binds with, and what a lookup reads of it.
//!
//! docs/bindings.md
//! docs/macros.md

use super::Binding;
use crate::syntax::ScopeId;

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
