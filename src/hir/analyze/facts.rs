// audited: 2026-10-04
//! What the analyzer records about a lambda for its callers, and how a form
//! that can raise reports itself.
//!
//! docs/signals/inference.md

use super::*;
use crate::hir::pattern::HirPattern;
use crate::value::fiber::SignalBits;

impl<'a> Analyzer<'a> {
    /// Report `bits` as a signal the enclosing function raises on its own.
    ///
    /// A node's signal reaches its parents by `combine`, but a lambda's
    /// inferred signal is built from its sources, not from its body's signal,
    /// so a form that may raise reports here as well: a direct `emit`, a
    /// `match` with no total arm, and every construct in
    /// docs/signals/inference.md "What raises".
    pub(crate) fn add_inherent_bits(&mut self, bits: SignalBits) {
        self.current_signal_sources.direct_bits =
            self.current_signal_sources.direct_bits.union(bits);
    }

    /// The `Destructure` node that binds `pattern` from `value`.
    ///
    /// A strict destructure raises `:type-error` on a missing element, a
    /// missing key or a wrong type, so it carries `:error` and reports it; a
    /// lenient one binds `nil` instead and carries only its value's signal.
    pub(crate) fn destructure(
        &mut self,
        pattern: HirPattern,
        value: Hir,
        strict: bool,
        span: Span,
    ) -> Hir {
        let mut signal = value.signal;
        if strict {
            signal = signal.combine(Signal::errors());
            self.add_inherent_bits(crate::value::SIG_ERROR);
        }
        Hir::new(
            HirKind::Destructure {
                pattern,
                value: Box::new(value),
                strict,
            },
            span,
            signal,
        )
    }

    /// Seed what a binding's lambda initializer promises before the lambda is
    /// analyzed, so a call that precedes it in a letrec reads the right facts:
    /// a silent signal the fixpoint raises, the arity the parameter list
    /// states, and whether the lambda collects keyword arguments.
    pub(crate) fn seed_lambda_facts(&mut self, binding: Binding, value_syntax: &Syntax) {
        let Some(list) = value_syntax.as_list() else {
            return;
        };
        if list.first().and_then(|s| s.as_symbol()) != Some("fn") {
            return;
        }
        self.signal_env.insert(binding, Signal::silent());
        if let Some(params_syn) = list.get(1).and_then(|s| s.as_list_or_tuple()) {
            self.arity_env
                .insert(binding, Self::arity_from_syntax_params(params_syn));
            if Self::params_collect_keywords(params_syn) {
                self.keyword_collectors.insert(binding);
            }
        }
    }

    /// Record what a binding's analyzed initializer established, when it is a
    /// lambda: its inferred signal, its arity, and whether it collects keyword
    /// arguments. Answers whether it was a lambda.
    pub(crate) fn record_lambda_facts(&mut self, binding: Binding, value: &Hir) -> bool {
        let HirKind::Lambda {
            params,
            num_required,
            rest_param,
            vararg_kind,
            inferred_signals,
            ..
        } = &value.kind
        else {
            return false;
        };
        self.signal_env.insert(binding, *inferred_signals);
        let arity = Arity::for_lambda(rest_param.is_some(), *num_required, params.len());
        self.arity_env.insert(binding, arity);
        if rest_param.is_some() && collects_keywords(vararg_kind) {
            self.keyword_collectors.insert(binding);
        } else {
            self.keyword_collectors.remove(&binding);
        }
        true
    }

    /// Whether a parameter list collects keyword arguments, `&keys` or
    /// `&named`. A call to such a function checks its keyword arguments at the
    /// call, so the call may raise.
    pub(crate) fn params_collect_keywords(params: &[Syntax]) -> bool {
        use super::destructure::{CollectorParams, ParsedParams};
        matches!(
            Self::parse_params(params, &Span::synthetic()),
            Ok(ParsedParams {
                collector: Some(CollectorParams::Keys(_) | CollectorParams::Named(_)),
                ..
            })
        )
    }

    /// Whether the callee is known to collect `&keys` or `&named`, so that the
    /// call checks its keyword arguments at run time.
    pub(crate) fn callee_collects_keywords(&self, callee: &Hir) -> bool {
        match &callee.kind {
            HirKind::Lambda {
                rest_param,
                vararg_kind,
                ..
            } => rest_param.is_some() && collects_keywords(vararg_kind),
            HirKind::Var(binding) => self.keyword_collectors.contains(binding),
            _ => false,
        }
    }
}

/// Whether a rest parameter of this kind collects keyword arguments.
fn collects_keywords(kind: &crate::hir::VarargKind) -> bool {
    matches!(
        kind,
        crate::hir::VarargKind::Struct | crate::hir::VarargKind::StrictStruct(_)
    )
}
