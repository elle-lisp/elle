// audited: 2026-11-20
//! The written set: the nodes the analyzer builds for the symbols the source spells.
//!
//! src/hir/analyze/AGENTS.md

use super::*;
use crate::hir::HirId;

impl<'a> Analyzer<'a> {
    /// Keep, from here on, the id of each node built for a symbol the source
    /// spells.
    pub fn record_written(&mut self) {
        self.written.get_or_insert_with(HashSet::new);
    }

    /// Take the ids `record_written` asked for; empty when it was never called.
    pub fn take_written(&mut self) -> HashSet<HirId> {
        self.written.take().unwrap_or_default()
    }

    /// Keep `node` as written when `scopes`, the scopes of the symbol it was
    /// built for, hold no intro scope.
    pub(super) fn note_written(&mut self, node: &Hir, scopes: &[ScopeId]) {
        if let Some(written) = self.written.as_mut() {
            if !scopes.iter().any(|s| s.is_intro()) {
                written.insert(node.id);
            }
        }
    }
}
