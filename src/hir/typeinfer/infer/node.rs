// audited: 2026-09-08
// src/hir/AGENTS.md
// docs/impl/typeinfer.md
//! The transfer function: one node's type, given the environment the ascent has
//! built so far. The binding forms, the branching forms and the call rules are
//! each a submodule; what stays here is the dispatch and the leaves.

use super::*;

mod binder;
mod branch;
mod call;

impl Infer<'_> {
    /// Infer the type of a single HIR node.
    pub(super) fn infer(&mut self, hir: &Hir) -> TyId {
        match &hir.kind {
            // Literals
            HirKind::Nil => TypeInterner::NIL,
            HirKind::Bool(_) => TypeInterner::BOOL,
            HirKind::Int(_) => TypeInterner::INT,
            HirKind::Float(_) => TypeInterner::FLOAT,
            HirKind::String(_) => TypeInterner::STRING,
            HirKind::Keyword(_) => TypeInterner::KEYWORD,
            HirKind::EmptyList => TypeInterner::EMPTY_LIST,

            // Variable reference
            HirKind::Var(binding) => self
                .binding_types
                .get(binding)
                .copied()
                .unwrap_or(TypeInterner::TOP),

            // Intrinsic operations — known return types
            HirKind::Intrinsic { op, args } => {
                for arg in args {
                    let ty = self.infer(arg);
                    self.hir_types.insert(arg.id, ty);
                }
                intrinsic_return_type(*op, args, &self.interner, &self.hir_types)
            }

            // Binding forms — `binder.rs`
            HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
                self.infer_let(bindings, body)
            }
            HirKind::Assign { target, value }
            | HirKind::Define {
                binding: target,
                value,
            } => self.infer_define(*target, value),
            HirKind::Loop { bindings, body } => self.infer_loop(bindings, body),
            HirKind::MakeCell { value } => self.infer_make_cell(value),
            HirKind::DerefCell { cell } => self.infer_deref_cell(cell),
            HirKind::SetCell { cell, value } => self.infer_set_cell(cell, value),

            // Branching forms — `branch.rs`
            HirKind::If {
                cond,
                then_branch,
                else_branch,
            } => self.infer_if(cond, then_branch, else_branch),
            HirKind::Match { value, arms } => self.infer_match(value, arms),
            HirKind::Cond {
                clauses,
                else_branch,
            } => self.infer_cond(clauses, else_branch.as_deref()),
            HirKind::Begin(exprs) => self.infer_sequence(exprs),
            HirKind::Block { body, .. } => self.infer_sequence(body),
            HirKind::And(exprs) | HirKind::Or(exprs) => self.infer_shortcircuit(exprs),

            // Call — `call.rs`
            HirKind::Call { func, args, .. } => self.infer_call(func, args),

            // Lambda — infer body type; the lambda value itself is a closure
            HirKind::Lambda { params, body, .. } => {
                // A `(numeric!)` declaration is the programmer's numeric contract for
                // the whole body (the GPU-eligibility gate holds the lowered code to
                // it), so it proves every parameter ⊑ Number for the operand contracts
                // — the declared analog of a call-site join. It is read from the
                // parameter bindings, the single place it is recorded. An undeclared
                // parameter is left ALONE — absence from the environment is meaningful
                // (the ascent's Kleene start: an unproven parameter reads as Top, a
                // provable one is seeded at Bottom).
                for p in params
                    .iter()
                    .filter(|p| self.arena.get(**p).declared_numeric)
                {
                    let old = self
                        .binding_types
                        .get(p)
                        .copied()
                        .unwrap_or(TypeInterner::TOP);
                    let floored = declared_floor(*p, old, self.arena, &self.interner);
                    self.binding_types.insert(*p, floored);
                }
                let body_ty = self.infer(body);
                self.hir_types.insert(body.id, body_ty);
                TypeInterner::TOP
            }

            // Return — the function-return ownership boundary is region-only and
            // type-transparent: the result is the same value. Without this arm a
            // lambda's body type (wrapped in Return by wrap_tail_returns) would
            // read Top and no callee return type would ever flow to callers.
            HirKind::Return { value } => {
                let ty = self.infer(value);
                self.hir_types.insert(value.id, ty);
                ty
            }

            // Quoted compound data has the type its template's outermost node
            // materializes to — a quoted proper list IS a pair chain, which is
            // what proves `(%first '(a b c))`.
            HirKind::QuoteConst(template) => {
                use crate::value::ConstTemplate;
                match template {
                    ConstTemplate::Pair(_, _) => TypeInterner::PAIR,
                    ConstTemplate::Array(_) => TypeInterner::ARRAY,
                    ConstTemplate::ArrayMut(_) => TypeInterner::MUTABLE_ARRAY,
                    ConstTemplate::String(_) => TypeInterner::STRING,
                    ConstTemplate::StringMut(_) => TypeInterner::MUTABLE_STRING,
                    ConstTemplate::EmptyList => TypeInterner::EMPTY_LIST,
                    ConstTemplate::Int(_) => TypeInterner::INT,
                    ConstTemplate::Float(_) => TypeInterner::FLOAT,
                    ConstTemplate::Bool(_) => TypeInterner::BOOL,
                    ConstTemplate::Keyword(_) => TypeInterner::KEYWORD,
                    ConstTemplate::Symbol(_) => TypeInterner::SYMBOL,
                    ConstTemplate::Nil => TypeInterner::NIL,
                    _ => TypeInterner::TOP,
                }
            }

            // Everything else — recurse and return Top
            _ => {
                hir.for_each_child(|child| {
                    let ty = self.infer(child);
                    self.hir_types.insert(child.id, ty);
                });
                TypeInterner::TOP
            }
        }
    }
}
