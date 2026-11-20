// audited: 2026-11-19
//! What a scope holds for one name: a binding, and the scope set it binds with.
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
}

impl ScopedBinding {
    pub(super) fn new(scopes: &[ScopeId], binding: Binding) -> Self {
        ScopedBinding {
            scopes: scopes.to_vec(),
            binding,
        }
    }
}
