// audited: 2026-11-24
//! Linting infrastructure for Elle: diagnostic types, the rules, and the CLI wrapper.
//!
//! docs/cookbook/lint-rules.md

pub mod cli;
pub mod diagnostics;
pub mod rules;
pub mod run;

pub use cli::{LintConfig, Linter, OutputFormat};
pub use diagnostics::{Diagnostic, Severity};

/// Every lint finding of an analysis: the `HirLinter`'s, then the expander's.
///
/// The lint CLI, `compile/analyze` and the LSP each read a file's findings
/// here. A macro that places a body twice expands a macro inside it twice,
/// so the expander's findings keep one for each rule, message and location,
/// as the `HirLinter`'s do.
pub fn lint_analysis(
    analysis: &crate::pipeline::AnalyzeResult,
    symbols: &crate::symbol::SymbolTable,
) -> Vec<Diagnostic> {
    let mut linter = crate::hir::HirLinter::new();
    linter.lint(&analysis.hir, symbols, &analysis.arena);
    let mut found = linter.diagnostics().to_vec();
    let mut seen = std::collections::HashSet::new();
    for d in &analysis.expansion {
        let at = d.location.as_ref().map(|l| (l.line, l.col));
        if seen.insert((d.rule.clone(), d.message.clone(), at)) {
            found.push(d.clone());
        }
    }
    found
}
