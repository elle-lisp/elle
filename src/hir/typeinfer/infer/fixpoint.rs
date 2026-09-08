// audited: 2026-09-08
// src/hir/AGENTS.md
// docs/impl/typeinfer.md
//! The ascent: passes over the whole tree until the type environment stops
//! moving, each one reading strictly more than the last.

use super::*;

/// How many passes one ascent may take. Information travels one call at a time
/// in walk order, so the count a program needs is the depth of its call chains
/// rather than their size; the whole corpus settles in five or fewer.
pub(in crate::hir::typeinfer) const MAX_ITERS: usize = 10;

impl Infer<'_> {
    /// Run the transfer function over `hir` until the environment stops moving.
    ///
    /// Convergence is judged on the whole type environment, not the root node's
    /// type: a call site visited late in a pass joins into a callee parameter
    /// whose occurrences were recorded earlier, so the refinement only reaches
    /// them on the next pass.
    pub(in crate::hir::typeinfer) fn solve(&mut self, hir: &Hir) {
        for _ in 0..MAX_ITERS {
            let before_hir = self.hir_types.clone();
            let before_bindings = self.binding_types.clone();
            self.pass(hir);
            if before_hir == self.hir_types && before_bindings == self.binding_types {
                return;
            }
        }
    }

    /// One pass: infer every node from the environment the last pass left, then
    /// REPLACE each contributed parameter's type with this pass's complete join
    /// (Top included). A `(numeric!)` declaration floors the result at Number
    /// (meet: callers can refine to Int/Float, never widen past the declared
    /// contract); a mutated parameter never receives proofs.
    fn pass(&mut self, hir: &Hir) {
        self.param_joins.clear();
        self.selfrec.clear();
        let ty = self.infer(hir);
        self.hir_types.insert(hir.id, ty);
        for (param, joined) in std::mem::take(&mut self.param_joins) {
            if self.mutated_params.contains(&param) {
                continue;
            }
            let floored = declared_floor(param, joined, self.arena, &self.interner);
            self.binding_types.insert(param, floored);
        }
    }
}
