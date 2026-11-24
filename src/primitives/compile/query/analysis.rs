// audited: 2026-11-24
//! `compile/analyze`, which builds the analysis handle, and `compile/diagnostics` and `compile/symbols`, which read it.
//!
//! docs/analysis/portrait.md

use std::collections::{HashMap, HashSet};

use crate::hir::symbols::extract_symbols_from_hir;
use crate::pipeline::analyze_file;
use crate::primitives::compile::{
    build_binding_spans, build_call_graph, build_signal_map, compile_parts, diagnostic_to_value,
    get_handle, kw, symbol_def_to_value, AnalysisHandle, CompiledCell,
};
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::sorted_struct_get;
use crate::value::Value;

/// `(compile/analyze source [opts])` → analysis handle
pub(in crate::primitives::compile) fn prim_compile_analyze(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let source = match args[0].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                ctx.error("type-error", "compile/analyze: expected string source"),
            )
        }
    };

    // Optional opts struct for :file key.
    let file_name = if args.len() == 2 {
        if let Some(fields) = args[1].as_struct() {
            sorted_struct_get(fields, &kw("file"))
                .and_then(|v| v.with_string(|s| s.to_string()))
                .unwrap_or_else(|| "<analyze>".to_string())
        } else {
            "<analyze>".to_string()
        }
    } else {
        "<analyze>".to_string()
    };

    // The driving VM hosts macro expansion during analysis.
    let (symbols, vm, cctx) = match compile_parts("compile/analyze", ctx) {
        Ok(parts) => parts,
        Err(e) => return e,
    };

    // Run analysis.
    let result = match analyze_file(&source, symbols, vm, cctx, &file_name) {
        Ok(r) => r,
        Err(e) => return (SIG_ERROR, ctx.error("compile-error", e)),
    };

    // Extract symbols and diagnostics.
    let symbol_index = extract_symbols_from_hir(&result.hir, symbols, &result.arena);
    let mut diagnostics = crate::lint::lint_analysis(&result, symbols);

    // Convert accumulated analysis errors to diagnostics
    for err in &result.errors {
        let crate::lint::diagnostics::LintCode { code, rule } =
            crate::lint::diagnostics::LintCode::for_error_kind(&err.kind);
        let loc = err
            .location
            .clone()
            .unwrap_or_else(|| crate::reader::SourceLoc::new(&file_name, 0, 0));
        diagnostics.push(crate::lint::diagnostics::Diagnostic::new(
            crate::lint::diagnostics::Severity::Error,
            code,
            rule,
            err.description(),
            Some(loc),
        ));
    }

    // Build signal map, call graph, and binding spans.
    let signal_map = build_signal_map(&result.hir, &result.arena, symbols);
    let call_graph = build_call_graph(
        &result.hir,
        &result.arena,
        symbols,
        &signal_map,
        &result.fixed,
        &result.written,
    );

    let mut binding_spans = HashMap::new();
    build_binding_spans(
        &result.hir,
        &result.arena,
        symbols,
        &source,
        &symbol_index,
        &mut binding_spans,
    );

    let handle = AnalysisHandle {
        hir: result.hir,
        arena: result.arena,
        symbol_index,
        diagnostics,
        signal_map,
        call_graph,
        source: source.clone(),
        file_name,
        binding_spans,
        compiled: CompiledCell::default(),
    };

    (SIG_OK, ctx.external("analysis", handle))
}

/// (compile/diagnostics analysis) → [{:severity :warning :code "..." ...}]
pub(in crate::primitives::compile) fn prim_compile_diagnostics(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let handle = match get_handle(args, "compile/diagnostics", ctx) {
        Ok(h) => h,
        Err(e) => return e,
    };
    let values: Vec<Value> = handle
        .diagnostics
        .iter()
        .map(|x| diagnostic_to_value(x, ctx))
        .collect();
    (SIG_OK, ctx.array(values))
}

/// (compile/symbols analysis) → [{:name "f" :kind :function ...}]
pub(in crate::primitives::compile) fn prim_compile_symbols(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let handle = match get_handle(args, "compile/symbols", ctx) {
        Ok(h) => h,
        Err(e) => return e,
    };
    // Only real in-file definitions (those with a source location). Usage-only
    // placeholder entries for primitives carry no location and are not symbols
    // the user defined here. Sorted by name: the index is a hash map, and a
    // consumer that diffs two analyses needs one order. A macro that places a
    // body more than once binds its names once per copy, at one place, so one
    // place answers once.
    let mut defs: Vec<_> = handle
        .symbol_index
        .definitions
        .values()
        .filter_map(|d| d.location.as_ref().map(|loc| (d, (loc.line, loc.col))))
        .collect();
    defs.sort_by(|(a, at), (b, bt)| a.name.cmp(&b.name).then(at.cmp(bt)));
    let mut seen = HashSet::new();
    let values: Vec<Value> = defs
        .into_iter()
        .filter(|(d, at)| seen.insert((d.name.clone(), *at)))
        .map(|(x, _)| symbol_def_to_value(x, ctx))
        .collect();
    (SIG_OK, ctx.array(values))
}
