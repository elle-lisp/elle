//! `compile/analyze` and the simple accessors that read straight off a handle
//! (diagnostics, symbols). These build or unpack the `AnalysisHandle`.
use std::collections::HashMap;

use crate::hir::symbols::extract_symbols_from_hir;
use crate::hir::HirLinter;
use crate::pipeline::analyze_file;
use crate::primitives::compile::{
    build_binding_spans, build_call_graph, build_signal_map, diagnostic_to_value, get_handle, kw,
    symbol_def_to_value, AnalysisHandle,
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

    // The driving VM hosts macro expansion during analysis. `ctx.vm()` is total
    // (a native always runs under a VM); read it as a raw pointer so it sits beside
    // the disjoint symbol-table and compile-context borrows below.
    let vm_ptr: *mut crate::vm::VM = ctx.vm();

    // The symbol table and compile context are disjoint siblings of the VM in the
    // owning `RuntimeCore`; read their pointers through the VM (this instance's
    // own) so all three become separate `&mut`.
    let symbols_ptr = unsafe { (*vm_ptr).symbols_ptr };
    if symbols_ptr.is_null() {
        return (
            SIG_ERROR,
            ctx.error(
                "runtime-error",
                "compile/analyze: no symbol table in context",
            ),
        );
    }
    let cctx_ptr = unsafe { (*vm_ptr).compile_ctx_ptr };
    if cctx_ptr.is_null() {
        return (
            SIG_ERROR,
            ctx.error("runtime-error", "compile/analyze: no compile context"),
        );
    }
    let (symbols, vm, cctx) = unsafe { (&mut *symbols_ptr, &mut *vm_ptr, &mut *cctx_ptr) };

    // Run analysis.
    let result = match analyze_file(&source, symbols, vm, cctx, &file_name) {
        Ok(r) => r,
        Err(e) => return (SIG_ERROR, ctx.error("compile-error", e)),
    };

    // Extract symbols and diagnostics.
    let symbol_index = extract_symbols_from_hir(&result.hir, symbols, &result.arena);
    let mut linter = HirLinter::new();
    linter.lint(&result.hir, symbols, &result.arena);
    let mut diagnostics = linter.diagnostics().to_vec();

    // Convert accumulated analysis errors to diagnostics
    for err in &result.errors {
        use crate::error::ErrorKind;
        let (code, rule) = match &err.kind {
            ErrorKind::UndefinedVariable { .. } => ("E001", "undefined-variable"),
            ErrorKind::SignalMismatch { .. } => ("E002", "signal-mismatch"),
            ErrorKind::UnterminatedForm { .. } => ("E003", "unterminated-form"),
            ErrorKind::CompileError { .. } => ("E004", "compile-error"),
            _ => ("E000", "analysis-error"),
        };
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
    let call_graph = build_call_graph(&result.hir, &result.arena, symbols, &signal_map);

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
        binding_spans,
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
    // the user defined here.
    let values: Vec<Value> = handle
        .symbol_index
        .definitions
        .values()
        .filter(|d| d.location.is_some())
        .map(|x| symbol_def_to_value(x, ctx))
        .collect();
    (SIG_OK, ctx.array(values))
}
