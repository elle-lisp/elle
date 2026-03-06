//! Evaluation pipeline: source -> value.

use super::cache;
use super::compile::compile_file;
use crate::hir::tailcall::mark_tail_calls;
use crate::hir::Analyzer;
use crate::lir::{Emitter, Lowerer};
use crate::primitives::cached_primitive_meta;
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

    let meta = cached_primitive_meta(symbols);
    let mut analyzer = Analyzer::new_with_primitives(symbols, meta.effects, meta.arities);
    let mut analysis = analyzer.analyze(&expanded)?;
    mark_tail_calls(&mut analysis.hir);

    let intrinsics = crate::lir::intrinsics::build_intrinsics(symbols);
    let imm_prims = crate::lir::intrinsics::build_immediate_primitives(symbols);
    let mut lowerer = Lowerer::new()
        .with_intrinsics(intrinsics)
        .with_immediate_primitives(imm_prims);
    let lir_func = lowerer.lower(&analysis.hir)?;

    let symbol_snapshot = symbols.all_names();
    let mut emitter = Emitter::new_with_symbols(symbol_snapshot);
    let (bytecode, _yield_points, _call_sites) = emitter.emit(&lir_func);

    vm.execute(&bytecode).map_err(|e| e.to_string())
}

/// Compile and execute using the pipeline.
///
/// Shares the caller's VM for both macro expansion and execution.
pub fn eval(
    source: &str,
    symbols: &mut SymbolTable,
    vm: &mut VM,
) -> Result<crate::value::Value, String> {
    let syntax = read_syntax(source)?;

    // Get cached expander and meta (uses throwaway cache VM only for init)
    let (mut expander, meta) = cache::get_cached_expander_and_meta();

    let expanded = expander.expand(syntax, symbols, vm)?;

    let mut analyzer = Analyzer::new_with_primitives(symbols, meta.effects, meta.arities);
    let mut analysis = analyzer.analyze(&expanded)?;
    mark_tail_calls(&mut analysis.hir);

    let intrinsics = crate::lir::intrinsics::build_intrinsics(symbols);
    let imm_prims = crate::lir::intrinsics::build_immediate_primitives(symbols);
    let mut lowerer = Lowerer::new()
        .with_intrinsics(intrinsics)
        .with_immediate_primitives(imm_prims);
    let lir_func = lowerer.lower(&analysis.hir)?;

    let symbol_snapshot = symbols.all_names();
    let mut emitter = Emitter::new_with_symbols(symbol_snapshot);
    let (bytecode, _yield_points, _call_sites) = emitter.emit(&lir_func);

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
) -> Result<crate::value::Value, String> {
    let result = compile_file(source, symbols)?;
    vm.execute(&result.bytecode).map_err(|e| e.to_string())
}

/// Compile and execute a file as a single synthetic letrec.
///
/// Returns the value of the last expression. Primitives are pre-bound
/// as immutable Global bindings.
pub fn eval_file(
    source: &str,
    symbols: &mut SymbolTable,
    vm: &mut VM,
) -> Result<crate::value::Value, String> {
    let result = super::compile::compile_file(source, symbols)?;
    vm.execute(&result.bytecode).map_err(|e| e.to_string())
}
