// audited: 2026-10-21
//! What a branch proves: the join over the arms a form can take, and the facts each arm knows.
//!
//! docs/impl/typeinfer.md

use super::*;
use crate::hir::pattern::HirPattern;

impl Infer<'_> {
    /// If — join the branches, with guard narrowing on both sides. The
    /// then-branch gets the condition's truthy facts (`(%int? x)` refines x to
    /// Int), and the else-branch its falsy facts (`(%not (number? b))` false
    /// means b is a number). A fact meets the binding's type, because a
    /// predicate refines a union (`Number ∧ Int = Int`), where a
    /// `match (type-of x)` arm below overrides it.
    pub(super) fn infer_if(&mut self, cond: &Hir, then_branch: &Hir, else_branch: &Hir) -> TyId {
        let cond_ty = self.infer(cond);
        self.hir_types.insert(cond.id, cond_ty);

        let facts = guard::cond_facts(cond, self.arena);

        let saved = apply_type_facts(&facts.when_true, &mut self.binding_types, &self.interner);
        let then_ty = self.infer(then_branch);
        self.hir_types.insert(then_branch.id, then_ty);
        restore_type_facts(saved, &mut self.binding_types, &self.interner);

        let saved = apply_type_facts(&facts.when_false, &mut self.binding_types, &self.interner);
        let else_ty = self.infer(else_branch);
        self.hir_types.insert(else_branch.id, else_ty);
        restore_type_facts(saved, &mut self.binding_types, &self.interner);

        self.interner.join(then_ty, else_ty)
    }

    /// Match — the type-discriminating sibling of `If`. When the scrutinee is
    /// `(type-of <var>)`, a keyword-literal arm (`:@array`, `:@struct`, …)
    /// proves `<var>`'s concrete type and mutability inside that arm's body.
    /// That is the proof a monomorphic data op such as `%push-array-mut` needs
    /// (pinned by the `match_typeof_narrows_*` tests).
    ///
    /// Children are visited in `for_each_child` order: the value, then each
    /// guard and body. Each arm restores its narrowing, so it never leaks to a
    /// sibling. The node's type is the join of its arm bodies, so a dispatch
    /// that returns one type from every arm, as the stdlib `compare`'s `rank`
    /// returns an Int, carries that type to its callers.
    pub(super) fn infer_match(
        &mut self,
        value: &Hir,
        arms: &[(HirPattern, Option<Hir>, Hir)],
    ) -> TyId {
        let val_ty = self.infer(value);
        self.hir_types.insert(value.id, val_ty);

        let subject = typeof_subject_binding(value, self.arena, &self.typeof_aliases);
        let mut arm_join = TypeInterner::BOTTOM;
        for (pat, guard, body) in arms {
            if let Some(g) = guard {
                let g_ty = self.infer(g);
                self.hir_types.insert(g.id, g_ty);
            }
            // Narrow the scrutinee's binding to the arm's type for the body,
            // then restore. The arm overrides the binding's type rather than
            // meeting it: the runtime dispatch guarantees `x`'s type whenever
            // the arm runs. A parameter called elsewhere with a disjoint
            // container would meet to Bottom and leave the arm's op unproven
            // (pinned by
            // `match_typeof_arm_narrows_authoritatively_over_a_called_param`).
            // The override is sound because each keyword names one exact type.
            let saved: Vec<Narrowed> = subject
                .zip(pattern_type_keyword(pat))
                .map(|(b, narrow_ty)| narrow_type(b, narrow_ty, &mut self.binding_types))
                .into_iter()
                .collect();
            let body_ty = self.infer(body);
            self.hir_types.insert(body.id, body_ty);
            arm_join = self.interner.join(arm_join, body_ty);
            restore_type_facts(saved, &mut self.binding_types, &self.interner);
        }
        if arms.is_empty() {
            TypeInterner::TOP
        } else {
            arm_join
        }
    }

    /// Cond — sequential If chain: each clause body gets its test's truthy
    /// facts; each later clause (and the else) additionally knows every
    /// earlier test was falsy. Result is the join of all bodies.
    pub(super) fn infer_cond(&mut self, clauses: &[(Hir, Hir)], else_branch: Option<&Hir>) -> TyId {
        let mut ty = TypeInterner::BOTTOM;
        let mut fallthrough_saved = Vec::new();
        for (test, body) in clauses {
            let test_ty = self.infer(test);
            self.hir_types.insert(test.id, test_ty);
            let facts = guard::cond_facts(test, self.arena);
            let saved = apply_type_facts(&facts.when_true, &mut self.binding_types, &self.interner);
            let body_ty = self.infer(body);
            self.hir_types.insert(body.id, body_ty);
            restore_type_facts(saved, &mut self.binding_types, &self.interner);
            ty = self.interner.join(ty, body_ty);
            fallthrough_saved.extend(apply_type_facts(
                &facts.when_false,
                &mut self.binding_types,
                &self.interner,
            ));
        }
        if let Some(els) = else_branch {
            let else_ty = self.infer(els);
            self.hir_types.insert(els.id, else_ty);
            ty = self.interner.join(ty, else_ty);
        }
        restore_type_facts(fallthrough_saved, &mut self.binding_types, &self.interner);
        ty
    }

    /// Begin/Block — the type is the last expression's. A one-armed guard
    /// statement whose taken branch diverges, such as
    /// `(when (%not (number? b)) (error …))`, proves its fall-through facts
    /// for every statement after it. That is the stdlib wrapper shape: the
    /// guard that raises is the fact that proves the op after it. The facts
    /// end with the sequence.
    pub(super) fn infer_sequence(&mut self, exprs: &[Hir]) -> TyId {
        let mut ty = TypeInterner::NIL;
        let mut saved = Vec::new();
        for expr in exprs {
            ty = self.infer(expr);
            self.hir_types.insert(expr.id, ty);
            let facts = guard::facts_after_statement(expr, self.arena);
            saved.extend(apply_type_facts(
                &facts,
                &mut self.binding_types,
                &self.interner,
            ));
        }
        restore_type_facts(saved, &mut self.binding_types, &self.interner);
        ty
    }

    /// Block — the block evaluates to its tail, or to the value of a `break`
    /// that names it, so its type joins the two.
    pub(super) fn infer_block(&mut self, id: crate::hir::expr::BlockId, body: &[Hir]) -> TyId {
        let outer = self.break_types.remove(&id);
        let tail = self.infer_sequence(body);
        let breaks = self.break_types.remove(&id).unwrap_or(TypeInterner::BOTTOM);
        if let Some(t) = outer {
            self.break_types.insert(id, t);
        }
        self.interner.join(tail, breaks)
    }

    /// Break — its value joins the block it names. The `break` itself leaves no
    /// value where it stands, so its own type is Bottom.
    pub(super) fn infer_break(&mut self, id: crate::hir::expr::BlockId, value: &Hir) -> TyId {
        let ty = self.infer(value);
        self.hir_types.insert(value.id, ty);
        let joined = match self.break_types.get(&id) {
            Some(t) => self.interner.join(*t, ty),
            None => ty,
        };
        self.break_types.insert(id, joined);
        TypeInterner::BOTTOM
    }

    /// And/Or — the form evaluates to one of its operands: `and` to the first
    /// falsy one, `or` to the first truthy one, else the last. So the type is
    /// the join of the operand types, as `If` joins its branches. An `(or a b)`
    /// of proven numbers proves a later `%`-intrinsic, and a mixed one does
    /// not. An empty `and` or `or` never reaches here: the analyzer emits a
    /// Bool literal for it.
    pub(super) fn infer_shortcircuit(&mut self, exprs: &[Hir]) -> TyId {
        let mut join = TypeInterner::BOTTOM;
        for child in exprs {
            let ty = self.infer(child);
            self.hir_types.insert(child.id, ty);
            join = self.interner.join(join, ty);
        }
        join
    }
}
