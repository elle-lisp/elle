// audited: 2026-10-21
//! The signal fixpoint: re-analyze lambda initializers until the signals they read stop changing.
//!
//! docs/pipeline.md

use super::*;
use crate::syntax::Syntax;

/// The most passes the fixpoint makes. Each pass can only move a signal up the
/// lattice, so the loop ends on lattice height; the cap guards against a
/// lattice bug.
const MAX_FIXPOINT_PASSES: usize = 10;

impl<'a> Analyzer<'a> {
    /// Re-analyze each lambda initializer in `entries` against the signals
    /// `signal_env` holds, recording each signal that moved, until a pass moves
    /// none. Answers each initializer's analysis from the last pass, in the
    /// order of `entries`.
    ///
    /// The caller analyzed each entry once already, and recorded its signal.
    /// A pass reports again the errors that analysis reported, so the loop
    /// keeps the last pass's errors and drops each one already reported.
    pub(super) fn converge_signals(
        &mut self,
        entries: &[(Binding, &Syntax)],
    ) -> Result<Vec<Hir>, String> {
        let reported = std::mem::take(&mut self.errors);
        let mut values = Vec::with_capacity(entries.len());
        for _ in 0..MAX_FIXPOINT_PASSES {
            self.errors.clear();
            values.clear();
            let mut moved = false;
            for &(binding, value_syntax) in entries {
                let old = self
                    .signal_env
                    .get(&binding)
                    .copied()
                    .unwrap_or_else(Signal::silent);
                // Re-analysis rebuilds the capture set, so it goes through
                // `analyze_initializer`, which classifies a self-edge as
                // `Recursive`.
                let value = self.analyze_initializer(binding, value_syntax)?;
                if let HirKind::Lambda(lambda) = &value.kind {
                    if lambda.inferred_signals != old {
                        self.signal_env.insert(binding, lambda.inferred_signals);
                        moved = true;
                    }
                }
                values.push(value);
            }
            if !moved {
                break;
            }
        }
        let last_pass = std::mem::replace(&mut self.errors, reported);
        for error in last_pass {
            let repeated = self
                .errors
                .iter()
                .any(|seen| seen.kind == error.kind && seen.location == error.location);
            if !repeated {
                self.errors.push(error);
            }
        }
        Ok(values)
    }

    /// Whether the analyzed initializer `value` of `binding` is a lambda that
    /// refers to `binding`. A self-reference crosses the lambda's boundary, so
    /// it is one of the lambda's captures, whether the lambda makes it directly
    /// or a lambda nested inside it does.
    pub(super) fn refers_to_itself(binding: Binding, value: &Hir) -> bool {
        matches!(&value.kind, HirKind::Lambda(lambda)
            if lambda.captures.iter().any(|capture| capture.binding == binding))
    }
}
