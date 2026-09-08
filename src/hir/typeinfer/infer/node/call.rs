// audited: 2026-09-08
// src/hir/AGENTS.md
// docs/impl/typeinfer.md
//! What a call proves: the arguments it forwards to the callee's parameters,
//! and the type its own result carries away (docs/impl/typeinfer.md).

use super::*;
use crate::hir::expr::CallArg;

impl Infer<'_> {
    /// Call — forward arg types to callee params; result = callee return type.
    pub(super) fn infer_call(&mut self, func: &Hir, args: &[CallArg]) -> TyId {
        let func_ty = self.infer(func);
        self.hir_types.insert(func.id, func_ty);

        let arg_types: Vec<TyId> = args
            .iter()
            .map(|a| {
                let ty = self.infer(&a.expr);
                self.hir_types.insert(a.expr.id, ty);
                ty
            })
            .collect();

        // Forward arg types to callee params.
        // Handle both Var(b) and DerefCell { Var(b) } (letrec recursive calls).
        // Forwarding is a complete proof only for a callee used EXCLUSIVELY
        // in call position — a single value-position use (stored, passed,
        // returned, exported) means invisible callers exist, so the joins
        // must not prove its parameters.
        let callee_binding = unwrap_callee_binding(func);
        if let Some(callee_binding) = callee_binding {
            if let Some(params) = self
                .lambda_params
                .get(&callee_binding)
                .filter(|_| !self.value_used.contains(&callee_binding))
            {
                for (i, param) in params.iter().enumerate() {
                    if let Some(&arg_ty) = arg_types.get(i) {
                        // Top contributes honestly: an unknown-typed call
                        // site makes the parameter unprovable (the ascent
                        // REPLACES the binding type from this map at pass
                        // end, so recursion converges from below instead
                        // of needing a Top skip).
                        let old = self
                            .param_joins
                            .get(param)
                            .copied()
                            .unwrap_or(TypeInterner::BOTTOM);
                        let joined = self.interner.join(old, arg_ty);
                        self.param_joins.insert(*param, joined);
                    }
                }
            }
            // Return type = whatever the callee's body returns, as the
            // previous pass left it.
            //
            // A SELF-recursive call takes no exception here: it reads the map
            // every other call reads. On the first pass the callee's own body
            // has not been walked yet, so the map holds no entry for it, and
            // BOTTOM is exactly what the recursive contribution to a
            // return-type join is worth — the base cases. Every later pass
            // reads the estimate the pass before it computed, which is Kleene
            // iteration from below and converges on the least fixpoint
            // (docs/impl/typeinfer.md § "What a call contributes").
            //
            // For unknown callees (primitives, imports) TOP, to avoid unsound
            // rewrites.
            if self.lambda_params.contains_key(&callee_binding) {
                return self
                    .lambda_body_type
                    .get(&callee_binding)
                    .copied()
                    .unwrap_or(TypeInterner::BOTTOM);
            }

            // Primitive return type inference for unresolved callees
            let callee_sym = self.arena.get(callee_binding).name;
            let prim_ty = primitive_return_type(callee_sym, &arg_types, &self.interner);
            if prim_ty != TypeInterner::TOP {
                return prim_ty;
            }
        }

        TypeInterner::TOP
    }
}
