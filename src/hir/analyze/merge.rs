// audited: 2026-11-12
//! The forms that may skip or repeat a `def`: the merge var each `def` in one assigns, and the binding a read after the form finds.
//!
//! docs/bindings.md
//!
//! A branch, a short-circuit operand after the first, and a loop each open a
//! merge frame in the scope they sit in. A `def` in the frame assigns the
//! frame's merge var for its name, which the frame sets before it runs to the
//! binding visible there, or to nil. When the frame closes, the bindings it
//! registered leave the scope. A new binding of the name takes the merge var's
//! value at the start of whatever runs next, and the name resolves to it.

use super::owned::NameKey;
use super::*;

mod branch;
mod head;
mod join;
mod pending;
mod uses;
use branch::{Branch, Defines};
pub(super) use join::Join;
pub(super) use pending::Pending;
pub(super) use uses::MergeUses;

/// One open form that may skip or repeat a `def`.
#[derive(Default)]
pub(super) struct MergeFrame {
    merges: Vec<Merge>,
    /// The branch the analysis is in.
    branch: Branch,
}

/// A name a `def` in the frame binds.
struct Merge {
    key: NameKey,
    name: String,
    scopes: Vec<ScopeId>,
    /// The var every `def` of the name in the frame assigns.
    var: Binding,
    /// How many scope entries the name had when the frame opened.
    entry_len: usize,
    /// How many it had where the frame's branches start.
    branch_len: usize,
    /// The binding of each `def` of the name in the frame, however deep.
    inputs: Vec<Binding>,
    /// The branches that define the name on every path through them.
    defines: Defines,
}

impl<'a> Analyzer<'a> {
    /// How many entries the current scope holds for `name`.
    fn entry_count(&self, name: &str) -> usize {
        self.scopes
            .last()
            .and_then(|s| s.bindings.get(&SymbolId::of(name)))
            .map_or(0, Vec::len)
    }

    /// Give every merge frame open in the current scope a merge var for `key`,
    /// before a binding of the name enters the scope inside them.
    pub(super) fn track(&mut self, key: &NameKey, name: &str, scopes: &[ScopeId]) {
        let len = self.entry_count(name);
        let frames = self.scopes.last().map_or(0, |s| s.merges.len());
        for i in 0..frames {
            let known = self
                .scopes
                .last()
                .is_some_and(|s| s.merges[i].merges.iter().any(|m| m.key == *key));
            if known {
                continue;
            }
            let var = self.arena.temp(self.symbols.intern(name));
            self.arena.get_mut(var).is_mutated = true;
            if let Some(scope) = self.scopes.last_mut() {
                scope.merges[i].merges.push(Merge {
                    key: key.clone(),
                    name: name.to_string(),
                    scopes: scopes.to_vec(),
                    var,
                    entry_len: len,
                    branch_len: len,
                    inputs: Vec::new(),
                    defines: Defines::default(),
                });
            }
        }
    }

    /// The assigns that record `binding` as the latest `def` of `key` in each
    /// merge frame open in the current scope.
    pub(super) fn assign_merges(
        &mut self,
        key: &NameKey,
        binding: Binding,
        span: Span,
    ) -> Vec<Hir> {
        let Some(scope) = self.scopes.last_mut() else {
            return Vec::new();
        };
        let mut vars = Vec::new();
        for frame in &mut scope.merges {
            for merge in frame.merges.iter_mut().filter(|m| m.key == *key) {
                merge.inputs.push(binding);
                vars.push(merge.var);
            }
        }
        self.defines_in_branch(key);
        vars.into_iter()
            .map(|var| {
                let value = Box::new(Hir::silent(HirKind::Var(binding), span));
                Hir::silent(HirKind::Assign { target: var, value }, span)
            })
            .collect()
    }

    /// Open a merge frame in the current scope.
    pub(super) fn open_merge(&mut self) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.merges.push(MergeFrame::default());
        }
    }

    fn innermost_merges(&self) -> &[Merge] {
        self.scopes
            .last()
            .and_then(|s| s.merges.last())
            .map_or(&[], |f| f.merges.as_slice())
    }

    /// Truncate each name's entries in the current scope to the given count.
    fn cut_entries(&mut self, cuts: &[(String, usize)]) {
        if let Some(scope) = self.scopes.last_mut() {
            for (name, len) in cuts {
                if let Some(entries) = scope.bindings.get_mut(&SymbolId::of(name)) {
                    entries.truncate(*len);
                }
            }
        }
    }

    /// Close the innermost merge frame around `hir`, the form it covers. Each
    /// name a `def` in it bound gets a merged binding, defined from the merge
    /// var where the analysis goes next. Answers `hir`, after the sets of the
    /// merge vars.
    pub(super) fn close_merge(&mut self, hir: Hir) -> Hir {
        let Some(frame) = self.scopes.last_mut().and_then(|s| s.merges.pop()) else {
            return hir;
        };
        if frame.merges.is_empty() {
            return hir;
        }
        let cuts: Vec<(String, usize)> = frame
            .merges
            .iter()
            .map(|m| (m.name.clone(), m.entry_len))
            .collect();
        self.cut_entries(&cuts);
        let depth = self.scopes.len().saturating_sub(1);
        self.discard_pending(depth);
        let span = hir.span;
        let signal = hir.signal;
        let mut exprs = Vec::with_capacity(frame.merges.len() + 1);
        for merge in frame.merges {
            let before = self.merge_input(&merge.name, &merge.scopes);
            let init = match before {
                Some(b) => Hir::silent(HirKind::Var(b), span),
                None => Hir::silent(HirKind::Nil, span),
            };
            exprs.push(Hir::silent(
                HirKind::Define {
                    binding: merge.var,
                    value: Box::new(init),
                },
                span,
            ));
            let poisoned = self.mutable_input(&merge.key, before);
            let merged = self.merged_binding(&merge.name, &merge.scopes, poisoned);
            // The binding before the form stands where a path skips every
            // `def` in it.
            let mut inputs: Vec<Option<Binding>> = merge.inputs.into_iter().map(Some).collect();
            if merge.defines.every_path() {
                self.defines_in_branch(&merge.key);
            } else {
                inputs.push(before);
            }
            self.join_merge(merged, inputs);
            self.defs.uses.start(merge.var, before);
            self.defs.uses.derive(merged, merge.var);
            self.defs
                .pending
                .push(Pending::new(depth, merged, merge.var, span));
        }
        exprs.push(hir);
        Hir::new(HirKind::Begin(exprs), span, signal)
    }

    /// Close the innermost merge frame around the form `analyzed` holds, or
    /// drop the frame when the form did not analyze.
    pub(super) fn end_merge(&mut self, analyzed: Result<Hir, String>) -> Result<Hir, String> {
        match analyzed {
            Ok(hir) => Ok(self.close_merge(hir)),
            Err(e) => {
                if let Some(scope) = self.scopes.last_mut() {
                    scope.merges.pop();
                }
                Err(e)
            }
        }
    }

    /// Refuse a read or an `assign` of `binding` when it is a merge that may
    /// stand for a mutable binding.
    pub(super) fn check_merge_read(
        &self,
        binding: Binding,
        name: &str,
        span: &Span,
    ) -> Result<(), String> {
        if self.defs.uses.poisoned.contains(&binding) {
            return Err(format!(
                "{}: '{}' may be one of several bindings here, and one of them is mutable",
                span, name
            ));
        }
        Ok(())
    }

    /// The binding a read of `name` finds where a frame opens, which its merge
    /// var starts from.
    fn merge_input(&mut self, name: &str, scopes: &[ScopeId]) -> Option<Binding> {
        let skipped = self.defs.skipped_pending;
        let found = MergeUses::quietly(self, |a| a.lookup(name, scopes));
        self.defs.skipped_pending = skipped;
        found
    }

    /// Whether a merge of `key` stands for a mutable binding: the `def` forms
    /// of the name in this scope are mutable, or the binding before the frame
    /// is mutable or such a merge itself.
    fn mutable_input(&self, key: &NameKey, before: Option<Binding>) -> bool {
        let declared = self
            .scopes
            .last()
            .and_then(|s| s.declared.get(key))
            .is_some_and(|&(mutable, _)| mutable);
        declared
            || before.is_some_and(|b| {
                !self.arena.get(b).is_immutable || self.defs.uses.poisoned.contains(&b)
            })
    }

    /// A new binding of `name`, visible from here on, that stands for the
    /// latest `def` of it that ran.
    fn merged_binding(&mut self, name: &str, scopes: &[ScopeId], poisoned: bool) -> Binding {
        let merged = self
            .arena
            .alloc(self.symbols.intern(name), BindingScope::Local);
        self.arena.get_mut(merged).is_immutable = true;
        if poisoned {
            self.defs.uses.poisoned.insert(merged);
        }
        self.register_binding(name, scopes, merged);
        merged
    }
}
