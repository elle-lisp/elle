// audited: 2026-09-08
// src/hir/AGENTS.md
// docs/impl/typeinfer.md
//! The ascent: passes over the whole tree until the type environment stops
//! moving, each one reading strictly more than the last.

use super::*;

/// How many passes one ascent may take. Information travels one call at a time
/// in walk order, so the count a program needs is the depth of its call chains
/// rather than their size; the whole corpus settles in six or fewer.
pub(in crate::hir::typeinfer) const MAX_ITERS: usize = 10;

impl Infer<'_> {
    /// Run the transfer function over `hir` until the environment stops moving,
    /// widening whatever is still moving when the budget runs out.
    pub(in crate::hir::typeinfer) fn solve(&mut self, hir: &Hir) {
        while let Some(moving) = self.ascend(hir) {
            // A round that pins nothing new has pinned everything there is, so
            // the environment is already Top throughout and proves nothing.
            // Stopping there is what bounds the widening.
            if !self.widen(moving) {
                return;
            }
        }
    }

    /// Passes until the environment settles or the budget runs out. `None` is
    /// convergence; `Some` names every binding the last pass was still moving.
    ///
    /// Convergence is judged on the whole type environment, not the root node's
    /// type: a call site visited late in a pass joins into a callee parameter
    /// whose occurrences were recorded earlier, so the refinement only reaches
    /// them on the next pass.
    fn ascend(&mut self, hir: &Hir) -> Option<Vec<Binding>> {
        let mut moving = Vec::new();
        for _ in 0..MAX_ITERS {
            let before_hir = self.hir_types.clone();
            let before_bindings = self.binding_types.clone();
            let before_bodies = self.lambda_body_type.clone();
            self.pass(hir);
            if before_hir == self.hir_types
                && before_bindings == self.binding_types
                && before_bodies == self.lambda_body_type
            {
                return None;
            }
            moving = moved(&before_bindings, &self.binding_types)
                .chain(moved(&before_bodies, &self.lambda_body_type))
                .collect();
        }
        Some(moving)
    }

    /// Hold every entry the last pass was still moving at Top, and report
    /// whether that pinned anything new.
    ///
    /// An ascent that ran out of budget left the environment BELOW the least
    /// fixpoint, which is the side that proves things that are not true: an
    /// entry still at Bottom discharges every subtype contract that asks about
    /// it. Top is above the fixpoint, so a pinned entry proves nothing. Pinning
    /// also freezes the entry, so each round pins strictly more than the last
    /// and the widening terminates.
    fn widen(&mut self, moving: Vec<Binding>) -> bool {
        let before = self.widened.len();
        self.widened.extend(moving);
        if self.widened.len() == before {
            // Nothing a binding carries moved, so the movement is in the node
            // types alone. Hold the whole environment at Top: a pass over a
            // frozen environment is a function of the tree alone, so the pass
            // after it changes nothing.
            let all: Vec<Binding> = self
                .binding_types
                .keys()
                .chain(self.lambda_body_type.keys())
                .copied()
                .collect();
            self.widened.extend(all);
        }
        self.widened.len() > before
    }

    /// One pass: infer every node from the environment the last pass left, then
    /// REPLACE each contributed parameter's type with this pass's complete join
    /// (Top included). A `(numeric!)` declaration floors the result at Number
    /// (meet: callers can refine to Int/Float, never widen past the declared
    /// contract); a mutated parameter never receives proofs.
    fn pass(&mut self, hir: &Hir) {
        self.param_joins.clear();
        let ty = self.infer(hir);
        self.hir_types.insert(hir.id, ty);
        for (param, joined) in std::mem::take(&mut self.param_joins) {
            if self.mutated_params.contains(&param) {
                continue;
            }
            let floored = declared_floor(param, joined, self.arena, &self.interner);
            self.binding_types.insert(param, floored);
        }
        // Anything widened is held at Top for the rest of the ascent, over
        // whatever this pass computed for it.
        for b in &self.widened {
            self.binding_types.insert(*b, TypeInterner::TOP);
            if self.lambda_params.contains_key(b) {
                self.lambda_body_type.insert(*b, TypeInterner::TOP);
            }
        }
    }
}

/// The keys whose type differs between two snapshots of one map. An entry is
/// only ever inserted or refined, so a key the later snapshot lacks is not a
/// move.
fn moved<'m>(
    before: &'m HashMap<Binding, TyId>,
    after: &'m HashMap<Binding, TyId>,
) -> impl Iterator<Item = Binding> + 'm {
    after
        .iter()
        .filter(|(b, ty)| before.get(*b) != Some(*ty))
        .map(|(b, _)| *b)
}
