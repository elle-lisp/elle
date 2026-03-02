//! Compilation pipeline: Syntax -> HIR -> LIR -> Bytecode
//!
//! This module provides the end-to-end compilation functions.

use crate::compiler::Bytecode;
use crate::effects::Effect;
use crate::hir::tailcall::mark_tail_calls;
use crate::hir::{AnalysisResult, Analyzer, Hir};
use crate::lir::{Emitter, Lowerer};
use crate::primitives::cached_primitive_meta;
use crate::primitives::def::PrimitiveMeta;
use crate::primitives::intern_primitive_names;
use crate::primitives::register_primitives;
use crate::reader::{read_syntax, read_syntax_all};
use crate::symbol::SymbolTable;
use crate::syntax::{Expander, Syntax, SyntaxKind};
use crate::value::types::Arity;
use crate::value::SymbolId;
use crate::vm::VM;
use std::collections::HashMap;

/// Compilation result
#[derive(Debug)]
pub struct CompileResult {
    pub bytecode: Bytecode,
    pub warnings: Vec<String>,
}

/// Analysis-only result (no bytecode generation)
/// Used by linter and LSP which need HIR but not bytecode
pub struct AnalyzeResult {
    pub hir: Hir,
}

/// Cached compilation state for pipeline functions.
///
/// Eliminates per-call costs of VM creation, primitive registration,
/// and prelude loading. Thread-local because VM contains Rc values.
///
/// # Invariants
///
/// - Prelude must be 100% defmacro (no runtime definitions)
/// - Primitives must be registered before any pipeline function call
/// - Pipeline functions are not re-entrant (no nested compile/compile_all)
/// - Primitive registration order is deterministic (ALL_TABLES)
struct CompilationCache {
    /// VM with primitives registered. Fiber always reset between uses.
    vm: VM,
    /// Expander with prelude loaded. Cloned for each pipeline call.
    expander: Expander,
    /// Primitive metadata from register_primitives.
    meta: PrimitiveMeta,
}

thread_local! {
    static COMPILATION_CACHE: std::cell::RefCell<Option<CompilationCache>> =
        const { std::cell::RefCell::new(None) };
}

/// Get or initialize the compilation cache, returning a VM with reset fiber,
/// a cloned Expander, and cloned PrimitiveMeta.
///
/// The VM's fiber is always reset before use. The Expander is cloned so that
/// each pipeline call gets independent expansion state (scope IDs, depth).
///
/// # Safety / Lifetime
///
/// Returns a raw pointer to the cached VM. The VM lives in a thread-local
/// RefCell; the borrow is released before this function returns, so the
/// caller must not call `get_compilation_cache` again while the VM pointer
/// is live. This is safe because pipeline functions are not re-entrant
/// (`eval_syntax` receives its own `&mut VM`, not the cache VM).
fn get_compilation_cache() -> (*mut VM, Expander, PrimitiveMeta) {
    COMPILATION_CACHE.with(|cache| {
        let mut cache_ref = cache.borrow_mut();
        let c = cache_ref.get_or_insert_with(|| {
            let mut vm = VM::new();
            let mut init_symbols = SymbolTable::new();
            let meta = register_primitives(&mut vm, &mut init_symbols);
            let mut expander = Expander::new();
            // Prelude loading needs the VM for macro body evaluation.
            // The init_symbols is throwaway — prelude is 100% defmacro,
            // so handle_defmacro doesn't touch SymbolTable.
            expander
                .load_prelude(&mut init_symbols, &mut vm)
                .expect("prelude loading must succeed");
            CompilationCache { vm, expander, meta }
        });

        // Always reset fiber before use
        c.vm.reset_fiber();

        let expander = c.expander.clone();
        let meta = c.meta.clone();
        let vm_ptr = &mut c.vm as *mut VM;
        (vm_ptr, expander, meta)
    })
}

/// Get a cloned Expander and PrimitiveMeta from the cache without
/// borrowing the cached VM. Used by functions that have their own VM
/// (eval, analyze, analyze_all).
fn get_cached_expander_and_meta() -> (Expander, PrimitiveMeta) {
    COMPILATION_CACHE.with(|cache| {
        let mut cache_ref = cache.borrow_mut();
        let c = cache_ref.get_or_insert_with(|| {
            let mut vm = VM::new();
            let mut init_symbols = SymbolTable::new();
            let meta = register_primitives(&mut vm, &mut init_symbols);
            let mut expander = Expander::new();
            expander
                .load_prelude(&mut init_symbols, &mut vm)
                .expect("prelude loading must succeed");
            CompilationCache { vm, expander, meta }
        });
        (c.expander.clone(), c.meta.clone())
    })
}

/// Scan an expanded syntax form for `(var/def name (fn ...))` patterns.
/// Returns the SymbolId and syntactic arity if this is a binding-lambda form.
fn scan_define_lambda(
    syntax: &Syntax,
    symbols: &mut SymbolTable,
) -> Option<(SymbolId, Option<Arity>)> {
    if let SyntaxKind::List(items) = &syntax.kind {
        if items.len() >= 3 {
            if let Some(name) = items[0].as_symbol() {
                if name == "var" || name == "def" {
                    if let Some(def_name) = items[1].as_symbol() {
                        // Check if value is a lambda form
                        if let SyntaxKind::List(val_items) = &items[2].kind {
                            if let Some(first) = val_items.first() {
                                if let Some(kw) = first.as_symbol() {
                                    if kw == "fn" {
                                        let sym = symbols.intern(def_name);
                                        // Extract arity from the parameter list
                                        let arity = val_items
                                            .get(1)
                                            .and_then(|s| s.as_list())
                                            .map(|params| Arity::Exact(params.len()));
                                        return Some((sym, arity));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

/// Scan an expanded syntax form for `(def name ...)` patterns.
/// Returns the SymbolId of the name if this is a def (immutable) form.
fn scan_const_binding(syntax: &Syntax, symbols: &mut SymbolTable) -> Option<SymbolId> {
    if let SyntaxKind::List(items) = &syntax.kind {
        if items.len() >= 3 {
            if let Some(name) = items[0].as_symbol() {
                if name == "def" {
                    if let Some(def_name) = items[1].as_symbol() {
                        return Some(symbols.intern(def_name));
                    }
                }
            }
        }
    }
    None
}

/// Compile and execute a Syntax tree, reusing the caller's Expander.
///
/// This is the entry point for macro body evaluation: the Expander builds
/// a let-expression wrapping the macro body, then calls this to compile
/// and run it in the VM. The same Expander is threaded through so nested
/// macro calls work.
pub fn eval_syntax(
    syntax: Syntax,
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
    let mut lowerer = Lowerer::new().with_intrinsics(intrinsics);
    let lir_func = lowerer.lower(&analysis.hir)?;

    let symbol_snapshot = symbols.all_names();
    let mut emitter = Emitter::new_with_symbols(symbol_snapshot);
    let bytecode = emitter.emit(&lir_func);

    vm.execute(&bytecode).map_err(|e| e.to_string())
}

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
    let (macro_vm_ptr, mut expander, meta) = get_compilation_cache();
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
    let mut lowerer = Lowerer::new().with_intrinsics(intrinsics);
    let lir_func = lowerer.lower(&analysis.hir)?;

    // Phase 5: Emit bytecode with symbol names for cross-thread portability
    let symbol_snapshot = symbols.all_names();
    let mut emitter = Emitter::new_with_symbols(symbol_snapshot);
    let bytecode = emitter.emit(&lir_func);

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
/// 2. Seed `global_effects` with `Effect::none()` for all such defines (optimistic)
/// 3. Analyze all forms, collecting actual inferred effects
/// 4. If any effect changed, re-analyze with corrected effects
/// 5. Repeat until stable (max 10 iterations)
pub fn compile_all(source: &str, symbols: &mut SymbolTable) -> Result<Vec<CompileResult>, String> {
    // Ensure caller's SymbolTable has primitive names interned so that
    // SymbolIds match the cached PrimitiveMeta.
    intern_primitive_names(symbols);

    let syntaxes = read_syntax_all(source)?;

    let (macro_vm_ptr, mut expander, meta) = get_compilation_cache();
    // SAFETY: The cached VM is thread-local and pipeline functions are not
    // re-entrant. The RefCell borrow was released by get_compilation_cache.
    let macro_vm = unsafe { &mut *macro_vm_ptr };

    // Expand all forms first (expansion is idempotent)
    let mut expanded_forms = Vec::new();
    for syntax in syntaxes {
        let expanded = expander.expand(syntax, symbols, macro_vm)?;
        expanded_forms.push(expanded);
    }

    // Pre-scan: find all (def name (fn ...)) patterns and seed effects/arities
    let mut global_effects: HashMap<SymbolId, Effect> = HashMap::new();
    let mut global_arities: HashMap<SymbolId, Arity> = HashMap::new();
    for form in &expanded_forms {
        if let Some((sym, arity)) = scan_define_lambda(form, symbols) {
            global_effects.insert(sym, Effect::none());
            if let Some(arity) = arity {
                global_arities.insert(sym, arity);
            }
        }
    }

    // Pre-scan: find all (def name ...) patterns for immutability tracking
    let mut immutable_globals: std::collections::HashSet<SymbolId> =
        std::collections::HashSet::new();
    for form in &expanded_forms {
        if let Some(sym) = scan_const_binding(form, symbols) {
            immutable_globals.insert(sym);
        }
    }

    // Fixpoint loop: analyze until effects stabilize
    let mut analysis_results: Vec<AnalysisResult> = Vec::new();
    const MAX_ITERATIONS: usize = 10;

    for _iteration in 0..MAX_ITERATIONS {
        analysis_results.clear();
        let mut new_global_effects: HashMap<SymbolId, Effect> = HashMap::new();

        for form in &expanded_forms {
            let mut analyzer =
                Analyzer::new_with_primitives(symbols, meta.effects.clone(), meta.arities.clone());
            // Seed with current global effects (from pre-scan or previous iteration)
            analyzer.set_global_effects(global_effects.clone());
            // Seed with global arities from pre-scan and previous forms
            analyzer.set_global_arities(global_arities.clone());
            // Seed with immutable globals from pre-scan
            analyzer.set_immutable_globals(immutable_globals.clone());

            let mut analysis = analyzer.analyze(form)?;

            // Collect effects and arities from this form's defines
            for (sym, effect) in analyzer.take_defined_global_effects() {
                new_global_effects.insert(sym, effect);
            }
            for (sym, arity) in analyzer.take_defined_global_arities() {
                global_arities.insert(sym, arity);
            }

            // Merge defined immutable globals from this form
            for sym in analyzer.take_defined_immutable_globals() {
                immutable_globals.insert(sym);
            }

            mark_tail_calls(&mut analysis.hir);
            analysis_results.push(analysis);
        }

        // Check for convergence: did any effect change?
        if new_global_effects == global_effects {
            break; // Stable -- we're done
        }

        // Effects changed -- update and re-analyze
        global_effects = new_global_effects;
    }

    // Lower and emit all forms
    let intrinsics = crate::lir::intrinsics::build_intrinsics(symbols);
    let mut results = Vec::new();
    for analysis in analysis_results {
        let mut lowerer = Lowerer::new().with_intrinsics(intrinsics.clone());
        let lir_func = lowerer.lower(&analysis.hir)?;

        let symbol_snapshot = symbols.all_names();
        let mut emitter = Emitter::new_with_symbols(symbol_snapshot);
        let bytecode = emitter.emit(&lir_func);

        results.push(CompileResult {
            bytecode,
            warnings: Vec::new(),
        });
    }

    Ok(results)
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
    let (mut expander, meta) = get_cached_expander_and_meta();

    let expanded = expander.expand(syntax, symbols, vm)?;

    let mut analyzer = Analyzer::new_with_primitives(symbols, meta.effects, meta.arities);
    let mut analysis = analyzer.analyze(&expanded)?;
    mark_tail_calls(&mut analysis.hir);

    let intrinsics = crate::lir::intrinsics::build_intrinsics(symbols);
    let mut lowerer = Lowerer::new().with_intrinsics(intrinsics);
    let lir_func = lowerer.lower(&analysis.hir)?;

    let symbol_snapshot = symbols.all_names();
    let mut emitter = Emitter::new_with_symbols(symbol_snapshot);
    let bytecode = emitter.emit(&lir_func);

    vm.execute(&bytecode).map_err(|e| e.to_string())
}

/// Compile and execute multiple top-level forms.
///
/// Each form is compiled with fixpoint effect inference (like `compile_all`)
/// then executed sequentially. Returns the value of the last form.
/// Returns `Ok(Value::NIL)` for empty input.
pub fn eval_all(
    source: &str,
    symbols: &mut SymbolTable,
    vm: &mut VM,
) -> Result<crate::value::Value, String> {
    let results = compile_all(source, symbols)?;
    let mut last_value = crate::value::Value::NIL;
    for result in results {
        last_value = vm.execute(&result.bytecode).map_err(|e| e.to_string())?;
    }
    Ok(last_value)
}

/// Analyze source code without generating bytecode.
/// Used by linter and LSP which need HIR but not bytecode.
pub fn analyze(
    source: &str,
    symbols: &mut SymbolTable,
    vm: &mut VM,
) -> Result<AnalyzeResult, String> {
    let syntax = read_syntax(source)?;

    let (mut expander, meta) = get_cached_expander_and_meta();

    let expanded = expander.expand(syntax, symbols, vm)?;
    let mut analyzer = Analyzer::new_with_primitives(symbols, meta.effects, meta.arities);
    let analysis = analyzer.analyze(&expanded)?;
    Ok(AnalyzeResult { hir: analysis.hir })
}

/// Analyze multiple top-level forms without generating bytecode.
/// Uses fixpoint iteration for effect inference (same as compile_all).
pub fn analyze_all(
    source: &str,
    symbols: &mut SymbolTable,
    vm: &mut VM,
) -> Result<Vec<AnalyzeResult>, String> {
    let syntaxes = read_syntax_all(source)?;

    let (mut expander, meta) = get_cached_expander_and_meta();

    // Expand all forms first
    let mut expanded_forms = Vec::new();
    for syntax in syntaxes {
        let expanded = expander.expand(syntax, symbols, vm)?;
        expanded_forms.push(expanded);
    }

    // Pre-scan: find all (def name (fn ...)) patterns and seed effects/arities
    let mut global_effects: HashMap<SymbolId, Effect> = HashMap::new();
    let mut global_arities: HashMap<SymbolId, Arity> = HashMap::new();
    for form in &expanded_forms {
        if let Some((sym, arity)) = scan_define_lambda(form, symbols) {
            global_effects.insert(sym, Effect::none());
            if let Some(arity) = arity {
                global_arities.insert(sym, arity);
            }
        }
    }

    // Pre-scan: find all (def name ...) patterns for immutability tracking
    let mut immutable_globals: std::collections::HashSet<SymbolId> =
        std::collections::HashSet::new();
    for form in &expanded_forms {
        if let Some(sym) = scan_const_binding(form, symbols) {
            immutable_globals.insert(sym);
        }
    }

    // Fixpoint loop: analyze until effects stabilize
    let mut analysis_results: Vec<AnalysisResult> = Vec::new();
    const MAX_ITERATIONS: usize = 10;

    for _iteration in 0..MAX_ITERATIONS {
        analysis_results.clear();
        let mut new_global_effects: HashMap<SymbolId, Effect> = HashMap::new();

        for form in &expanded_forms {
            let mut analyzer =
                Analyzer::new_with_primitives(symbols, meta.effects.clone(), meta.arities.clone());
            analyzer.set_global_effects(global_effects.clone());
            analyzer.set_global_arities(global_arities.clone());
            analyzer.set_immutable_globals(immutable_globals.clone());

            let analysis = analyzer.analyze(form)?;

            for (sym, effect) in analyzer.take_defined_global_effects() {
                new_global_effects.insert(sym, effect);
            }
            for (sym, arity) in analyzer.take_defined_global_arities() {
                global_arities.insert(sym, arity);
            }

            // Merge defined immutable globals from this form
            for sym in analyzer.take_defined_immutable_globals() {
                immutable_globals.insert(sym);
            }

            analysis_results.push(analysis);
        }

        // Check for convergence
        if new_global_effects == global_effects {
            break;
        }

        global_effects = new_global_effects;
    }

    // Convert to AnalyzeResult
    Ok(analysis_results
        .into_iter()
        .map(|a| AnalyzeResult { hir: a.hir })
        .collect())
}
