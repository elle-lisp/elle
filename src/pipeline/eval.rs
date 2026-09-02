//! Evaluation pipeline: source -> value.

use super::compile::compile_file;
use super::CompileCtx;
use crate::hir::functionalize::functionalize;
use crate::hir::tailcall::mark_tail_calls;
use crate::hir::{Analyzer, BindingArena};
use crate::lir::{Emitter, Lowerer};
use crate::reader::read_syntax;
use crate::symbol::SymbolTable;
use crate::syntax::Expander;
use crate::vm::VM;

/// Compile and execute a Syntax tree, reusing the caller's Expander.
///
/// This is the entry point for macro body evaluation: the Expander builds
/// a let-expression wrapping the macro body, then calls this to compile
/// and run it in the VM. The same Expander is threaded through so nested
/// macro calls work.
pub fn eval_syntax(
    syntax: crate::syntax::Syntax,
    expander: &mut Expander,
    symbols: &mut SymbolTable,
    vm: &mut VM,
) -> Result<crate::value::Value, String> {
    let expanded = expander.expand(syntax, symbols, vm)?;

    // The macro-body metadata (primitives + stdlib) rides on the expander, so
    // no separate `CompileCtx` borrow is needed mid-expansion.
    let meta = expander.eval_meta().clone();
    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new_with_primitives(
        symbols,
        &mut arena,
        meta.signals.clone(),
        meta.arities.clone(),
    );
    analyzer.bind_primitives(&meta);
    // Make core.lisp exports and compile-time defs visible in macro bodies.
    if !expander.core_env.is_empty() {
        analyzer.bind_compile_time_env(&expander.core_env, true);
    }
    if !expander.compile_time_env.is_empty() {
        analyzer.bind_compile_time_env(&expander.compile_time_env, false);
    }
    let mut analysis = analyzer.analyze(&expanded)?;
    if !analysis.errors.is_empty() {
        return Err(analysis.errors[0].description());
    }
    mark_tail_calls(&mut analysis.hir);
    let prim_values = analyzer.primitive_values().clone();
    drop(analyzer);
    functionalize(&mut analysis.hir, &mut arena);
    crate::hir::anf::anf_lift(&mut analysis.hir, &mut arena);
    let pc = crate::lir::intrinsics::PrimitiveClassification::new(&meta);
    let region_info =
        crate::hir::analyze_regions_with(&analysis.hir, &arena, pc.call_classification.clone());
    if crate::config::get().trace_bits() & crate::config::trace_bits::REGIONS != 0 {
        eprintln!(
            "[trace:regions] eval_syntax:\n{}",
            crate::hir::format_regions(&region_info, &arena, Some(symbols))
        );
    }
    let mut lowerer = Lowerer::new(&arena)
        .with_primitive_classification(pc)
        .with_primitive_values(prim_values)
        .with_region_info(region_info);
    let lir_module = lowerer.lower(&analysis.hir)?;

    let mut emitter = Emitter::new();
    let (bytecode, _yield_points, _call_sites) = emitter.emit_module(&lir_module);

    vm.execute(&bytecode).map_err(|e| e.to_string())
}

/// Compile and execute using the pipeline.
///
/// Shares the caller's VM for both macro expansion and execution.
pub fn eval(
    source: &str,
    symbols: &mut SymbolTable,
    vm: &mut VM,
    cctx: &mut CompileCtx,
    source_name: &str,
) -> Result<crate::value::Value, String> {
    let syntax = read_syntax(source, source_name)?;

    // The instance's expander + compile meta; expansion runs on the caller's vm.
    let (mut expander, meta) = cctx.expander_and_meta();

    let scoped = if source_name.starts_with('<') {
        syntax
    } else {
        expander.stamp_file_scope(syntax)
    };
    let expanded = expander.expand(scoped, symbols, vm)?;

    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new_with_primitives(
        symbols,
        &mut arena,
        meta.signals.clone(),
        meta.arities.clone(),
    );
    analyzer.set_compile_ctx(cctx);
    analyzer.bind_primitives(&meta);
    if !expander.core_env.is_empty() {
        analyzer.bind_compile_time_env(&expander.core_env, true);
    }
    let mut analysis = analyzer.analyze(&expanded)?;
    if !analysis.errors.is_empty() {
        return Err(analysis.errors[0].description());
    }
    mark_tail_calls(&mut analysis.hir);
    let prim_values = analyzer.primitive_values().clone();
    drop(analyzer);
    functionalize(&mut analysis.hir, &mut arena);
    crate::hir::anf::anf_lift(&mut analysis.hir, &mut arena);
    let pc = crate::lir::intrinsics::PrimitiveClassification::new(&meta);
    let region_info =
        crate::hir::analyze_regions_with(&analysis.hir, &arena, pc.call_classification.clone());
    if crate::config::get().trace_bits() & crate::config::trace_bits::REGIONS != 0 {
        eprintln!(
            "[trace:regions] eval:\n{}",
            crate::hir::format_regions(&region_info, &arena, Some(symbols))
        );
    }
    let mut lowerer = Lowerer::new(&arena)
        .with_primitive_classification(pc)
        .with_primitive_values(prim_values)
        .with_region_info(region_info);
    let lir_module = lowerer.lower(&analysis.hir)?;

    let mut emitter = Emitter::new();
    let (bytecode, _yield_points, _call_sites) = emitter.emit_module(&lir_module);

    vm.execute(&bytecode).map_err(|e| e.to_string())
}

/// Compile and execute multiple top-level forms.
///
/// All forms are compiled as a single synthetic letrec (via `compile_file`)
/// then executed as one unit. Returns the value of the last form.
/// Returns `Ok(Value::NIL)` for empty input.
pub fn eval_all(
    source: &str,
    symbols: &mut SymbolTable,
    vm: &mut VM,
    cctx: &mut CompileCtx,
    source_name: &str,
) -> Result<crate::value::Value, String> {
    let result = compile_file(source, symbols, cctx, source_name)?;
    // Run under the async scheduler, exactly as the binary's `run_source`
    // does — top-level Elle always executes inside `ev/run`. This is what
    // lets scheduler-cooperative primitives (e.g. `sys/join`'s deadline via
    // `chan/select`) work in the test harness. `execute_scheduled` falls back
    // to a plain `execute` when `ev/run` is absent (no stdlib loaded), so
    // bare-VM callers are unaffected.
    vm.execute_scheduled(&result.bytecode, cctx)
        .map_err(|e| e.to_string())
}

/// Compile and execute a file as a single synthetic letrec.
///
/// Returns the value of the last expression. Primitives are pre-bound
/// as immutable Global bindings.
pub fn eval_file(
    source: &str,
    symbols: &mut SymbolTable,
    vm: &mut VM,
    cctx: &mut CompileCtx,
    source_name: &str,
) -> Result<crate::value::Value, String> {
    let result = super::compile::compile_file(source, symbols, cctx, source_name)?;
    vm.execute(&result.bytecode).map_err(|e| e.to_string())
}
