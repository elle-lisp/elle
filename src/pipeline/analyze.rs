//! Analysis pipeline: source -> HIR (no bytecode generation).

use super::cache;
use super::compile::classify_form;
use super::AnalyzeResult;
use crate::hir::Analyzer;
use crate::primitives::intern_primitive_names;
use crate::reader::{read_syntax, read_syntax_all};
use crate::symbol::SymbolTable;
use crate::syntax::Span;
use crate::vm::VM;

/// Analyze source code without generating bytecode.
/// Used by linter and LSP which need HIR but not bytecode.
pub fn analyze(
    source: &str,
    symbols: &mut SymbolTable,
    vm: &mut VM,
    source_name: &str,
) -> Result<AnalyzeResult, String> {
    let syntax = read_syntax(source, source_name)?;

    let (mut expander, meta) = cache::get_cached_expander_and_meta();

    let expanded = expander.expand(syntax, symbols, vm)?;
    let mut analyzer =
        Analyzer::new_with_primitives(symbols, meta.signals.clone(), meta.arities.clone());
    analyzer.bind_primitives(&meta);
    let analysis = analyzer.analyze(&expanded)?;
    Ok(AnalyzeResult { hir: analysis.hir })
}

/// Analyze a file as a single synthetic letrec (no bytecode).
///
/// Used by linter and LSP for file-level analysis. Primitives are
/// pre-bound as immutable Global bindings.
pub fn analyze_file(
    source: &str,
    symbols: &mut SymbolTable,
    vm: &mut VM,
    source_name: &str,
) -> Result<AnalyzeResult, String> {
    intern_primitive_names(symbols);

    let syntaxes = read_syntax_all(source, source_name)?;

    let (mut expander, meta) = cache::get_cached_expander_and_meta();

    // Expand all forms
    let mut expanded_forms = Vec::new();
    for syntax in syntaxes {
        let expanded = expander.expand(syntax, symbols, vm)?;
        expanded_forms.push(expanded);
    }

    // Classify each form
    let forms = expanded_forms.iter().map(classify_form).collect();

    // Compute span
    let span = if expanded_forms.is_empty() {
        Span::synthetic()
    } else {
        expanded_forms[0]
            .span
            .merge(&expanded_forms[expanded_forms.len() - 1].span)
    };

    // Analyze
    let mut analyzer =
        Analyzer::new_with_primitives(symbols, meta.signals.clone(), meta.arities.clone());
    analyzer.bind_primitives(&meta);
    let hir = analyzer.analyze_file_letrec(forms, span)?;

    Ok(AnalyzeResult { hir })
}
