// audited: 2026-11-22
//! The compile-time refusal of a call whose callee is an immediate the analyzer knows.
//!
//! docs/functions.md
//! src/hir/analyze/overview.md

use super::*;

/// The value of each immutable binding whose initializer is a constant, and
/// each call through a binding whose initializer the pass has not finished.
#[derive(Default)]
pub(in crate::hir::analyze) struct Constants {
    values: HashMap<Binding, Value>,
    pending: Vec<(Binding, Span)>,
}

impl<'a> Analyzer<'a> {
    /// Record `value` as the compile-time value of `binding`, when the binding
    /// is immutable and `constant_value` answers for `value`.
    pub(in crate::hir::analyze) fn record_constant(&mut self, binding: Binding, value: &Hir) {
        if !self.arena.get(binding).is_immutable {
            return;
        }
        let values = &self.constants.values;
        if let Some(constant) = crate::hir::constant_value(value, |b| values.get(b).copied()) {
            self.constants.values.insert(binding, constant);
        }
    }

    /// Refuse a call on `func` when its value is an immediate. A binding whose
    /// initializer is still pending may take its value later in the pass, from
    /// a later `letrec` initializer, so its call waits for
    /// `report_immediate_calls`.
    pub(super) fn check_callee(&mut self, func: &Hir, span: Span) -> Result<(), String> {
        if let HirKind::Var(binding) = &func.kind {
            if self.arena.get(*binding).init_pending {
                self.constants.pending.push((*binding, span));
                return Ok(());
            }
        }
        let values = &self.constants.values;
        match crate::hir::constant_value(func, |b| values.get(b).copied()) {
            Some(value) if !value.is_heap() => Err(self.not_callable(value, span)),
            _ => Ok(()),
        }
    }

    /// Report the first call that waited for its binding's initializer, when
    /// the binding's value is an immediate.
    pub(in crate::hir::analyze) fn report_immediate_calls(&mut self) -> Result<(), String> {
        let pending = std::mem::take(&mut self.constants.pending);
        let refused = pending.into_iter().find_map(|(binding, span)| {
            let value = self.constants.values.get(&binding)?;
            (!value.is_heap()).then_some((*value, span))
        });
        match refused {
            Some((value, span)) => Err(self.not_callable(value, span)),
            None => Ok(()),
        }
    }

    /// The error for a call on `value`, spelled with this instance's names.
    fn not_callable(&self, value: Value, span: Span) -> String {
        let value = value.display_with(Some(&*self.symbols));
        format!("{}: {} is not callable", span, value)
    }
}
