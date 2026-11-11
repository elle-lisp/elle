// audited: 2026-11-09
//! The merged bindings a closed merge frame leaves waiting, and where the analysis defines them.
//!
//! docs/bindings.md

use super::super::*;

/// A merged binding, which the next expression analyzed in its scope defines
/// first from the merge var.
pub(in crate::hir::analyze) struct Pending {
    depth: usize,
    merged: Binding,
    var: Binding,
    span: Span,
}

impl Pending {
    pub(in crate::hir::analyze) fn new(
        depth: usize,
        merged: Binding,
        var: Binding,
        span: Span,
    ) -> Self {
        Pending {
            depth,
            merged,
            var,
            span,
        }
    }

    /// The merged binding's value: its merge var.
    fn value(&self) -> Hir {
        Hir::silent(HirKind::Var(self.var), self.span)
    }

    fn define(self) -> Hir {
        let value = Box::new(self.value());
        Hir::silent(
            HirKind::Define {
                binding: self.merged,
                value,
            },
            self.span,
        )
    }
}

impl<'a> Analyzer<'a> {
    /// `hir` followed by the merged bindings waiting to be defined, as an
    /// expression that answers `hir`'s value. For a form whose next analyzed
    /// expression may not run after it: a condition, a scrutinee, a callee.
    pub(in crate::hir::analyze) fn flush_merges(&mut self, hir: Hir) -> Hir {
        if self.defs.pending.is_empty() {
            return hir;
        }
        let span = hir.span;
        let signal = hir.signal;
        let value = self
            .arena
            .alloc(self.symbols.intern("__merged_value"), BindingScope::Local);
        self.arena.get_mut(value).is_synthetic = true;
        let mut exprs = vec![Hir::new(
            HirKind::Define {
                binding: value,
                value: Box::new(hir),
            },
            span,
            signal,
        )];
        exprs.extend(self.defs.pending.drain(..).map(Pending::define));
        exprs.push(Hir::silent(HirKind::Var(value), span));
        Hir::new(HirKind::Begin(exprs), span, signal)
    }

    /// The merged bindings waiting to be defined, for the expression about to
    /// be analyzed to run first.
    pub(in crate::hir::analyze) fn take_merges(&mut self) -> Vec<Hir> {
        self.defs.pending.drain(..).map(Pending::define).collect()
    }

    /// The merged bindings waiting to be defined, each with its value, for a
    /// file to define as its own entries.
    pub(in crate::hir::analyze) fn take_merged_entries(&mut self) -> Vec<(Binding, Hir)> {
        self.defs
            .pending
            .drain(..)
            .map(|p| (p.merged, p.value()))
            .collect()
    }

    /// Drop the merged bindings waiting in the scope at `depth` or deeper:
    /// nothing after them in their scope runs.
    pub(in crate::hir::analyze) fn discard_pending(&mut self, depth: usize) {
        self.defs.pending.retain(|p| p.depth < depth);
    }
}
