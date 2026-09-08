// audited: 2026-09-08
// src/hir/AGENTS.md
// docs/impl/typeinfer.md
//! What a branch proves: the join over the arms a form can take, and the facts
//! each arm knows because control reached it.

use super::*;
use crate::hir::pattern::HirPattern;

impl Infer<'_> {
    /// If — join branches, with guard narrowing on both sides: the
    /// then-branch gets the condition's truthy facts (`(%int? x)` refines
    /// x to Int), the else-branch its falsy facts (`(%not (number? b))`
    /// false means b IS a number). `meet` because a predicate refines a
    /// union (`Number ∧ Int = Int`) rather than fully determining the type
    /// (contrast the authoritative `match (type-of x)` override below).
    pub(super) fn infer_if(&mut self, cond: &Hir, then_branch: &Hir, else_branch: &Hir) -> TyId {
        let cond_ty = self.infer(cond);
        self.hir_types.insert(cond.id, cond_ty);

        let facts = guard::cond_facts(cond, self.arena);

        let saved = apply_type_facts(&facts.when_true, &mut self.binding_types, &self.interner);
        let then_ty = self.infer(then_branch);
        self.hir_types.insert(then_branch.id, then_ty);
        restore_type_facts(saved, &mut self.binding_types);

        let saved = apply_type_facts(&facts.when_false, &mut self.binding_types, &self.interner);
        let else_ty = self.infer(else_branch);
        self.hir_types.insert(else_branch.id, else_ty);
        restore_type_facts(saved, &mut self.binding_types);

        self.interner.join(then_ty, else_ty)
    }

    /// Match — the type-discriminating sibling of `If`. When the scrutinee is
    /// `(type-of <var>)`, a keyword-literal arm (`:@array`/`:@struct`/…) proves
    /// `<var>`'s concrete type+mutability inside that arm's body — the static
    /// proof a monomorphic data op (`%push-array-mut`, …) needs to lower as a
    /// silent opcode (pinned by the `match_typeof_narrows_*` tests). Children are
    /// visited exactly as `for_each_child` (value, then each guard+body); the
    /// narrowing is saved/restored per arm so it never leaks to a sibling — the
    /// same discipline as the `If` then/else branches above. The Match node's
    /// own type is the join of its arm bodies: a type-dispatch helper that
    /// returns the same type from every arm (the stdlib `compare`'s `rank`,
    /// all-Int) carries that type to its callers, which is what proves the
    /// downstream intrinsic operands.
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
            // Narrow the scrutinee's binding to the arm's proven container
            // type for the duration of the body, then restore. The narrowing
            // **overrides** the binding's accumulated type rather than `meet`ing
            // with it: a `(type-of x)` arm is *authoritative* — the runtime
            // dispatch guarantees `x`'s concrete type whenever the arm runs, so
            // within the body `x` simply IS the arm's keyword type, regardless of
            // what the forward flow widened the binding to across all call sites.
            // `meet` would be wrong here: if the binding accumulated a *disjoint*
            // concrete type (a parameter called elsewhere with a different
            // container — exactly the stdlib `push`/`put` shape), `meet` collapses
            // to `Bottom`, leaving an immutable-arm container "unproven" and its
            // silent monomorphic op a spurious compile error on an arm that only
            // ever runs for that very type (pinned by
            // `match_typeof_arm_narrows_authoritatively_over_a_called_param`). This
            // differs from the `If` type-guard above, which `meet`s because a
            // predicate like `(%int? x)` *refines* a union (`Number ∧ Int = Int`)
            // rather than fully determining the type. Override is sound because
            // the container keyword types are flat — the keyword is the most
            // precise type, never a supertype of the accumulated one.
            let saved = subject
                .zip(pattern_type_keyword(pat))
                .map(|(b, narrow_ty)| {
                    let prev = self.binding_types.get(&b).copied();
                    self.binding_types.insert(b, narrow_ty);
                    (b, prev)
                });
            let body_ty = self.infer(body);
            self.hir_types.insert(body.id, body_ty);
            arm_join = self.interner.join(arm_join, body_ty);
            if let Some((b, prev)) = saved {
                match prev {
                    Some(t) => {
                        self.binding_types.insert(b, t);
                    }
                    None => {
                        self.binding_types.remove(&b);
                    }
                }
            }
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
            restore_type_facts(saved, &mut self.binding_types);
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
        restore_type_facts(fallthrough_saved, &mut self.binding_types);
        ty
    }

    /// Begin/Block — type is last expression. Flow-through guard
    /// narrowing: a one-armed diverging guard statement —
    /// `(when (%not (number? b)) (error …))` — proves its fall-through
    /// facts for every statement after it (the stdlib wrapper shape: the
    /// guard that raises the wrapper's :error is the same fact that
    /// discharges the intrinsic's contract downstream). Facts are scoped
    /// to the sequence and restored on exit.
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
        restore_type_facts(saved, &mut self.binding_types);
        ty
    }

    /// And/Or — `and`/`or` evaluate to one of their operands — `and` the first
    /// falsy (else the last), `or` the first truthy (else the last) — so the
    /// result type is the JOIN of the operand types, exactly as `If` joins its
    /// branches. Join is sound (a returned value is always ⊑ its operand's
    /// type) and lets a homogeneous `(or a b)`/`(and a b)` of proven numbers
    /// discharge a downstream `%`-intrinsic; a heterogeneous one widens and
    /// correctly fails to prove. (Empty `and`/`or` never reach here — the
    /// analyzer emits a Bool literal for them.)
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
