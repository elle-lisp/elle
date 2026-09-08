// audited: 2026-09-08
// src/hir/AGENTS.md
// docs/impl/typeinfer.md
//! What a binder records: the type a `let`, a `def`, a loop binding or a cell
//! write leaves in the environment for every later read of that binding.

use super::*;

impl Infer<'_> {
    /// Let/Letrec — seed binding types from init values.
    pub(super) fn infer_let(&mut self, bindings: &[(Binding, Hir)], body: &Hir) -> TyId {
        for (binding, init) in bindings {
            let ty = self.infer(init);
            self.hir_types.insert(init.id, ty);
            // For lambda bindings (possibly cell-wrapped when
            // self-recursive/captured), record their body's return type:
            // this is what every call to the binding reads, a self-call
            // included (docs/impl/typeinfer.md).
            // REPLACE, don't join: each pass re-derives the body type from
            // strictly more information, and a join could never come back
            // down from the estimate an earlier pass computed with less.
            if let HirKind::Lambda { body: lam_body, .. } = &unwrap_make_cell(init).kind {
                let body_ty = self
                    .hir_types
                    .get(&lam_body.id)
                    .copied()
                    .unwrap_or(TypeInterner::TOP);
                self.lambda_body_type.insert(*binding, body_ty);
            } else {
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
        if let HirKind::Lambda { body: lam_body, .. } = &unwrap_make_cell(value).kind {
            let body_ty = self
                .hir_types
                .get(&lam_body.id)
                .copied()
                .unwrap_or(TypeInterner::TOP);
            self.lambda_body_type.insert(target, body_ty);
        }
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
