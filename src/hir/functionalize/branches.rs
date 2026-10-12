// audited: 2026-10-04
//! The branch forms of the functionalize walk, where an SSA rename made inside
//! one arm must not escape to code that arm does not dominate.
//!
//! docs/impl/hir.md
//!
//! `transform_begin_at` is the only place that emits a phi, and it intercepts
//! an `if`, `cond` or `match` whose branches assign before they reach here. So
//! every branch form that does reach here is one no phi will guard, and an
//! assign inside its arms stays a runtime slot mutation.

use super::*;
use crate::syntax::Span;

impl<'a> FnCtx<'a> {
    /// The condition is transformed before the save: it always executes, so a
    /// rename from an assign inside it must propagate outward.
    pub(super) fn transform_if(
        &mut self,
        cond: &Hir,
        then_branch: &Hir,
        else_branch: &Hir,
        span: Span,
        signal: Signal,
    ) -> Hir {
        let new_cond = self.transform(cond);
        let saved = self.renames.clone();
        let saved_preserved = self.assign_preserved.clone();
        for branch in [then_branch, else_branch] {
            self.preserve_assigns_in(branch);
        }
        let new_then = self.transform(then_branch);
        self.renames = saved.clone();
        let new_else = self.transform(else_branch);
        self.renames = saved;
        self.assign_preserved = saved_preserved;
        Hir::new(
            HirKind::If {
                cond: Box::new(new_cond),
                then_branch: Box::new(new_then),
                else_branch: Box::new(new_else),
            },
            span,
            signal,
        )
    }

    /// A `cond` as a let init or a call argument, where its assigns must stay
    /// runtime slot mutations.
    pub(super) fn transform_cond(
        &mut self,
        clauses: &[(Hir, Hir)],
        else_branch: Option<&Hir>,
        span: Span,
        signal: Signal,
    ) -> Hir {
        let saved = self.renames.clone();
        let saved_preserved = self.assign_preserved.clone();
        for (_, body) in clauses {
            self.preserve_assigns_in(body);
        }
        if let Some(e) = else_branch {
            self.preserve_assigns_in(e);
        }
        let new_clauses: Vec<_> = clauses
            .iter()
            .map(|(c, b)| {
                self.renames = saved.clone();
                (self.transform(c), self.transform(b))
            })
            .collect();
        self.renames = saved.clone();
        let new_else = else_branch.map(|e| Box::new(self.transform(e)));
        self.renames = saved;
        self.assign_preserved = saved_preserved;
        Hir::new(
            HirKind::Cond {
                clauses: new_clauses,
                else_branch: new_else,
            },
            span,
            signal,
        )
    }

    /// A `match` outside a `begin`, where its arms' assigns must stay runtime
    /// slot mutations.
    pub(super) fn transform_match(
        &mut self,
        value: &Hir,
        arms: &[(HirPattern, Option<Hir>, Hir)],
        span: Span,
        signal: Signal,
    ) -> Hir {
        let new_value = self.transform(value);
        let saved = self.renames.clone();
        let saved_preserved = self.assign_preserved.clone();
        for (_, _, body) in arms {
            self.preserve_assigns_in(body);
        }
        let new_arms: Vec<_> = arms
            .iter()
            .map(|(pat, guard, body)| {
                self.renames = saved.clone();
                (
                    pat.clone(),
                    guard.as_ref().map(|g| self.transform(g)),
                    self.transform(body),
                )
            })
            .collect();
        self.renames = saved;
        self.assign_preserved = saved_preserved;
        Hir::new(
            HirKind::Match {
                value: Box::new(new_value),
                arms: new_arms,
            },
            span,
            signal,
        )
    }

    /// Transform the operands of a short-circuiting `and`/`or`.
    ///
    /// Only the first operand always executes; each later one is conditional on
    /// the ones before it, and neither form has a phi-insertion path, so a
    /// fresh SSA version forked inside a later operand would escape to code
    /// that did not evaluate it. The first operand is transformed before the
    /// save: it always runs, so a rename from an assign inside it must
    /// propagate outward.
    pub(super) fn transform_short_circuit(&mut self, exprs: &[Hir]) -> Vec<Hir> {
        let mut out: Vec<Hir> = Vec::with_capacity(exprs.len());
        if exprs.is_empty() {
            return out;
        }
        out.push(self.transform(&exprs[0]));

        let saved = self.renames.clone();
        let saved_preserved = self.assign_preserved.clone();
        for e in &exprs[1..] {
            self.preserve_assigns_in(e);
        }
        for e in &exprs[1..] {
            self.renames = saved.clone();
            out.push(self.transform(e));
        }
        self.renames = saved;
        self.assign_preserved = saved_preserved;
        out
    }

    /// Mark every binding `branch` assigns as one whose assigns stay runtime
    /// slot mutations, resolved through the current renames.
    fn preserve_assigns_in(&mut self, branch: &Hir) {
        let mut assigned = BTreeSet::new();
        self.collect_assigned_bindings(branch, &mut assigned);
        for b in &assigned {
            let resolved = self.resolve(*b);
            self.assign_preserved.insert(resolved);
        }
    }
}
