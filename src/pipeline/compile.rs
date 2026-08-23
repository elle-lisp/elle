//! Compilation pipeline: source -> bytecode.

use super::CompileCtx;
use super::CompileResult;
use crate::hir::{classify_form, Analyzer, BindingArena, FileForm};
use crate::lir::{Emitter, Lowerer};
use crate::primitives::intern_primitive_names;
use crate::reader::{read_syntax, read_syntax_all_for};
use crate::symbol::SymbolTable;
use crate::syntax::{Span, Syntax, SyntaxKind};
use std::collections::HashSet;

mod frontend;
mod transforms;
pub use transforms::*;

// The shared front end lives in `frontend`; these are the entry points the
// per-backend lowering wrappers below reach for. `Frontend`/`FrontendResult`
// name the tuple those wrappers destructure.
use frontend::{
    compile_file_frontend, compile_file_frontend_xform, compile_syntaxes_frontend_xform, Frontend,
};

/// Run `f` under a fresh transient region on `heap`, freeing it when `f`
/// returns. `heap` is this compilation's own heap (`cctx.heap_ptr()` — the macro
/// VM's heap, which is the owning instance's when built with `new_with_heap`), so
/// the per-compilation scratch (anything bare-allocated during parse/expand/
/// analyze) lands in the same region store as the macro expander's allocations
/// and is reclaimed when the result is promoted out. The macro expander mints its
/// OWN per-expansion transient (`expand_macro_call`), so nothing the surviving
/// compile result references lives in this region.
fn with_transient<R>(heap: *mut crate::value::fiberheap::FiberHeap, f: impl FnOnce() -> R) -> R {
    let region = unsafe { (*heap).new_runtime_region() };
    let out = f();
    unsafe { (*heap).decref_region_if_present(region) };
    out
}

/// Compile source code to bytecode.
///
/// Creates an internal VM for macro expansion. Macro side effects
/// don't persist beyond compilation.
pub fn compile(
    source: &str,
    symbols: &mut SymbolTable,
    cctx: &mut CompileCtx,
    source_name: &str,
) -> Result<CompileResult, String> {
    with_transient(cctx.heap_ptr(), || {
        compile_inner(source, symbols, cctx, source_name)
    })
}

fn compile_inner(
    source: &str,
    symbols: &mut SymbolTable,
    cctx: &mut CompileCtx,
    source_name: &str,
) -> Result<CompileResult, String> {
    // Ensure caller's SymbolTable has primitive names interned so that
    // SymbolIds match the compile context's PrimitiveMeta.
    intern_primitive_names(symbols);

    // Phase 1: Parse to Syntax
    let syntax = read_syntax(source, source_name)?;

    // Phase 2: Macro expansion (the compile context's macro VM)
    let (expanded, meta, core_env) =
        cctx.with_macro_expansion(|macro_vm, mut expander, meta| {
            let expanded = expander.expand(syntax, symbols, macro_vm)?;
            Ok::<_, String>((expanded, meta, expander.core_env.clone()))
        })?;

    // Phase 3: Analyze to HIR with interprocedural signal and arity tracking
    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new_with_primitives(
        symbols,
        &mut arena,
        meta.signals.clone(),
        meta.arities.clone(),
    );
    analyzer.bind_primitives(&meta);
    if !core_env.is_empty() {
        analyzer.bind_compile_time_env(&core_env, true);
    }
    let mut analysis = analyzer.analyze(&expanded)?;
    let prim_values = analyzer.primitive_values().clone();
    drop(analyzer);

    // Phase 3.5: regularize the analyzed HIR — prune dead `(type-of x)` arms,
    // mark tail calls, functionalize, ANF-lift, type inference (crate::hir::regularize).
    let (dispatch_wrappers, fn_inline) = cctx.compile_registries_mut();
    crate::hir::regularize(
        &mut analysis.hir,
        &mut arena,
        symbols,
        dispatch_wrappers,
        fn_inline,
    )?;

    // Phase 4: Lower to LIR with intrinsic specialization
    let pc = crate::lir::intrinsics::PrimitiveClassification::new(symbols, &meta);
    let region_info =
        crate::hir::analyze_regions_with(&analysis.hir, &arena, pc.call_classification.clone());
    if crate::config::get().trace_bits() & crate::config::trace_bits::REGIONS != 0 {
        let names = symbols.all_names();
        eprintln!(
            "[trace:regions] compile:\n{}",
            crate::hir::format_regions(&region_info, &arena, &names)
        );
    }
    let symbol_names = symbols.all_names();
    let mut lowerer = Lowerer::new(&arena)
        .with_primitive_classification(pc)
        .with_primitive_values(prim_values)
        .with_symbol_names(symbol_names.clone())
        .with_region_info(region_info);
    let lir_module = lowerer.lower(&analysis.hir)?;

    // Phase 5: Emit bytecode with symbol names for cross-thread portability
    let mut emitter = Emitter::new_with_symbols(symbol_names);
    let (bytecode, _yield_points, _call_sites) = emitter.emit_module(&lir_module);

    Ok(CompileResult { bytecode })
}

/// Compile a file to LIR as a single synthetic letrec (for WASM backend).
///
/// `epoch_skip` — number of leading forms to exclude from epoch migration
/// (e.g. stdlib forms that are already in the current epoch). When 0,
/// epoch migration applies to all forms.
pub fn compile_file_to_lir(
    source: &str,
    symbols: &mut SymbolTable,
    cctx: &mut CompileCtx,
    source_name: &str,
    epoch_skip: usize,
) -> Result<crate::lir::LirModule, String> {
    with_transient(cctx.heap_ptr(), || {
        compile_file_to_lir_inner(source, symbols, cctx, source_name, epoch_skip)
    })
}

fn compile_file_to_lir_inner(
    source: &str,
    symbols: &mut SymbolTable,
    cctx: &mut CompileCtx,
    source_name: &str,
    epoch_skip: usize,
) -> Result<crate::lir::LirModule, String> {
    intern_primitive_names(symbols);

    let mut syntaxes = read_syntax_all_for(source, source_name)?;

    let source_epoch = crate::epoch::extract_epoch(&mut syntaxes)?;
    if let Some(epoch) = source_epoch {
        if epoch_skip > 0 && epoch_skip < syntaxes.len() {
            crate::epoch::migrate_forms(&mut syntaxes[epoch_skip..], epoch)?;
        } else {
            crate::epoch::migrate_forms(&mut syntaxes, epoch)?;
        }
    }

    // Expand all forms, splicing include/include-file inline
    let (expanded_forms, meta, core_env) =
        cctx.with_macro_expansion(|macro_vm, mut expander, meta| {
            let mut pending: std::collections::VecDeque<Syntax> = syntaxes.into();
            let mut expanded_forms = Vec::new();
            let mut included: HashSet<String> = HashSet::from([source_name.to_string()]);
            while let Some(syntax) = pending.pop_front() {
                if resolve_and_splice_include(&syntax, source_name, &mut pending, &mut included)? {
                    continue;
                }
                expanded_forms.push(expander.expand(syntax, symbols, macro_vm)?);
            }
            Ok::<_, String>((expanded_forms, meta, expander.core_env.clone()))
        })?;

    let forms: Vec<FileForm> = expanded_forms.iter().map(classify_form).collect();

    let span = if expanded_forms.is_empty() {
        Span::synthetic()
    } else {
        expanded_forms[0]
            .span
            .merge(&expanded_forms[expanded_forms.len() - 1].span)
    };

    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new_with_primitives(
        symbols,
        &mut arena,
        meta.signals.clone(),
        meta.arities.clone(),
    );
    analyzer.set_compile_ctx(cctx);
    let effective_epoch = source_epoch.unwrap_or(crate::epoch::CURRENT_EPOCH);
    analyzer.set_immutable_by_default(effective_epoch >= 8);
    analyzer.set_unicode_generation(cctx.unicode_generation());
    analyzer.bind_primitives(&meta);
    if !core_env.is_empty() {
        analyzer.bind_compile_time_env(&core_env, true);
    }
    let mut hir = analyzer.analyze_file_letrec(forms, span)?;
    let prim_values = analyzer.primitive_values().clone();
    let errors = analyzer.take_errors();
    drop(analyzer);

    // If analysis accumulated any recoverable errors (undefined vars,
    // signal mismatches, etc.), surface the first one now in the standard
    // "file:line:col: message" format. Without this, poison nodes from
    // accumulated errors reach the lowerer and surface as the opaque
    // "internal: error poison node in lowerer".
    if !errors.is_empty() {
        let err = &errors[0];
        let msg = match &err.location {
            Some(loc) => format!(
                "{}:{}:{}: {}",
                loc.file,
                loc.line,
                loc.col,
                err.description()
            ),
            None => err.description(),
        };
        return Err(msg);
    }

    let (dispatch_wrappers, fn_inline) = cctx.compile_registries_mut();
    crate::hir::regularize(&mut hir, &mut arena, symbols, dispatch_wrappers, fn_inline)?;

    let pc = crate::lir::intrinsics::PrimitiveClassification::new(symbols, cctx.primitive_meta());
    let region_info =
        crate::hir::analyze_regions_with(&hir, &arena, pc.call_classification.clone());
    if crate::config::get().trace_bits() & crate::config::trace_bits::REGIONS != 0 {
        let names = symbols.all_names();
        eprintln!(
            "[trace:regions] compile_file_to_lir:\n{}",
            crate::hir::format_regions(&region_info, &arena, &names)
        );
    }
    let symbol_names = symbols.all_names();
    let mut lowerer = Lowerer::new(&arena)
        .with_primitive_classification(pc)
        .with_primitive_values(prim_values)
        .with_symbol_names(symbol_names)
        .with_region_info(region_info);
    lowerer.lower(&hir)
}

/// Compile a file as a single synthetic letrec.
///
/// Compile to functionalized HIR (for `--dump=fhir`). Returns the HIR tree,
/// the binding arena, and the symbol name map.
pub fn compile_file_to_fhir(
    source: &str,
    symbols: &mut SymbolTable,
    cctx: &mut CompileCtx,
    source_name: &str,
) -> Result<
    (
        crate::hir::Hir,
        BindingArena,
        std::collections::HashMap<u32, String>,
    ),
    String,
> {
    let (hir, arena, _expander, _prim_values, _signal_projection) =
        compile_file_frontend(source, symbols, cctx, source_name)?;
    let names = symbols.all_names();
    Ok((hir, arena, names))
}

/// All top-level forms are analyzed together, enabling mutual recursion.
/// Returns a single `CompileResult`. Primitives are pre-bound as immutable
/// Global bindings in an outer scope.
pub fn compile_file(
    source: &str,
    symbols: &mut SymbolTable,
    cctx: &mut CompileCtx,
    source_name: &str,
) -> Result<CompileResult, String> {
    compile_file_inner(source, symbols, cctx, source_name).map(|(result, _)| result)
}

/// Like `compile_file`, but also returns the Expander after expansion.
/// The REPL uses this to persist macro definitions across inputs.
pub fn compile_file_repl(
    source: &str,
    symbols: &mut SymbolTable,
    cctx: &mut CompileCtx,
    source_name: &str,
) -> Result<(CompileResult, crate::syntax::Expander), String> {
    compile_file_inner(source, symbols, cctx, source_name)
}

fn compile_file_inner(
    source: &str,
    symbols: &mut SymbolTable,
    cctx: &mut CompileCtx,
    source_name: &str,
) -> Result<(CompileResult, crate::syntax::Expander), String> {
    let profile = std::env::var("ELLE_PROFILE").is_ok();
    let t0 = std::time::Instant::now();
    let mark = |label: &str| {
        if profile {
            eprintln!(
                "[elle-profile] compile {source_name} {label}: {:?}",
                t0.elapsed()
            );
        }
    };
    let (hir, arena, expander, prim_values, signal_projection) =
        compile_file_frontend(source, symbols, cctx, source_name)?;
    mark("frontend(parse+expand+analyze)");

    // Lower to LIR
    let pc = crate::lir::intrinsics::PrimitiveClassification::new(symbols, cctx.primitive_meta());
    let region_info =
        crate::hir::analyze_regions_with(&hir, &arena, pc.call_classification.clone());
    mark("regions");
    if crate::config::get().trace_bits() & crate::config::trace_bits::REGIONS != 0 {
        let names = symbols.all_names();
        eprintln!(
            "[trace:regions] compile_file:\n{}",
            crate::hir::format_regions(&region_info, &arena, &names)
        );
    }
    let symbol_names = symbols.all_names();
    let mut lowerer = Lowerer::new(&arena)
        .with_primitive_classification(pc)
        .with_primitive_values(prim_values)
        .with_symbol_names(symbol_names.clone())
        .with_region_info(region_info);

    let lir_module = lowerer.lower(&hir)?;
    mark("lower(HIR->LIR)");

    // Emit bytecode
    let signal = lir_module.entry.signal;
    let mut emitter = Emitter::new_with_symbols(symbol_names);
    let (mut bytecode, _, _) = emitter.emit_module(&lir_module);
    mark("emit(LIR->bytecode)");
    bytecode.signal = signal;
    bytecode.signal_projection = signal_projection;

    Ok((CompileResult { bytecode }, expander))
}

/// Compile a file in the per-form fault-barrier test mode
/// (docs/test-runner.md § Mechanism → "How the barrier is realized").
///
/// The file is compiled ONCE through the whole-module `analyze_file_letrec`
/// path (so binding resolution, capture analysis, signal inference, and epoch
/// migration all apply across the file). The returned module, when executed,
/// evaluates the file's `def`/`var` forms eagerly — establishing the shared
/// bindings every test form can see (including forward references) — and returns
/// a mutable array of `[index thunk]` pairs, one 0-arg thunk per test
/// (expression) form, each capturing that shared environment.
///
/// The runner then runs each thunk on each tier with a fault barrier OUTSIDE the
/// tiered closure (a worker fiber + `protect`); see `src/test.lisp`.
pub fn compile_barrier_module(
    source: &str,
    symbols: &mut SymbolTable,
    cctx: &mut CompileCtx,
    source_name: &str,
) -> Result<CompileResult, String> {
    compile_module_with_transform(source, symbols, cctx, source_name, barrier_transform)
}

/// Shared body for the test compilation modes (`compile_barrier_module`,
/// `compile_whole_module`): run the file front end with `xform` applied to the
/// expanded forms, then lower and emit to bytecode. The two modes differ only in
/// the transform (per-form thunks vs one whole-file thunk).
fn compile_module_with_transform(
    source: &str,
    symbols: &mut SymbolTable,
    cctx: &mut CompileCtx,
    source_name: &str,
    xform: impl FnOnce(Vec<Syntax>, crate::syntax::ScopeId) -> Vec<Syntax>,
) -> Result<CompileResult, String> {
    let frontend = compile_file_frontend_xform(source, symbols, cctx, source_name, xform)?;
    lower_test_frontend(frontend, symbols, cctx)
}

/// Like `compile_module_with_transform`, but from already-read top-level
/// `syntaxes` (parse once in one VM, compile in another). Powers
/// `compile/whole-module-syntax` — the runner reads a legacy multi-form file in
/// the main VM and ships its syntax to a worker that compiles it here.
fn compile_syntaxes_with_transform(
    syntaxes: Vec<Syntax>,
    symbols: &mut SymbolTable,
    cctx: &mut CompileCtx,
    source_name: &str,
    xform: impl FnOnce(Vec<Syntax>, crate::syntax::ScopeId) -> Vec<Syntax>,
) -> Result<CompileResult, String> {
    let frontend = compile_syntaxes_frontend_xform(syntaxes, symbols, cctx, source_name, xform)?;
    lower_test_frontend(frontend, symbols, cctx)
}

/// Lower + emit a test-mode front-end result to bytecode. Shared by the source
/// and syntax entry points so they differ only in how the front end is fed.
fn lower_test_frontend(
    frontend: Frontend,
    symbols: &mut SymbolTable,
    cctx: &mut CompileCtx,
) -> Result<CompileResult, String> {
    let (hir, arena, _expander, prim_values, signal_projection) = frontend;

    let pc = crate::lir::intrinsics::PrimitiveClassification::new(symbols, cctx.primitive_meta());
    let region_info =
        crate::hir::analyze_regions_with(&hir, &arena, pc.call_classification.clone());
    let symbol_names = symbols.all_names();
    let mut lowerer = Lowerer::new(&arena)
        .with_primitive_classification(pc)
        .with_primitive_values(prim_values)
        .with_symbol_names(symbol_names.clone())
        .with_region_info(region_info);
    let lir_module = lowerer.lower(&hir)?;

    let signal = lir_module.entry.signal;
    let mut emitter = Emitter::new_with_symbols(symbol_names);
    let (mut bytecode, _, _) = emitter.emit_module(&lir_module);
    bytecode.signal = signal;
    bytecode.signal_projection = signal_projection;

    Ok(CompileResult { bytecode })
}
