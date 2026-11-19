// audited: 2026-11-19
//! The names a `def` form binds, and the rule that every `def` of a name in one scope agrees on mutability.
//!
//! docs/bindings.md

use super::*;
use crate::syntax::Syntax;

mod site;
pub(super) use site::DefBound;

/// A name as binding identity: its symbol and its sorted scope set.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(super) struct NameKey {
    sym: SymbolId,
    scopes: Vec<ScopeId>,
}

impl NameKey {
    pub(super) fn new(sym: SymbolId, scopes: &[ScopeId]) -> Self {
        let mut scopes = scopes.to_vec();
        scopes.sort_unstable();
        NameKey { sym, scopes }
    }
}

impl<'a> Analyzer<'a> {
    /// The names a `def`'s target binds, with each name's scopes and whether
    /// an `@` marks it mutable. A target that is neither a symbol nor a
    /// pattern binds none, and the analysis of the `def` reports it.
    pub(super) fn def_names(target: &Syntax) -> Vec<(&str, &[ScopeId], bool)> {
        if let Some(raw) = target.as_symbol() {
            let (name, at_mutable) = super::strip_at_prefix(raw);
            return vec![(name, target.scopes.as_slice(), at_mutable)];
        }
        let mut leaves = Vec::new();
        if Self::is_destructure_pattern(target) {
            Self::extract_pattern_leaves(target, &mut leaves);
        }
        leaves
    }

    /// Whether a name a `def` form binds is mutable: a `var`, or an `@` name.
    pub(super) fn def_is_mutable(
        &self,
        form_is_def: bool,
        pattern: bool,
        at_mutable: bool,
    ) -> bool {
        let immutable = form_is_def && (pattern || self.immutable_by_default);
        !(immutable && !at_mutable)
    }

    /// Record that a `def` in the current scope binds `key` with the given
    /// mutability. Every `def` of a name in one scope agrees.
    pub(super) fn declare(
        &mut self,
        key: &NameKey,
        name: &str,
        mutable: bool,
        span: &Span,
    ) -> Result<(), String> {
        let Some(scope) = self.scopes.last_mut() else {
            return Ok(());
        };
        match scope.declared.get(key) {
            Some(&(first, at)) if first != mutable => Err(format!(
                "{}: '{}' is defined both mutable and immutable in one scope (first at {})",
                span, name, at
            )),
            Some(_) => Ok(()),
            None => {
                scope.declared.insert(key.clone(), (mutable, *span));
                Ok(())
            }
        }
    }
}
