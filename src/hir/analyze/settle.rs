// audited: 2026-10-24
//! Settle a unit's signals after the pass: solve them, raise the HIR to the model, then check each ceiling and bound.
//!
//! docs/pipeline.md
//! docs/impl/solver.md

use super::call::BoundArg;
use super::*;
use crate::hir::expr::HirId;
use crate::hir::solve::{self, Answers, Extractor, Model, Unit, VarKey};
use crate::signals::registry;

impl<'a> Analyzer<'a> {
    /// The signal of each binding the compile started with: a primitive, or
    /// a core, stdlib or compile-time export, as recorded when it was bound.
    pub fn fixed_signals(&self) -> HashMap<Binding, Signal> {
        self.signal_env
            .iter()
            .filter(|(b, _)| {
                self.arena.get(**b).is_primitive || self.primitive_values.contains_key(*b)
            })
            .map(|(b, s)| (*b, *s))
            .collect()
    }

    /// Solve the signals of the unit the pass built as `hir`, and raise every
    /// call, node and lambda in it to the model. Then check each ceiling, and
    /// each bound a call meets, against the raised signals.
    pub(super) fn settle(&mut self, hir: &mut Hir) -> Result<(), String> {
        let fixed = self.fixed_signals();
        let mut model = Model::default();
        let unit = Unit {
            id: 0,
            hir: &*hir,
            arena: &*self.arena,
            decls: &self.lambda_decls,
            top_direct: self.current_signal_sources.direct_bits,
            fixed: &fixed,
        };
        let mut ex = Extractor::new(&mut model, &unit);
        let top = ex.model.var(VarKey::Top(0));
        ex.walk_file(top);
        let vars: HashMap<Binding, solve::VarId> = self
            .bound_calls
            .iter()
            .flat_map(|call| call.bindings())
            .map(|b| (b, ex.binding_var(b)))
            .collect();
        let calls = std::mem::take(&mut ex.calls);
        let pending = std::mem::take(&mut ex.pending);

        solve::link(&mut model, &HashMap::new(), vec![pending]);
        let lowered = solve::lower(&model);
        let solved = solve::solve(&model, &lowered);
        let answers = Answers::new(&model, &solved, &calls, 0);
        answers.raise(hir);

        let mut lambdas = HashMap::new();
        self.check_ceilings(hir, &mut lambdas)?;
        self.check_bound_calls(|arg| match arg {
            BoundArg::Lambda(id) => lambdas.get(&id).copied(),
            BoundArg::Binding(b) => Some(
                self.known_binding_signal(b)?
                    .combine(answers.signal_of(vars[&b])),
            ),
        })
    }

    /// Check each lambda's ceiling against its raised signal, innermost
    /// first, and record each lambda's signal in `lambdas`. A ceiling checks
    /// and does not replace: a caller sees what the body does, not what the
    /// declaration admits.
    fn check_ceilings(
        &self,
        hir: &Hir,
        lambdas: &mut HashMap<HirId, Signal>,
    ) -> Result<(), String> {
        let mut result = Ok(());
        hir.for_each_child(|c| {
            if result.is_ok() {
                result = self.check_ceilings(c, lambdas);
            }
        });
        result?;
        let HirKind::Lambda(lambda) = &hir.kind else {
            return Ok(());
        };
        let inferred = lambda.inferred_signals;
        lambdas.insert(hir.id, inferred);
        let Some(ceiling) = self.lambda_decls.get(&hir.id).and_then(|d| d.ceiling) else {
            return Ok(());
        };
        let excess = inferred.bits.subtract(ceiling.bits);
        if !excess.is_empty() {
            return Err(format!(
                "{}: function restricted to {} but body may emit {}",
                hir.span,
                registry::format_bits(ceiling.bits),
                registry::format_bits(excess),
            ));
        }
        if ceiling.propagates == 0 && inferred.propagates != 0 {
            return Err(format!(
                "{}: function restricted to {} but body passes on the signal of its parameters",
                hir.span,
                registry::format_bits(ceiling.bits),
            ));
        }
        Ok(())
    }
}
