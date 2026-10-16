// audited: 2026-10-06
//! What a call proves: the operand types it forwards to the callee's parameters, and its result's type.
//!
//! docs/impl/typeinfer.md

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

        // Forward the operand types to the callee's parameters. The callee is
        // a `Var(b)` or, for a letrec recursive call, a `DerefCell { Var(b) }`.
        // Forwarding proves a parameter only when every use of the callee is
        // in call position. One value-position use (stored, passed, returned,
        // exported) means callers this unit cannot see.
        let callee_binding = unwrap_callee_binding(func);
        if let Some(callee_binding) = callee_binding {
            if let Some(sig) = self
                .lambda_params
                .get(&callee_binding)
                .filter(|_| !self.value_used.contains(&callee_binding))
            {
                // An unknown operand contributes Top, which leaves the
                // parameter unproven. A spliced operand spreads into positions
                // nobody can count, so every parameter from its position on is
                // unknown. A fixed parameter with no operand is an `&opt` one
                // bound to nil.
                let spliced_at = args.iter().position(|a| a.spliced).unwrap_or(args.len());
                let contributions: Vec<(Binding, TyId)> = sig
                    .fixed
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        let ty = if i >= spliced_at {
                            TypeInterner::TOP
                        } else {
                            arg_types.get(i).copied().unwrap_or(TypeInterner::NIL)
                        };
                        (*p, ty)
                    })
                    .chain(sig.rest.map(|(rest, collects_list)| {
                        let ty = if !collects_list || spliced_at < args.len() {
                            TypeInterner::TOP
                        } else if args.len() > sig.fixed.len() {
                            TypeInterner::PAIR
                        } else {
                            TypeInterner::EMPTY_LIST
                        };
                        (rest, ty)
                    }))
                    .collect();
                for (param, ty) in contributions {
                    self.join_param(param, ty);
                }
            }
            // The result is the callee's body type, as the previous pass left
            // it. A self-recursive call reads the same map. On the first pass
            // the callee's body has no entry yet, and the Bottom it reads is
            // what the recursive contribution to a return-type join is worth.
            if self.lambda_params.contains_key(&callee_binding) {
                return self
                    .lambda_body_type
                    .get(&callee_binding)
                    .copied()
                    .unwrap_or(TypeInterner::BOTTOM);
            }

            // The primitive tables and the stdlib rows answer for the binding
            // the runtime supplies under a name, never for a parameter or a
            // local that shares the name. Any other callee's result is Top.
            let callee = self.arena.get(callee_binding);
            if callee.is_primitive && callee.is_immutable && !callee.is_mutated {
                let prim_ty = primitive_return_type(callee.name, &arg_types, &self.interner);
                if prim_ty != TypeInterner::TOP {
                    return prim_ty;
                }
            }
        }

        TypeInterner::TOP
    }

    /// Join one call site's contribution into a parameter's type for this
    /// pass.
    pub(in crate::hir::typeinfer::infer) fn join_param(&mut self, param: Binding, ty: TyId) {
        let old = self
            .param_joins
            .get(&param)
            .copied()
            .unwrap_or(TypeInterner::BOTTOM);
        let joined = self.interner.join(old, ty);
        self.param_joins.insert(param, joined);
    }
}
