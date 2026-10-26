// audited: 2026-10-25
//! The compile-time check of a call's argument count against its callee's known arity.
//!
//! docs/pipeline.md
//! docs/signals/inference.md

use super::*;

/// A call through a binding whose count does not match the binding's arity
/// as the pass knew it, and the error that reports it.
pub(in crate::hir::analyze) struct CountMismatch {
    callee: Binding,
    message: String,
}

impl<'a> Analyzer<'a> {
    /// Check a call of `func` with `count` arguments against the callee's
    /// known arity. A lambda literal that refuses the count fails at once. A
    /// binding's arity holds only if no `assign` reaches the binding, and an
    /// `assign` may stand after the call, so its mismatch waits for
    /// `report_count_mismatches`.
    pub(super) fn check_count(
        &mut self,
        func: &Hir,
        count: usize,
        span: Span,
    ) -> Result<(), String> {
        let Some(arity) = self.get_callee_arity(func) else {
            return Ok(());
        };
        if arity.matches(count) {
            return Ok(());
        }
        let message = format!(
            "{}: arity error: {} expects {} argument{}, got {}",
            span,
            self.callee_name(func),
            arity,
            if arity == Arity::Exact(1) { "" } else { "s" },
            count,
        );
        match &func.kind {
            HirKind::Var(callee) => {
                self.count_mismatches.push(CountMismatch {
                    callee: *callee,
                    message,
                });
                Ok(())
            }
            _ => Err(message),
        }
    }

    /// Report the first count mismatch whose callee binding no `assign`
    /// reaches. A call through an assigned binding checks its count when it
    /// runs, against the value it finds.
    pub(in crate::hir::analyze) fn report_count_mismatches(&mut self) -> Result<(), String> {
        let mismatches = std::mem::take(&mut self.count_mismatches);
        match mismatches
            .into_iter()
            .find(|m| !self.arena.get(m.callee).is_mutated)
        {
            Some(mismatch) => Err(mismatch.message),
            None => Ok(()),
        }
    }

    /// Get the callee's known arity, if available.
    pub(super) fn get_callee_arity(&self, callee: &Hir) -> Option<Arity> {
        match &callee.kind {
            HirKind::Lambda(lambda) => Some(lambda.arity()),
            HirKind::Var(binding) => {
                // The arity env covers primitives (`bind_primitives`) and
                // bindings initialized with a lambda (`record_lambda_facts`). A
                // user shadow is a new binding, so it carries its own arity or
                // none — never the primitive's.
                self.arity_env.get(binding).copied()
            }
            _ => None,
        }
    }

    /// Get a human-readable name for the callee (for error messages).
    fn callee_name(&self, callee: &Hir) -> String {
        match &callee.kind {
            HirKind::Var(binding) => {
                if let Some(name) = self.symbols.name(self.arena.get(*binding).name) {
                    return name.to_string();
                }
                "<unknown>".to_string()
            }
            HirKind::Lambda(_) => "<lambda>".to_string(),
            _ => "<expression>".to_string(),
        }
    }
}
