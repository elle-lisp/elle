// audited: 2026-09-09
// src/hir/AGENTS.md
// docs/impl/typeinfer.md
//! What a binder records: the type a `let`, a `def` or a cell write leaves for
//! every later read of that binding. A lambda init also records its body type,
//! which is what the binding's callers read — unless something writes the
//! binding, and then they read Top.

use super::*;

impl Infer<'_> {
    /// Record a lambda binding's body type, which is what every call to the
    /// binding reads, a self-call included. Reports whether `init` held a
    /// lambda at all; a cell wrapper around a self-recursive or captured one is
    /// peeled off first.
    ///
    /// REPLACE, don't join: each pass re-derives the body type from strictly
    /// more information, and a join could never come back down from the
    /// estimate an earlier pass computed with less.
    ///
    /// A binding something writes records Top. The initializer's body type
    /// describes one lambda, an `assign` puts a different lambda in the same
    /// binding, and the pass cannot tell which one a call reaches — the same
    /// argument that denies a mutated parameter its call-site proofs. The rule
    /// sits here rather than on the write, so a route that reaches the write
    /// differently cannot defeat it: `functionalize` rewrites a top-level
    /// `assign` into a `SetCell`, which records no body type at all
    /// (docs/impl/typeinfer.md § "What still does not prove").
    fn record_lambda_body_type(&mut self, binding: Binding, init: &Hir) -> bool {
        let HirKind::Lambda { body, .. } = &unwrap_make_cell(init).kind else {
            return false;
        };
        let body_ty = if self.mutated_params.contains(&binding) {
            TypeInterner::TOP
        } else {
            self.hir_types
                .get(&body.id)
                .copied()
                .unwrap_or(TypeInterner::TOP)
        };
        self.lambda_body_type.insert(binding, body_ty);
        true
    }

    /// Let/Letrec — seed binding types from init values.
    pub(super) fn infer_let(&mut self, bindings: &[(Binding, Hir)], body: &Hir) -> TyId {
        for (binding, init) in bindings {
            let ty = self.infer(init);
            self.hir_types.insert(init.id, ty);
            if !self.record_lambda_body_type(*binding, init) {
                let old = self
                    .binding_types
                    .get(binding)
                    .copied()
                    .unwrap_or(TypeInterner::BOTTOM);
                let joined = self.interner.join(old, ty);
                // A `(numeric!)`-declared binding keeps its floor here: this is
                // the form a kernel parameter takes once HOF fusion has spliced
                // its body into a loop (`(let [x (get coll i)] BODY)`), where
                // the init's own type carries no proof.
                let floored = declared_floor(*binding, joined, self.arena, &self.interner);
                self.binding_types.insert(*binding, floored);
                // Track min_length for array constructor bindings
                if ty == TypeInterner::MUTABLE_ARRAY || ty == TypeInterner::ARRAY {
                    if let Some(len) = unwrap_to_call(init) {
                        self.binding_min_length.insert(*binding, len);
                    }
                }
            }
        }
        let body_ty = self.infer(body);
        self.hir_types.insert(body.id, body_ty);
        body_ty
    }

    /// Assign/Define — update binding type. A Define whose value is a
    /// lambda (the in-function `defn` idiom — a letrec*-semantics local)
    /// records its return type exactly like a Let/Letrec lambda binding;
    /// `collect_lambda_info` records its params.
    pub(super) fn infer_define(&mut self, target: Binding, value: &Hir) -> TyId {
        let ty = self.infer(value);
        self.record_lambda_body_type(target, value);
        self.hir_types.insert(value.id, ty);
        let old = self
            .binding_types
            .get(&target)
            .copied()
            .unwrap_or(TypeInterner::BOTTOM);
        let joined = self.interner.join(old, ty);
        self.binding_types.insert(target, joined);
        // Track min_length for array constructor bindings
        if ty == TypeInterner::MUTABLE_ARRAY || ty == TypeInterner::ARRAY {
            if let Some(call) = unwrap_to_call(value) {
                self.binding_min_length.insert(target, call);
            }
        }
        ty
    }

    /// Loop — recurse into body.
    pub(super) fn infer_loop(&mut self, bindings: &[(Binding, Hir)], body: &Hir) -> TyId {
        for (binding, init) in bindings {
            let ty = self.infer(init);
            self.hir_types.insert(init.id, ty);
            let old = self
                .binding_types
                .get(binding)
                .copied()
                .unwrap_or(TypeInterner::BOTTOM);
            let joined = self.interner.join(old, ty);
            self.binding_types.insert(*binding, joined);
        }
        let body_ty = self.infer(body);
        self.hir_types.insert(body.id, body_ty);
        body_ty
    }

    /// MakeCell — propagate inner value type.
    pub(super) fn infer_make_cell(&mut self, value: &Hir) -> TyId {
        let ty = self.infer(value);
        self.hir_types.insert(value.id, ty);
        ty
    }

    /// DerefCell — return binding type if cell is Var(b).
    pub(super) fn infer_deref_cell(&mut self, cell: &Hir) -> TyId {
        let ty = self.infer(cell);
        self.hir_types.insert(cell.id, ty);
        if let HirKind::Var(b) = &cell.kind {
            self.binding_types
                .get(b)
                .copied()
                .unwrap_or(TypeInterner::TOP)
        } else {
            TypeInterner::TOP
        }
    }

    /// SetCell — widen binding type.
    pub(super) fn infer_set_cell(&mut self, cell: &Hir, value: &Hir) -> TyId {
        let cell_ty = self.infer(cell);
        self.hir_types.insert(cell.id, cell_ty);
        let val_ty = self.infer(value);
        self.hir_types.insert(value.id, val_ty);
        // Widen the binding's type with the new value
        if let HirKind::Var(b) = &cell.kind {
            let old = self
                .binding_types
                .get(b)
                .copied()
                .unwrap_or(TypeInterner::BOTTOM);
            let joined = self.interner.join(old, val_ty);
            self.binding_types.insert(*b, joined);
        }
        val_ty
    }
}
