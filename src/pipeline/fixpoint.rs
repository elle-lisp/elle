//! Fixpoint effect-inference loop for multi-form compilation.

use crate::effects::Effect;
use crate::hir::{AnalysisResult, Analyzer};
use crate::primitives::def::PrimitiveMeta;
use crate::symbol::SymbolTable;
use crate::syntax::Syntax;
use crate::value::types::Arity;
use crate::value::SymbolId;
use std::collections::{HashMap, HashSet};

/// Maximum iterations for fixpoint effect inference loop.
const MAX_ITERATIONS: usize = 10;

/// Run the fixpoint effect-inference loop over expanded forms.
///
/// Iteratively analyzes all forms until `global_effects` converge (max 10
/// iterations). Each iteration creates a fresh Analyzer per form, seeds it
/// with the current global state, and collects newly-defined effects,
/// arities, and immutable globals.
///
/// `post_analyze` is called on each `AnalysisResult` after analysis.
/// `compile_all` passes `|a| mark_tail_calls(&mut a.hir)`;
/// `analyze_all` passes `|_| {}`.
///
/// Returns the final `Vec<AnalysisResult>` after convergence.
pub(super) fn run_fixpoint(
    expanded_forms: &[Syntax],
    symbols: &mut SymbolTable,
    meta: &PrimitiveMeta,
    mut global_effects: HashMap<SymbolId, Effect>,
    mut global_arities: HashMap<SymbolId, Arity>,
    mut immutable_globals: HashSet<SymbolId>,
    mut post_analyze: impl FnMut(&mut AnalysisResult),
) -> Result<Vec<AnalysisResult>, String> {
    let mut analysis_results: Vec<AnalysisResult> = Vec::new();

    for _iteration in 0..MAX_ITERATIONS {
        analysis_results.clear();
        let mut new_global_effects: HashMap<SymbolId, Effect> = HashMap::new();

        for form in expanded_forms {
            let mut analyzer =
                Analyzer::new_with_primitives(symbols, meta.effects.clone(), meta.arities.clone());
            // Seed with current global effects (from pre-scan or previous iteration)
            analyzer.set_global_effects(global_effects.clone());
            // Seed with global arities from pre-scan and previous forms
            analyzer.set_global_arities(global_arities.clone());
            // Seed with immutable globals from pre-scan
            analyzer.set_immutable_globals(immutable_globals.clone());

            let mut analysis = analyzer.analyze(form)?;

            // Collect effects and arities from this form's defines
            for (sym, effect) in analyzer.take_defined_global_effects() {
                new_global_effects.insert(sym, effect);
            }
            for (sym, arity) in analyzer.take_defined_global_arities() {
                global_arities.insert(sym, arity);
            }

            // Merge defined immutable globals from this form
            for sym in analyzer.take_defined_immutable_globals() {
                immutable_globals.insert(sym);
            }

            post_analyze(&mut analysis);
            analysis_results.push(analysis);
        }

        // Check for convergence: did any effect change?
        if new_global_effects == global_effects {
            break; // Stable -- we're done
        }

        // Effects changed -- update and re-analyze
        global_effects = new_global_effects;
    }

    Ok(analysis_results)
}
