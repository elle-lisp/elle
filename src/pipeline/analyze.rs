//! Analysis pipeline: source -> HIR (no bytecode generation).

use super::AnalyzeResult;
use super::CompileCtx;
use crate::hir::{classify_form, Analyzer, BindingArena};
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
    cctx: &mut CompileCtx,
    source_name: &str,
) -> Result<AnalyzeResult, String> {
    // One working arena per analysis, freed when the HIR is built: the result
    // carries spans, not nodes, so nothing in it points here.
    let arena = unsafe { crate::syntax::SyntaxArena::mint(&mut *cctx.heap_ptr()) };
    let out = analyze_in_arena(arena, source, symbols, vm, cctx, source_name);
    unsafe { (*cctx.heap_ptr()).decref_region_if_present(arena.region()) };
    out
}

fn analyze_in_arena(
    arena: crate::syntax::SyntaxArena,
    source: &str,
    symbols: &mut SymbolTable,
    vm: &mut VM,
    cctx: &mut CompileCtx,
    source_name: &str,
) -> Result<AnalyzeResult, String> {
    let syntax = read_syntax(arena, source, source_name)?;

    let (mut expander, meta) = cctx.expander_and_meta(arena);

    let expanded = expander.expand(syntax, symbols, vm)?;
    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new_with_primitives(
        symbols,
        &mut arena,
        meta.signals.clone(),
        meta.arities.clone(),
    );
    analyzer.set_compile_ctx(cctx);
    analyzer.bind_primitives(&meta);
    let analysis = analyzer.analyze(&expanded)?;
    let errors = analysis.errors;
    drop(analyzer);
    let mut hir = analysis.hir;
    crate::hir::tailcall::mark_tail_calls(&mut hir);
    Ok(AnalyzeResult { hir, arena, errors })
}

/// Analyze a file as a single synthetic letrec (no bytecode).
///
/// Used by linter and LSP for file-level analysis. Primitives are
/// pre-bound as immutable Global bindings.
pub fn analyze_file(
    source: &str,
    symbols: &mut SymbolTable,
    vm: &mut VM,
    cctx: &mut CompileCtx,
    source_name: &str,
) -> Result<AnalyzeResult, String> {
    let arena = unsafe { crate::syntax::SyntaxArena::mint(&mut *cctx.heap_ptr()) };
    let out = analyze_file_in_arena(arena, source, symbols, vm, cctx, source_name);
    unsafe { (*cctx.heap_ptr()).decref_region_if_present(arena.region()) };
    out
}

fn analyze_file_in_arena(
    arena: crate::syntax::SyntaxArena,
    source: &str,
    symbols: &mut SymbolTable,
    vm: &mut VM,
    cctx: &mut CompileCtx,
    source_name: &str,
) -> Result<AnalyzeResult, String> {
    let syntaxes = read_syntax_all(arena, source, source_name)?;

    let (mut expander, meta) = cctx.expander_and_meta(arena);

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
    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new_with_primitives(
        symbols,
        &mut arena,
        meta.signals.clone(),
        meta.arities.clone(),
    );
    analyzer.set_compile_ctx(cctx);
    analyzer.bind_primitives(&meta);
    let mut hir = analyzer.analyze_file_letrec(forms, span)?;
    let errors = analyzer.take_errors();
    drop(analyzer);

    crate::hir::tailcall::mark_tail_calls(&mut hir);
    Ok(AnalyzeResult { hir, arena, errors })
}
