// audited: 2026-10-13
//! The entry points that compile source or a syntax tree and run it on the caller's VM.
//!
//! docs/pipeline.md

use super::compile::compile_file;
use super::CompileCtx;
use crate::hir::functionalize::functionalize;
use crate::hir::tailcall::mark_tail_calls;
use crate::hir::{Analyzer, BindingArena};
use crate::lir::{Emitter, Lowerer};
use crate::reader::read_syntax;
use crate::symbol::SymbolTable;
use crate::syntax::{Expander, UnitTag};
use crate::vm::VM;

/// Compile and run a syntax tree with the caller's expander: a macro
/// transformer, or a `begin-for-syntax` value. The expander is the one mid
/// expansion, so a macro call inside the tree expands in the same context.
/// The tree is not regularized: no graft, no fold, and no type information
/// reaches the lowerer. The body runs under the expansion fuel budget
/// (`VM::run_at_expansion`).
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
        .with_heap(unsafe { &mut *vm.heap_ptr })
        .with_primitive_classification(pc)
        .with_primitive_values(prim_values)
        .with_region_info(region_info);
    let lir_module = lowerer.lower(&analysis.hir)?;

    // The transformer body runs on this VM, so its code lands on this VM's
    // heap (docs/impl/region/template.md).
    let code = crate::value::CodeArena::mint(vm.heap());
    let mut emitter = Emitter::new(code);
    let bytecode = emitter.emit_module(&lir_module).bytecode;

    // The body runs under the expansion fuel budget, and a park it raises is
    // refused: an expansion cannot hold one. A gensym it calls counts in the
    // expander's unit.
    vm.run_at_expansion(
        &crate::value::CodeUnit::new(code, bytecode),
        expander.unit_mut(),
    )
    .map_err(|fault| fault.describe())
}

/// Compile one form and run it. The caller's VM both expands the form and runs
/// it. Like `eval_syntax`, it does not regularize.
pub fn eval(
    source: &str,
    symbols: &mut SymbolTable,
    vm: &mut VM,
    cctx: &mut CompileCtx,
    source_name: &str,
) -> Result<crate::value::Value, String> {
    let arena = unsafe { crate::syntax::SyntaxArena::mint(&mut *cctx.heap_ptr()) };
    let out = eval_in_arena(arena, source, symbols, vm, cctx, source_name);
    unsafe { (*cctx.heap_ptr()).decref_region_if_present(arena.region()) };
    out
}

fn eval_in_arena(
    arena: crate::syntax::SyntaxArena,
    source: &str,
    symbols: &mut SymbolTable,
    vm: &mut VM,
    cctx: &mut CompileCtx,
    source_name: &str,
) -> Result<crate::value::Value, String> {
    let syntax = read_syntax(arena, source, source_name)?;

    // The instance's expander + compile meta; expansion runs on the caller's vm.
    let unit = UnitTag::of(source_name, std::slice::from_ref(&syntax));
    let (mut expander, meta) = cctx.expander_and_meta(arena, unit);

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
        .with_heap(unsafe { &mut *cctx.heap_ptr() })
        .with_primitive_classification(pc)
        .with_primitive_values(prim_values)
        .with_region_info(region_info);
    let lir_module = lowerer.lower(&analysis.hir)?;

    // The form runs on `vm`, whatever heap the compile context expands on,
    // so its code lands on `vm`'s heap (docs/impl/region/template.md).
    let code = crate::value::CodeArena::mint(vm.heap());
    let mut emitter = Emitter::new(code);
    let bytecode = emitter.emit_module(&lir_module).bytecode;

    vm.execute(&crate::value::CodeUnit::new(code, bytecode))
        .map_err(|e| e.to_string())
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
    // Run inside `ev/run`, as the binary's `run_source` does, so a primitive
    // that cooperates with the scheduler works in a test. With no stdlib loaded
    // there is no `ev/run`, and `execute_scheduled` runs the bytecode directly.
    vm.execute_scheduled(&result, cctx)
        .map_err(|e| e.to_string())
}

/// Compile and execute a file as a single synthetic letrec.
///
/// Returns the value of the last expression.
pub fn eval_file(
    source: &str,
    symbols: &mut SymbolTable,
    vm: &mut VM,
    cctx: &mut CompileCtx,
    source_name: &str,
) -> Result<crate::value::Value, String> {
    let result = super::compile::compile_file(source, symbols, cctx, source_name)?;
    vm.execute(&result).map_err(|e| e.to_string())
}
