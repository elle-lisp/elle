// audited: 2026-11-09
//! The binding each pass of a loop starts from for a name its body may define.
//!
//! docs/bindings.md

use super::super::owned::{reach, NameKey, Reached};
use super::super::*;
use crate::syntax::Syntax;

impl<'a> Analyzer<'a> {
    /// The `Define` of each binding a loop's head gives a name its body may
    /// define: the latest `def` of it that ran, this pass or an earlier one.
    /// Opens the loop's merge frame.
    pub(in crate::hir::analyze) fn open_loop(&mut self, forms: &[Syntax]) -> Vec<Hir> {
        self.open_merge();
        let mut reached = Vec::new();
        for form in forms {
            reach(form, false, &mut reached);
        }
        let mut heads = Vec::new();
        let mut seen = HashSet::new();
        for Reached { items, .. } in reached {
            let form_is_def = items[0].as_symbol() == Some("def");
            let pattern = Self::is_destructure_pattern(&items[1]);
            for (name, scopes, at_mutable) in Self::def_names(&items[1]) {
                let key = NameKey::new(self.symbols.intern(name), scopes);
                if !seen.insert(key.clone()) {
                    continue;
                }
                self.track(&key, name, scopes);
                let before = self.merge_input(name, scopes);
                let poisoned = self.def_is_mutable(form_is_def, pattern, at_mutable)
                    || self.mutable_input(&key, before);
                let var = self
                    .innermost_merges()
                    .iter()
                    .find(|m| m.key == key)
                    .map(|m| m.var);
                let Some(var) = var else { continue };
                let span = items[1].span;
                let head = self.merged_binding(name, scopes, poisoned);
                self.defs.uses.start(var, before);
                self.defs.uses.derive(head, var);
                let value = Box::new(Hir::silent(HirKind::Var(var), span));
                heads.push(Hir::silent(
                    HirKind::Define {
                        binding: head,
                        value,
                    },
                    span,
                ));
            }
        }
        heads
    }
}
