//! Compilation pipeline: source -> bytecode.

use super::cache;
use super::fixpoint;
use super::scan;
use super::CompileResult;
use crate::hir::tailcall::mark_tail_calls;
use crate::hir::Analyzer;
use crate::lir::{Emitter, Lowerer};
use crate::primitives::intern_primitive_names;
use crate::reader::{read_syntax, read_syntax_all};
use crate::symbol::SymbolTable;

/// Compile source code to bytecode.
///
/// Creates an internal VM for macro expansion. Macro side effects
/// don't persist beyond compilation.
pub fn compile(source: &str, symbols: &mut SymbolTable) -> Result<CompileResult, String> {
    // Ensure caller's SymbolTable has primitive names interned so that
    // SymbolIds match the cached PrimitiveMeta.
    intern_primitive_names(symbols);

    // Phase 1: Parse to Syntax
    let syntax = read_syntax(source)?;

    // Phase 2: Macro expansion (cached VM for macro bodies)
    let (macro_vm_ptr, mut expander, meta) = cache::get_compilation_cache();
    // SAFETY: The cached VM is thread-local and pipeline functions are not
    // re-entrant. The RefCell borrow was released by get_compilation_cache.
    let macro_vm = unsafe { &mut *macro_vm_ptr };
    let expanded = expander.expand(syntax, symbols, macro_vm)?;

    // Phase 3: Analyze to HIR with interprocedural effect and arity tracking
    let mut analyzer = Analyzer::new_with_primitives(symbols, meta.effects, meta.arities);
    let mut analysis = analyzer.analyze(&expanded)?;

    // Phase 3.5: Mark tail calls
    mark_tail_calls(&mut analysis.hir);

    // Phase 4: Lower to LIR with intrinsic specialization
    let intrinsics = crate::lir::intrinsics::build_intrinsics(symbols);
    let imm_prims = crate::lir::intrinsics::build_immediate_primitives(symbols);
    let mut lowerer = Lowerer::new()
        .with_intrinsics(intrinsics)
        .with_immediate_primitives(imm_prims);
    let lir_func = lowerer.lower(&analysis.hir)?;

    // Phase 5: Emit bytecode with symbol names for cross-thread portability
    let symbol_snapshot = symbols.all_names();
    let mut emitter = Emitter::new_with_symbols(symbol_snapshot);
    let (bytecode, _yield_points, _call_sites) = emitter.emit(&lir_func);

    Ok(CompileResult {
        bytecode,
        warnings: Vec::new(),
    })
}

/// Compile multiple top-level forms with fixpoint effect inference.
///
/// Uses fixpoint iteration to correctly infer effects for mutually recursive
/// top-level defines. The algorithm:
/// 1. Pre-scan all forms for `(def name (fn ...))` patterns
/// 2. Seed `global_effects` with `Effect::inert()` for all such defines (optimistic)
/// 3. Analyze all forms, collecting actual inferred effects
/// 4. If any effect changed, re-analyze with corrected effects
/// 5. Repeat until stable (max 10 iterations)
pub fn compile_all(source: &str, symbols: &mut SymbolTable) -> Result<Vec<CompileResult>, String> {
    // Ensure caller's SymbolTable has primitive names interned so that
    // SymbolIds match the cached PrimitiveMeta.
    intern_primitive_names(symbols);

    let syntaxes = read_syntax_all(source)?;

    let (macro_vm_ptr, mut expander, meta) = cache::get_compilation_cache();
    // SAFETY: The cached VM is thread-local and pipeline functions are not
    // re-entrant. The RefCell borrow was released by get_compilation_cache.
    let macro_vm = unsafe { &mut *macro_vm_ptr };

    // Expand all forms first (expansion is idempotent)
    let mut expanded_forms = Vec::new();
    for syntax in syntaxes {
        let expanded = expander.expand(syntax, symbols, macro_vm)?;
        expanded_forms.push(expanded);
    }

    let (global_effects, global_arities, immutable_globals) =
        scan::prescan_forms(&expanded_forms, symbols);

    let analysis_results = fixpoint::run_fixpoint(
        &expanded_forms,
        symbols,
        &meta,
        global_effects,
        global_arities,
        immutable_globals,
        |a| mark_tail_calls(&mut a.hir),
    )?;

    // Lower and emit all forms
    let intrinsics = crate::lir::intrinsics::build_intrinsics(symbols);
    let mut results = Vec::new();
    for analysis in analysis_results {
        let imm_prims = crate::lir::intrinsics::build_immediate_primitives(symbols);
        let mut lowerer = Lowerer::new()
            .with_intrinsics(intrinsics.clone())
            .with_immediate_primitives(imm_prims);
        let lir_func = lowerer.lower(&analysis.hir)?;

        let symbol_snapshot = symbols.all_names();
        let mut emitter = Emitter::new_with_symbols(symbol_snapshot);
        let (bytecode, _yield_points, _call_sites) = emitter.emit(&lir_func);

        results.push(CompileResult {
            bytecode,
            warnings: Vec::new(),
        });
    }

    Ok(results)
}
