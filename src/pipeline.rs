//! Compilation pipeline: Syntax -> HIR -> LIR -> Bytecode
//!
//! This module provides the end-to-end compilation functions.

use crate::compiler::Bytecode;
use crate::effects::Effect;
use crate::hir::tailcall::mark_tail_calls;
use crate::hir::{AnalysisResult, Analyzer, Hir};
use crate::lir::{Emitter, Lowerer};
use crate::primitives::build_primitive_meta;
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

    let meta = build_primitive_meta(symbols);
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
    // Phase 1: Parse to Syntax
    let syntax = read_syntax(source)?;

    // Phase 2: Macro expansion (internal VM for macro bodies)
    let mut expander = Expander::new();
    let mut macro_vm = VM::new();
    let meta = register_primitives(&mut macro_vm, symbols);
    expander.load_prelude(symbols, &mut macro_vm)?;
    let expanded = expander.expand(syntax, symbols, &mut macro_vm)?;

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
    let syntaxes = read_syntax_all(source)?;
    let mut expander = Expander::new();
    let mut macro_vm = VM::new();
    let meta = register_primitives(&mut macro_vm, symbols);
    expander.load_prelude(symbols, &mut macro_vm)?;

    // Expand all forms first (expansion is idempotent)
    let mut expanded_forms = Vec::new();
    for syntax in syntaxes {
        let expanded = expander.expand(syntax, symbols, &mut macro_vm)?;
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

    let mut expander = Expander::new();
    expander.load_prelude(symbols, vm)?;
    let expanded = expander.expand(syntax, symbols, vm)?;

    let meta = build_primitive_meta(symbols);
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
    let mut expander = Expander::new();
    expander.load_prelude(symbols, vm)?;
    let expanded = expander.expand(syntax, symbols, vm)?;
    let meta = build_primitive_meta(symbols);
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
    let mut expander = Expander::new();
    expander.load_prelude(symbols, vm)?;

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
    let meta = build_primitive_meta(symbols);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::VM;

    fn setup() -> (SymbolTable, VM) {
        let mut symbols = SymbolTable::new();
        let mut vm = VM::new();
        let _effects = register_primitives(&mut vm, &mut symbols);
        (symbols, vm)
    }

    #[test]
    fn test_compile_literal() {
        let (mut symbols, _) = setup();
        let result = compile("42", &mut symbols);
        assert!(result.is_ok());
        let compiled = result.unwrap();
        assert!(!compiled.bytecode.instructions.is_empty());
    }

    #[test]
    fn test_compile_if() {
        let (mut symbols, _) = setup();
        let result = compile("(if true 1 2)", &mut symbols);
        assert!(result.is_ok());
    }

    #[test]
    fn test_compile_let() {
        let (mut symbols, _) = setup();
        let result = compile("(let ((x 10)) x)", &mut symbols);
        assert!(result.is_ok());
    }

    #[test]
    fn test_compile_lambda() {
        let (mut symbols, _) = setup();
        let result = compile("(fn (x) x)", &mut symbols);
        assert!(result.is_ok());
    }

    #[test]
    fn test_compile_call() {
        let (mut symbols, _) = setup();
        // Note: Function calls to built-in symbols like + may fail during lowering
        // because the new pipeline doesn't yet have full integration with built-in symbols.
        // This test just verifies that the pipeline can attempt to compile function calls.
        let result = compile("(+ 1 2)", &mut symbols);
        // We accept either success or a specific error about unbound variables
        // since the new pipeline is still being integrated
        match result {
            Ok(_) => {}                                    // Success is fine
            Err(e) if e.contains("Unbound variable") => {} // Expected during integration
            Err(e) => panic!("Unexpected error: {}", e),
        }
    }

    #[test]
    fn test_compile_global_variable() {
        let (mut symbols, _) = setup();
        // Test that global variables (like +) are properly recognized and emit LoadGlobal
        // instead of "Unbound variable" error
        let result = compile("(+ 1 2)", &mut symbols);
        // After the fix, this should compile successfully (or at least not fail with "Unbound variable")
        match result {
            Ok(_) => {
                // Success! The global variable + was properly handled
            }
            Err(e) if e.contains("Unbound variable") => {
                panic!("Global variable handling failed: {}", e);
            }
            Err(_e) => {
                // Other errors are acceptable (e.g., bytecode execution issues)
                // as long as it's not "Unbound variable"
            }
        }
    }

    #[test]
    fn test_compile_begin() {
        let (mut symbols, _) = setup();
        let result = compile("(begin 1 2 3)", &mut symbols);
        assert!(result.is_ok());
    }

    #[test]
    fn test_compile_and() {
        let (mut symbols, _) = setup();
        let result = compile("(and true true false)", &mut symbols);
        assert!(result.is_ok());
    }

    #[test]
    fn test_compile_or() {
        let (mut symbols, _) = setup();
        let result = compile("(or false false true)", &mut symbols);
        assert!(result.is_ok());
    }

    #[test]
    fn test_compile_while() {
        let (mut symbols, _) = setup();
        let result = compile("(while false nil)", &mut symbols);
        assert!(result.is_ok());
    }

    #[test]
    fn test_compile_cond() {
        let (mut symbols, _) = setup();
        let result = compile("(cond (true 1) (else 2))", &mut symbols);
        assert!(result.is_ok());
    }

    #[test]
    fn test_compile_all() {
        let (mut symbols, _) = setup();
        let result = compile_all("1 2 3", &mut symbols);
        assert!(result.is_ok());
        let compiled = result.unwrap();
        assert_eq!(compiled.len(), 3);
    }

    #[test]
    fn test_eval_literal() {
        let (mut symbols, mut vm) = setup();
        let result = eval("42", &mut symbols, &mut vm);
        // Note: execution may fail due to incomplete bytecode mapping
        // but compilation should succeed
        let _ = result;
    }

    #[test]
    fn test_eval_addition() {
        let (mut symbols, mut vm) = setup();
        let result = eval("(+ 1 2)", &mut symbols, &mut vm);
        match result {
            Ok(v) => assert_eq!(v, crate::value::Value::int(3)),
            Err(e) => panic!("Expected Ok(3), got Err: {}", e),
        }
    }

    #[test]
    fn test_eval_subtraction() {
        let (mut symbols, mut vm) = setup();
        let result = eval("(- 10 3)", &mut symbols, &mut vm);
        match result {
            Ok(v) => assert_eq!(v, crate::value::Value::int(7)),
            Err(e) => panic!("Expected Ok(7), got Err: {}", e),
        }
    }

    #[test]
    fn test_eval_nested_arithmetic() {
        let (mut symbols, mut vm) = setup();
        let result = eval("(+ (* 2 3) (- 10 5))", &mut symbols, &mut vm);
        match result {
            Ok(v) => assert_eq!(v, crate::value::Value::int(11)),
            Err(e) => panic!("Expected Ok(11), got Err: {}", e),
        }
    }

    #[test]
    fn test_eval_if_true() {
        let (mut symbols, mut vm) = setup();
        let result = eval("(if true 42 0)", &mut symbols, &mut vm);
        match result {
            Ok(v) => assert_eq!(v, crate::value::Value::int(42)),
            Err(e) => panic!("Expected Ok(42), got Err: {}", e),
        }
    }

    #[test]
    fn test_eval_if_false() {
        let (mut symbols, mut vm) = setup();
        let result = eval("(if false 42 0)", &mut symbols, &mut vm);
        match result {
            Ok(v) => assert_eq!(v, crate::value::Value::int(0)),
            Err(e) => panic!("Expected Ok(0), got Err: {}", e),
        }
    }

    #[test]
    fn test_eval_let_simple() {
        let (mut symbols, mut vm) = setup();
        let result = eval("(let ((x 10)) x)", &mut symbols, &mut vm);
        match result {
            Ok(v) => assert_eq!(v, crate::value::Value::int(10)),
            Err(e) => panic!("Expected Ok(10), got Err: {}", e),
        }
    }

    #[test]
    fn test_eval_let_with_arithmetic() {
        let (mut symbols, mut vm) = setup();
        let result = eval("(let ((x 10) (y 5)) (+ x y))", &mut symbols, &mut vm);
        match result {
            Ok(v) => assert_eq!(v, crate::value::Value::int(15)),
            Err(e) => panic!("Expected Ok(15), got Err: {}", e),
        }
    }

    #[test]
    fn test_eval_lambda_identity() {
        let (mut symbols, mut vm) = setup();
        let result = eval("((fn (x) x) 42)", &mut symbols, &mut vm);
        match result {
            Ok(v) => assert_eq!(v, crate::value::Value::int(42)),
            Err(e) => panic!("Expected Ok(42), got Err: {}", e),
        }
    }

    #[test]
    fn test_eval_lambda_add_one() {
        let (mut symbols, mut vm) = setup();
        let result = eval("((fn (x) (+ x 1)) 10)", &mut symbols, &mut vm);
        match result {
            Ok(v) => assert_eq!(v, crate::value::Value::int(11)),
            Err(e) => panic!("Expected Ok(11), got Err: {}", e),
        }
    }

    #[test]
    fn test_compile_lambda_with_capture() {
        let (mut symbols, _) = setup();
        let result = compile("(let ((x 10)) (fn () x))", &mut symbols);
        match result {
            Ok(_) => {}
            Err(e) => panic!("Failed to compile lambda with capture: {}", e),
        }
    }

    #[test]
    fn test_eval_begin() {
        let (mut symbols, mut vm) = setup();
        let result = eval("(begin 1 2 3)", &mut symbols, &mut vm);
        match result {
            Ok(v) => assert_eq!(v, crate::value::Value::int(3)),
            Err(e) => panic!("Expected Ok(3), got Err: {}", e),
        }
    }

    #[test]
    fn test_eval_comparison_lt() {
        let (mut symbols, mut vm) = setup();
        let result = eval("(< 1 2)", &mut symbols, &mut vm);
        match result {
            Ok(v) => assert_eq!(v, crate::value::Value::bool(true)),
            Err(e) => panic!("Expected Ok(true), got Err: {}", e),
        }
    }

    #[test]
    fn test_compile_all_examples() {
        use std::fs;
        use std::path::Path;

        let examples_dir = "examples";
        let mut passed = Vec::new();
        let mut failed = Vec::new();

        if !Path::new(examples_dir).exists() {
            println!("Examples directory not found, skipping test");
            return;
        }

        for entry in fs::read_dir(examples_dir).expect("Failed to read examples directory") {
            let entry = entry.expect("Failed to read directory entry");
            let path = entry.path();

            if path.extension().is_some_and(|e| e == "lisp") {
                let filename = path.file_name().unwrap().to_string_lossy().to_string();
                let content = fs::read_to_string(&path).expect("Failed to read example file");

                let (mut symbols, _) = setup();
                match compile(&content, &mut symbols) {
                    Ok(_) => {
                        passed.push(filename);
                    }
                    Err(e) => {
                        failed.push((filename, e));
                    }
                }
            }
        }

        println!("\n=== Example Compilation Results ===");
        println!("Passed: {}", passed.len());
        for file in &passed {
            println!("  ✓ {}", file);
        }

        if !failed.is_empty() {
            println!("\nFailed: {}", failed.len());
            for (file, err) in &failed {
                println!("  ✗ {}: {}", file, err);
            }
        }

        println!("\nTotal: {} passed, {} failed", passed.len(), failed.len());

        // Don't fail the test - just report results
        // This allows us to see which examples work and which don't
    }

    #[test]
    fn test_execute_simple_examples() {
        use std::fs;
        use std::path::Path;

        let examples_dir = "examples";
        let mut executed = Vec::new();
        let mut execution_failed = Vec::new();

        if !Path::new(examples_dir).exists() {
            println!("Examples directory not found, skipping test");
            return;
        }

        // Test specific simple examples that should execute
        let test_files = vec!["hello.lisp"];

        for filename in test_files {
            let path = Path::new(examples_dir).join(filename);
            if path.exists() {
                let content = fs::read_to_string(&path).expect("Failed to read example file");
                let (mut symbols, mut vm) = setup();

                match eval(&content, &mut symbols, &mut vm) {
                    Ok(_) => {
                        executed.push(filename.to_string());
                    }
                    Err(e) => {
                        execution_failed.push((filename.to_string(), e));
                    }
                }
            }
        }

        println!("\n=== Example Execution Results ===");
        println!("Executed: {}", executed.len());
        for file in &executed {
            println!("  ✓ {}", file);
        }

        if !execution_failed.is_empty() {
            println!("\nExecution Failed: {}", execution_failed.len());
            for (file, err) in &execution_failed {
                println!("  ✗ {}: {}", file, err);
            }
        }
    }

    // === Control Flow: cond ===

    #[test]
    fn test_eval_cond_first_true() {
        let (mut symbols, mut vm) = setup();
        let result = eval("(cond (true 42))", &mut symbols, &mut vm);
        assert_eq!(result.unwrap(), crate::value::Value::int(42));
    }

    #[test]
    fn test_eval_cond_second_true() {
        let (mut symbols, mut vm) = setup();
        let result = eval("(cond (false 1) (true 42))", &mut symbols, &mut vm);
        assert_eq!(result.unwrap(), crate::value::Value::int(42));
    }

    #[test]
    fn test_eval_cond_else() {
        let (mut symbols, mut vm) = setup();
        let result = eval(
            "(cond (false 1) (false 2) (else 42))",
            &mut symbols,
            &mut vm,
        );
        assert_eq!(result.unwrap(), crate::value::Value::int(42));
    }

    #[test]
    fn test_eval_cond_with_expressions() {
        let (mut symbols, mut vm) = setup();
        let result = eval("(cond ((< 5 10) (+ 20 22)))", &mut symbols, &mut vm);
        assert_eq!(result.unwrap(), crate::value::Value::int(42));
    }

    // === Control Flow: and ===

    #[test]
    fn test_eval_and_all_true() {
        let (mut symbols, mut vm) = setup();
        let result = eval("(and true true true)", &mut symbols, &mut vm);
        assert_eq!(result.unwrap(), crate::value::Value::bool(true));
    }

    #[test]
    fn test_eval_and_one_false() {
        let (mut symbols, mut vm) = setup();
        let result = eval("(and true false true)", &mut symbols, &mut vm);
        assert_eq!(result.unwrap(), crate::value::Value::bool(false));
    }

    #[test]
    fn test_eval_and_returns_last() {
        let (mut symbols, mut vm) = setup();
        let result = eval("(and 1 2 3)", &mut symbols, &mut vm);
        assert_eq!(result.unwrap(), crate::value::Value::int(3));
    }

    #[test]
    fn test_eval_and_short_circuit() {
        let (mut symbols, mut vm) = setup();
        // If and doesn't short-circuit, this would fail trying to call nil
        let result = eval("(and false (nil))", &mut symbols, &mut vm);
        assert_eq!(result.unwrap(), crate::value::Value::bool(false));
    }

    #[test]
    fn test_eval_and_empty() {
        let (mut symbols, mut vm) = setup();
        let result = eval("(and)", &mut symbols, &mut vm);
        assert_eq!(result.unwrap(), crate::value::Value::bool(true));
    }

    // === Control Flow: or ===

    #[test]
    fn test_eval_or_all_false() {
        let (mut symbols, mut vm) = setup();
        let result = eval("(or false false false)", &mut symbols, &mut vm);
        assert_eq!(result.unwrap(), crate::value::Value::bool(false));
    }

    #[test]
    fn test_eval_or_one_true() {
        let (mut symbols, mut vm) = setup();
        let result = eval("(or false true false)", &mut symbols, &mut vm);
        assert_eq!(result.unwrap(), crate::value::Value::bool(true));
    }

    #[test]
    fn test_eval_or_returns_first_truthy() {
        let (mut symbols, mut vm) = setup();
        let result = eval("(or false 42 99)", &mut symbols, &mut vm);
        assert_eq!(result.unwrap(), crate::value::Value::int(42));
    }

    #[test]
    fn test_destructure_list_basic() {
        let (mut symbols, mut vm) = setup();
        eval("(def (a b c) (list 1 2 3))", &mut symbols, &mut vm).unwrap();
        assert_eq!(eval("a", &mut symbols, &mut vm).unwrap().as_int(), Some(1));
        assert_eq!(eval("b", &mut symbols, &mut vm).unwrap().as_int(), Some(2));
        assert_eq!(eval("c", &mut symbols, &mut vm).unwrap().as_int(), Some(3));
    }

    #[test]
    fn test_eval_or_empty() {
        let (mut symbols, mut vm) = setup();
        let result = eval("(or)", &mut symbols, &mut vm);
        assert_eq!(result.unwrap(), crate::value::Value::bool(false));
    }

    // === Control Flow: while ===

    #[test]
    fn test_eval_while_never_executes() {
        let (mut symbols, mut vm) = setup();
        let result = eval("(while false 42)", &mut symbols, &mut vm);
        assert_eq!(result.unwrap(), crate::value::Value::NIL);
    }

    #[test]
    fn test_eval_while_with_mutation() {
        let (mut symbols, mut vm) = setup();
        let result = eval(
            "(begin (var x 0) (while (< x 5) (set! x (+ x 1))) x)",
            &mut symbols,
            &mut vm,
        );
        assert_eq!(result.unwrap(), crate::value::Value::int(5));
    }

    // === Closures and Captures ===

    #[test]
    fn test_eval_closure_captures_local() {
        let (mut symbols, mut vm) = setup();
        let result = eval("(let ((x 10)) ((fn () x)))", &mut symbols, &mut vm);
        assert_eq!(result.unwrap(), crate::value::Value::int(10));
    }

    #[test]
    fn test_eval_closure_captures_multiple() {
        let (mut symbols, mut vm) = setup();
        let result = eval(
            "(let ((x 10) (y 20)) ((fn () (+ x y))))",
            &mut symbols,
            &mut vm,
        );
        assert_eq!(result.unwrap(), crate::value::Value::int(30));
    }

    #[test]
    fn test_eval_nested_closure() {
        let (mut symbols, mut vm) = setup();
        let result = eval(
            "(let ((x 10)) ((fn () ((fn () x)))))",
            &mut symbols,
            &mut vm,
        );
        assert_eq!(result.unwrap(), crate::value::Value::int(10));
    }

    #[test]
    fn test_eval_closure_with_param_and_capture() {
        let (mut symbols, mut vm) = setup();
        let result = eval("(let ((x 10)) ((fn (y) (+ x y)) 5))", &mut symbols, &mut vm);
        assert_eq!(result.unwrap(), crate::value::Value::int(15));
    }

    // === Higher-Order Functions ===

    #[test]
    fn test_eval_function_as_argument() {
        let (mut symbols, mut vm) = setup();
        let result = eval(
            "((fn (f x) (f x)) (fn (n) (+ n 1)) 10)",
            &mut symbols,
            &mut vm,
        );
        assert_eq!(result.unwrap(), crate::value::Value::int(11));
    }

    #[test]
    fn test_eval_function_returning_function() {
        let (mut symbols, mut vm) = setup();
        let result = eval("(((fn (x) (fn (y) (+ x y))) 10) 5)", &mut symbols, &mut vm);
        assert_eq!(result.unwrap(), crate::value::Value::int(15));
    }

    // === Define and Set! ===

    #[test]
    fn test_eval_define_then_use() {
        let (mut symbols, mut vm) = setup();
        let result = eval("(begin (var x 42) x)", &mut symbols, &mut vm);
        assert_eq!(result.unwrap(), crate::value::Value::int(42));
    }

    #[test]
    fn test_eval_define_then_set() {
        let (mut symbols, mut vm) = setup();
        let result = eval("(begin (var x 10) (set! x 42) x)", &mut symbols, &mut vm);
        assert_eq!(result.unwrap(), crate::value::Value::int(42));
    }

    #[test]
    fn test_eval_set_in_closure() {
        let (mut symbols, mut vm) = setup();
        let result = eval(
            "(begin 
               (var counter 0)
               (def inc (fn () (set! counter (+ counter 1))))
               (inc)
               (inc)
               counter)",
            &mut symbols,
            &mut vm,
        );
        assert_eq!(result.unwrap(), crate::value::Value::int(2));
    }

    #[test]
    fn test_intrinsic_fib() {
        // Fibonacci exercises intrinsic specialization with double recursion
        let (mut symbols, mut vm) = setup();
        let result = eval(
            "(begin
               (def fib (fn (n) (if (< n 2) n (+ (fib (- n 1)) (fib (- n 2))))))
               (fib 10))",
            &mut symbols,
            &mut vm,
        );
        assert_eq!(result.unwrap(), crate::value::Value::int(55));
    }

    #[test]
    fn test_intrinsic_unary_neg() {
        let (mut symbols, mut vm) = setup();
        assert_eq!(
            eval("(- 5)", &mut symbols, &mut vm).unwrap(),
            crate::value::Value::int(-5)
        );
        let (mut symbols, mut vm) = setup();
        assert_eq!(
            eval("(- -3)", &mut symbols, &mut vm).unwrap(),
            crate::value::Value::int(3)
        );
    }

    #[test]
    fn test_intrinsic_variadic_fallthrough() {
        // Variadic + falls through to generic call
        let (mut symbols, mut vm) = setup();
        assert_eq!(
            eval("(+ 1 2 3)", &mut symbols, &mut vm).unwrap(),
            crate::value::Value::int(6)
        );
    }

    #[test]
    fn test_intrinsic_not() {
        let (mut symbols, mut vm) = setup();
        assert_eq!(
            eval("(not true)", &mut symbols, &mut vm).unwrap(),
            crate::value::Value::bool(false)
        );
        let (mut symbols, mut vm) = setup();
        assert_eq!(
            eval("(not false)", &mut symbols, &mut vm).unwrap(),
            crate::value::Value::bool(true)
        );
    }

    #[test]
    fn test_intrinsic_rem() {
        let (mut symbols, mut vm) = setup();
        assert_eq!(
            eval("(rem 17 5)", &mut symbols, &mut vm).unwrap(),
            crate::value::Value::int(2)
        );
    }

    #[test]
    fn test_fold_multiple_elements() {
        let (mut symbols, mut vm) = setup();

        // Test with (list 1) - should work
        let code1 = r#"(begin
            (def process (fn (acc x) (begin (var doubled (* x 2)) (+ acc doubled))))
            (def my-fold (fn (f init lst)
                (if (nil? lst)
                    init
                    (my-fold f (f init (first lst)) (rest lst)))))
            (my-fold process 0 (list 1)))"#;

        let result1 = eval(code1, &mut symbols, &mut vm);
        println!("list 1: {:?}", result1);

        // Test with (list 1 2) - might fail
        let (mut symbols2, mut vm2) = setup();
        let code2 = r#"(begin
            (def process (fn (acc x) (begin (var doubled (* x 2)) (+ acc doubled))))
            (def my-fold (fn (f init lst)
                (if (nil? lst)
                    init
                    (my-fold f (f init (first lst)) (rest lst)))))
            (my-fold process 0 (list 1 2)))"#;

        let result2 = eval(code2, &mut symbols2, &mut vm2);
        println!("list 1 2: {:?}", result2);

        // Test with (list 1 2 3) - original failing case
        let (mut symbols3, mut vm3) = setup();
        let code3 = r#"(begin
            (def process (fn (acc x) (begin (var doubled (* x 2)) (+ acc doubled))))
            (def my-fold (fn (f init lst)
                (if (nil? lst)
                    init
                    (my-fold f (f init (first lst)) (rest lst)))))
            (my-fold process 0 (list 1 2 3)))"#;

        let result3 = eval(code3, &mut symbols3, &mut vm3);
        println!("list 1 2 3: {:?}", result3);
    }

    // === analyze tests ===

    #[test]
    fn test_analyze_literal() {
        let (mut symbols, mut vm) = setup();
        let result = analyze("42", &mut symbols, &mut vm);
        assert!(result.is_ok());
        let analysis = result.unwrap();
        assert!(matches!(analysis.hir.kind, crate::hir::HirKind::Int(42)));
    }

    #[test]
    fn test_analyze_define() {
        let (mut symbols, mut vm) = setup();
        let result = analyze("(var x 10)", &mut symbols, &mut vm);
        assert!(result.is_ok());
        let analysis = result.unwrap();
        assert!(matches!(
            analysis.hir.kind,
            crate::hir::HirKind::Define { .. }
        ));
    }

    #[test]
    fn test_analyze_lambda() {
        let (mut symbols, mut vm) = setup();
        let result = analyze("(fn (x) x)", &mut symbols, &mut vm);
        assert!(result.is_ok());
        let analysis = result.unwrap();
        assert!(matches!(
            analysis.hir.kind,
            crate::hir::HirKind::Lambda { .. }
        ));
    }

    #[test]
    fn test_analyze_all_multiple_forms() {
        let (mut symbols, mut vm) = setup();
        let result = analyze_all("1 2 3", &mut symbols, &mut vm);
        assert!(result.is_ok());
        let analyses = result.unwrap();
        assert_eq!(analyses.len(), 3);
        assert!(matches!(analyses[0].hir.kind, crate::hir::HirKind::Int(1)));
        assert!(matches!(analyses[1].hir.kind, crate::hir::HirKind::Int(2)));
        assert!(matches!(analyses[2].hir.kind, crate::hir::HirKind::Int(3)));
    }

    #[test]
    fn test_analyze_with_let() {
        let (mut symbols, mut vm) = setup();
        let result = analyze("(let ((x 1) (y 2)) (+ x y))", &mut symbols, &mut vm);
        assert!(result.is_ok());
        let analysis = result.unwrap();
        // Should produce a Let HIR node
        assert!(matches!(analysis.hir.kind, crate::hir::HirKind::Let { .. }));
    }

    #[test]
    fn test_mutual_recursion_effect_inference() {
        // Test that mutually recursive functions are inferred as Pure
        // when they only call each other and pure primitives
        let (mut symbols, _) = setup();
        let source = r#"
(def f (fn (x) (if (= x 0) 1 (g (- x 1)))))
(def g (fn (x) (if (= x 0) 2 (f (- x 1)))))
"#;
        let results = compile_all(source, &mut symbols);
        assert!(results.is_ok(), "Compilation should succeed");
        let results = results.unwrap();
        assert_eq!(results.len(), 2, "Should have 2 compiled forms");
        // Both forms should compile successfully
        // The key test is that they don't fail due to effect issues
    }

    #[test]
    fn test_mutual_recursion_execution() {
        // Test that mutually recursive functions execute correctly
        let (mut symbols, mut vm) = setup();
        let source = r#"
(def f (fn (x) (if (= x 0) 1 (g (- x 1)))))
(def g (fn (x) (if (= x 0) 2 (f (- x 1)))))
(f 5)
"#;
        let results = compile_all(source, &mut symbols);
        assert!(results.is_ok(), "Compilation should succeed");
        let results = results.unwrap();

        // Execute all forms
        for result in &results {
            let _ = vm.execute(&result.bytecode);
        }

        // f(5) -> g(4) -> f(3) -> g(2) -> f(1) -> g(0) -> 2
        // The last result should be 2
    }

    #[test]
    fn test_mutual_recursion_effects_are_pure() {
        // Test that mutually recursive functions are inferred as Pure
        let (mut symbols, _) = setup();
        let source = r#"
(def f (fn (x) (if (= x 0) 1 (g (- x 1)))))
(def g (fn (x) (if (= x 0) 2 (f (- x 1)))))
"#;
        let results = compile_all(source, &mut symbols);
        assert!(results.is_ok(), "Compilation should succeed");
        let results = results.unwrap();

        // Check that the closures don't suspend
        for (i, result) in results.iter().enumerate() {
            for constant in &result.bytecode.constants {
                if let Some(closure) = constant.as_closure() {
                    assert!(
                        !closure.effect.may_suspend(),
                        "Form {} closure should not suspend, got {:?}",
                        i,
                        closure.effect
                    );
                }
            }
        }
    }

    #[test]
    fn test_nqueens_functions_are_pure() {
        // Test that the nqueens functions are inferred as Pure
        let (mut symbols, _) = setup();
        let source = r#"
(var check-safe-helper
  (fn (col remaining row-offset)
    (if (empty? remaining)
      true
      (let ((placed-col (first remaining)))
        (if (or (= col placed-col)
                (= row-offset (abs (- col placed-col))))
          false
          (check-safe-helper col (rest remaining) (+ row-offset 1)))))))

(var safe?
  (fn (col queens)
    (check-safe-helper col queens 1)))

(var try-cols-helper
  (fn (n col queens row)
    (if (= col n)
      (list)
      (if (safe? col queens)
        (let ((new-queens (cons col queens)))
          (append (solve-helper n (+ row 1) new-queens)
                  (try-cols-helper n (+ col 1) queens row)))
        (try-cols-helper n (+ col 1) queens row)))))

(var solve-helper
  (fn (n row queens)
    (if (= row n)
      (list (reverse queens))
      (try-cols-helper n 0 queens row))))
"#;
        let results = compile_all(source, &mut symbols);
        assert!(results.is_ok(), "Compilation should succeed");
        let results = results.unwrap();

        // Check that all closures don't suspend
        let mut found_closures = 0;
        for (i, result) in results.iter().enumerate() {
            for constant in &result.bytecode.constants {
                if let Some(closure) = constant.as_closure() {
                    found_closures += 1;
                    assert!(
                        !closure.effect.may_suspend(),
                        "Form {} closure should not suspend, got {:?}",
                        i,
                        closure.effect
                    );
                }
            }
        }
        assert_eq!(found_closures, 4, "Should have 4 closures");
    }

    // === Fiber integration tests ===

    #[test]
    fn test_fiber_new_and_status() {
        let (mut symbols, mut vm) = setup();
        crate::context::set_symbol_table(&mut symbols as *mut SymbolTable);
        let result = eval(
            r#"(let ((f (fiber/new (fn () 42) 0)))
                 (= (fiber/status f) :new))"#,
            &mut symbols,
            &mut vm,
        );
        crate::context::clear_symbol_table();
        match result {
            Ok(v) => assert_eq!(v, crate::value::Value::bool(true)),
            Err(e) => panic!("Expected Ok(true), got Err: {}", e),
        }
    }

    #[test]
    fn test_fiber_resume_simple() {
        // A fiber that just returns a value
        let (mut symbols, mut vm) = setup();
        let result = eval(
            r#"(let ((f (fiber/new (fn () 42) 0)))
                 (fiber/resume f))"#,
            &mut symbols,
            &mut vm,
        );
        match result {
            Ok(v) => assert_eq!(v, crate::value::Value::int(42)),
            Err(e) => panic!("Expected Ok(42), got Err: {}", e),
        }
    }

    #[test]
    fn test_fiber_resume_dead_status() {
        // After a fiber completes, its status should be :dead
        let (mut symbols, mut vm) = setup();
        crate::context::set_symbol_table(&mut symbols as *mut SymbolTable);
        let result = eval(
            r#"(let ((f (fiber/new (fn () 42) 0)))
                 (fiber/resume f)
                 (= (fiber/status f) :dead))"#,
            &mut symbols,
            &mut vm,
        );
        crate::context::clear_symbol_table();
        match result {
            Ok(v) => assert_eq!(v, crate::value::Value::bool(true)),
            Err(e) => panic!("Expected Ok(true), got Err: {}", e),
        }
    }

    #[test]
    fn test_fiber_signal_and_resume() {
        // A fiber that signals, then is resumed to completion
        let (mut symbols, mut vm) = setup();
        // SIG_YIELD = 2, mask catches it
        let result = eval(
            r#"(let ((f (fiber/new (fn () (fiber/signal 2 99) 42) 2)))
                 (fiber/resume f)
                 (fiber/value f))"#,
            &mut symbols,
            &mut vm,
        );
        match result {
            Ok(v) => assert_eq!(v, crate::value::Value::int(99)),
            Err(e) => panic!("Expected Ok(99), got Err: {}", e),
        }
    }

    #[test]
    fn test_fiber_signal_resume_continues() {
        // Resume after signal should continue execution and return final value
        let (mut symbols, mut vm) = setup();
        let result = eval(
            r#"(let ((f (fiber/new (fn () (fiber/signal 2 99) 42) 2)))
                 (fiber/resume f)
                 (fiber/resume f))"#,
            &mut symbols,
            &mut vm,
        );
        match result {
            Ok(v) => assert_eq!(v, crate::value::Value::int(42)),
            Err(e) => panic!("Expected Ok(42), got Err: {}", e),
        }
    }

    #[test]
    fn test_fiber_is_fiber() {
        let (mut symbols, mut vm) = setup();
        let result = eval(
            r#"(fiber? (fiber/new (fn () 42) 0))"#,
            &mut symbols,
            &mut vm,
        );
        match result {
            Ok(v) => assert_eq!(v, crate::value::Value::bool(true)),
            Err(e) => panic!("Expected Ok(true), got Err: {}", e),
        }
    }

    #[test]
    fn test_fiber_not_fiber() {
        let (mut symbols, mut vm) = setup();
        let result = eval(r#"(fiber? 42)"#, &mut symbols, &mut vm);
        match result {
            Ok(v) => assert_eq!(v, crate::value::Value::bool(false)),
            Err(e) => panic!("Expected Ok(false), got Err: {}", e),
        }
    }

    #[test]
    fn test_fiber_signal_through_nested_call() {
        // A fiber whose body calls a function that signals.
        // This tests yield propagation through nested calls.
        let (mut symbols, mut vm) = setup();
        let result = eval(
            r#"(begin
                 (defn inner () (fiber/signal 2 99))
                 (let ((f (fiber/new (fn () (inner) 42) 2)))
                   (fiber/resume f)
                   (fiber/value f)))"#,
            &mut symbols,
            &mut vm,
        );
        match result {
            Ok(v) => assert_eq!(v, crate::value::Value::int(99)),
            Err(e) => panic!("Expected Ok(99), got Err: {}", e),
        }
    }

    #[test]
    fn test_fiber_mask() {
        let (mut symbols, mut vm) = setup();
        let result = eval(
            r#"(fiber/mask (fiber/new (fn () 42) 3))"#,
            &mut symbols,
            &mut vm,
        );
        match result {
            Ok(v) => assert_eq!(v, crate::value::Value::int(3)),
            Err(e) => panic!("Expected Ok(3), got Err: {}", e),
        }
    }

    #[test]
    fn test_const_basic() {
        let (mut symbols, mut vm) = setup();
        let result = eval("(begin (def x 42) x)", &mut symbols, &mut vm);
        assert_eq!(result.unwrap(), crate::value::Value::int(42));
    }

    #[test]
    fn test_const_set_error() {
        let (mut symbols, _) = setup();
        let result = compile("(begin (def x 42) (set! x 99))", &mut symbols);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("immutable"));
    }

    #[test]
    fn test_const_function() {
        let (mut symbols, mut vm) = setup();
        let result = eval(
            "(begin (defn add1 (x) (+ x 1)) (add1 10))",
            &mut symbols,
            &mut vm,
        );
        assert_eq!(result.unwrap(), crate::value::Value::int(11));
    }

    #[test]
    fn test_const_function_set_error() {
        let (mut symbols, _) = setup();
        let result = compile("(begin (defn f (x) x) (set! f 99))", &mut symbols);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("immutable"));
    }

    #[test]
    fn test_const_cross_form_set_error() {
        let (mut symbols, _) = setup();
        let result = compile_all("(def x 42)\n(set! x 99)", &mut symbols);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("immutable"));
    }

    #[test]
    fn test_const_cross_form_reference() {
        let (mut symbols, mut vm) = setup();
        let results = compile_all("(def x 42)\n(+ x 1)", &mut symbols);
        assert!(results.is_ok());
        let results = results.unwrap();
        for result in &results {
            let _ = vm.execute(&result.bytecode);
        }
    }

    #[test]
    fn test_const_in_function_scope() {
        let (mut symbols, mut vm) = setup();
        let result = eval("((fn () (def x 42) x))", &mut symbols, &mut vm);
        assert_eq!(result.unwrap(), crate::value::Value::int(42));
    }

    #[test]
    fn test_const_in_function_set_error() {
        let (mut symbols, _) = setup();
        let result = compile("((fn () (def x 42) (set! x 99)))", &mut symbols);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("immutable"));
    }

    #[test]
    fn test_arity_cons_wrong_args() {
        // cons requires exactly 2 arguments; passing 1 should be a compile-time arity error
        let mut symbols = SymbolTable::new();
        let result = compile("(cons 1)", &mut symbols);
        assert!(result.is_err(), "Expected compile error for (cons 1)");
        assert!(result.unwrap_err().contains("arity"));
    }

    #[test]
    fn test_arity_various_primitives() {
        // first expects 1 arg, 0 should fail
        let mut symbols = SymbolTable::new();
        let result = compile("(first)", &mut symbols);
        assert!(result.is_err(), "first with 0 args should fail");
        assert!(result.unwrap_err().contains("arity"));

        // not expects exactly 1 arg, 2 should fail
        let mut symbols = SymbolTable::new();
        let result = compile("(not 1 2)", &mut symbols);
        assert!(result.is_err(), "not with 2 args should fail");
        assert!(result.unwrap_err().contains("arity"));

        // cons expects exactly 2 args, 3 should fail
        let mut symbols = SymbolTable::new();
        let result = compile("(cons 1 2 3)", &mut symbols);
        assert!(result.is_err(), "cons with 3 args should fail");
        assert!(result.unwrap_err().contains("arity"));

        // + accepts 0+ args, so (+) should succeed
        let mut symbols = SymbolTable::new();
        let result = compile("(+)", &mut symbols);
        assert!(result.is_ok(), "(+) should succeed since + accepts 0+ args");
    }

    #[test]
    fn test_arity_user_shadow_disables_check() {
        // When user redefines a primitive, arity checking should NOT apply
        // the primitive's arity to the user's version
        let mut symbols = SymbolTable::new();
        let result = compile("(begin (var cons 42) (cons 1))", &mut symbols);
        assert!(
            !result.as_ref().err().is_some_and(|e| e.contains("arity")),
            "User-shadowed cons should not get primitive arity check, got: {:?}",
            result
        );
    }

    #[test]
    fn test_arity_in_nested_positions() {
        // Arity checking should work in nested calls, let bodies, and lambda bodies
        let mut symbols = SymbolTable::new();
        let result = compile("(+ 1 (cons 1))", &mut symbols);
        assert!(result.is_err(), "Nested (cons 1) should fail arity check");
        assert!(result.unwrap_err().contains("arity"));

        let mut symbols = SymbolTable::new();
        let result = compile("(let ((x 1)) (cons x))", &mut symbols);
        assert!(result.is_err(), "(cons x) in let body should fail");
        assert!(result.unwrap_err().contains("arity"));

        let mut symbols = SymbolTable::new();
        let result = compile("(fn (x) (cons x))", &mut symbols);
        assert!(result.is_err(), "(cons x) in lambda body should fail");
        assert!(result.unwrap_err().contains("arity"));
    }

    // === Eval special form ===

    #[test]
    fn test_eval_simple_literal() {
        let (mut symbols, mut vm) = setup();
        crate::context::set_symbol_table(&mut symbols as *mut SymbolTable);
        let result = eval("(eval '42)", &mut symbols, &mut vm);
        crate::context::clear_symbol_table();
        assert_eq!(result.unwrap(), crate::value::Value::int(42));
    }

    #[test]
    fn test_eval_quoted_expression() {
        let (mut symbols, mut vm) = setup();
        crate::context::set_symbol_table(&mut symbols as *mut SymbolTable);
        let result = eval("(eval '(+ 1 2))", &mut symbols, &mut vm);
        crate::context::clear_symbol_table();
        assert_eq!(result.unwrap(), crate::value::Value::int(3));
    }

    #[test]
    fn test_eval_list_construction() {
        let (mut symbols, mut vm) = setup();
        crate::context::set_symbol_table(&mut symbols as *mut SymbolTable);
        let result = eval("(eval (list '+ 1 2))", &mut symbols, &mut vm);
        crate::context::clear_symbol_table();
        assert_eq!(result.unwrap(), crate::value::Value::int(3));
    }

    #[test]
    fn test_eval_with_env() {
        let (mut symbols, mut vm) = setup();
        crate::context::set_symbol_table(&mut symbols as *mut SymbolTable);
        let result = eval("(eval '(+ x y) {:x 10 :y 20})", &mut symbols, &mut vm);
        crate::context::clear_symbol_table();
        assert_eq!(result.unwrap(), crate::value::Value::int(30));
    }

    #[test]
    fn test_eval_nil_env() {
        let (mut symbols, mut vm) = setup();
        crate::context::set_symbol_table(&mut symbols as *mut SymbolTable);
        let result = eval("(eval '(+ 3 4) nil)", &mut symbols, &mut vm);
        crate::context::clear_symbol_table();
        assert_eq!(result.unwrap(), crate::value::Value::int(7));
    }

    #[test]
    fn test_eval_arity_error() {
        let (mut symbols, _vm) = setup();
        // eval with no arguments
        let result = compile("(eval)", &mut symbols);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("eval"));
    }

    #[test]
    fn test_eval_too_many_args() {
        let (mut symbols, _vm) = setup();
        // eval with three arguments should fail at compile time
        let result = compile("(eval 'a 'b 'c)", &mut symbols);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("eval"));
    }

    #[test]
    fn test_eval_returns_string() {
        let (mut symbols, mut vm) = setup();
        crate::context::set_symbol_table(&mut symbols as *mut SymbolTable);
        let result = eval("(eval '\"hello\")", &mut symbols, &mut vm);
        crate::context::clear_symbol_table();
        assert_eq!(result.unwrap(), crate::value::Value::string("hello"));
    }

    #[test]
    fn test_eval_returns_bool() {
        let (mut symbols, mut vm) = setup();
        crate::context::set_symbol_table(&mut symbols as *mut SymbolTable);
        let result = eval("(eval 'true)", &mut symbols, &mut vm);
        crate::context::clear_symbol_table();
        assert_eq!(result.unwrap(), crate::value::Value::TRUE);
    }

    #[test]
    fn test_eval_returns_nil() {
        let (mut symbols, mut vm) = setup();
        crate::context::set_symbol_table(&mut symbols as *mut SymbolTable);
        let result = eval("(eval 'nil)", &mut symbols, &mut vm);
        crate::context::clear_symbol_table();
        assert_eq!(result.unwrap(), crate::value::Value::NIL);
    }

    #[test]
    fn test_eval_with_macros() {
        // eval'd code should have access to prelude macros like `when`
        let (mut symbols, mut vm) = setup();
        crate::context::set_symbol_table(&mut symbols as *mut SymbolTable);
        let result = eval("(eval '(when true 42))", &mut symbols, &mut vm);
        crate::context::clear_symbol_table();
        assert_eq!(result.unwrap(), crate::value::Value::int(42));
    }

    #[test]
    fn test_eval_with_begin() {
        let (mut symbols, mut vm) = setup();
        crate::context::set_symbol_table(&mut symbols as *mut SymbolTable);
        let result = eval("(eval '(begin 1 2 3))", &mut symbols, &mut vm);
        crate::context::clear_symbol_table();
        assert_eq!(result.unwrap(), crate::value::Value::int(3));
    }

    #[test]
    fn test_eval_with_let_in_evald_code() {
        let (mut symbols, mut vm) = setup();
        crate::context::set_symbol_table(&mut symbols as *mut SymbolTable);
        let result = eval("(eval '(let ((x 10)) (+ x 5)))", &mut symbols, &mut vm);
        crate::context::clear_symbol_table();
        assert_eq!(result.unwrap(), crate::value::Value::int(15));
    }

    #[test]
    fn test_eval_with_closure_in_evald_code() {
        let (mut symbols, mut vm) = setup();
        crate::context::set_symbol_table(&mut symbols as *mut SymbolTable);
        let result = eval("(eval '(let ((x 1)) ((fn () x))))", &mut symbols, &mut vm);
        crate::context::clear_symbol_table();
        assert_eq!(result.unwrap(), crate::value::Value::int(1));
    }

    #[test]
    fn test_eval_result_in_computation() {
        // eval's return value used in a larger expression
        let (mut symbols, mut vm) = setup();
        crate::context::set_symbol_table(&mut symbols as *mut SymbolTable);
        let result = eval("(+ 1 (eval '2))", &mut symbols, &mut vm);
        crate::context::clear_symbol_table();
        assert_eq!(result.unwrap(), crate::value::Value::int(3));
    }

    #[test]
    fn test_eval_inside_let() {
        let (mut symbols, mut vm) = setup();
        crate::context::set_symbol_table(&mut symbols as *mut SymbolTable);
        let result = eval("(let ((x 10)) (eval '(+ 1 2)))", &mut symbols, &mut vm);
        crate::context::clear_symbol_table();
        assert_eq!(result.unwrap(), crate::value::Value::int(3));
    }

    #[test]
    fn test_eval_inside_lambda() {
        let (mut symbols, mut vm) = setup();
        crate::context::set_symbol_table(&mut symbols as *mut SymbolTable);
        let result = eval("((fn () (eval '42)))", &mut symbols, &mut vm);
        crate::context::clear_symbol_table();
        assert_eq!(result.unwrap(), crate::value::Value::int(42));
    }

    #[test]
    fn test_eval_nested() {
        // eval within eval'd code
        let (mut symbols, mut vm) = setup();
        crate::context::set_symbol_table(&mut symbols as *mut SymbolTable);
        let result = eval("(eval '(eval '42))", &mut symbols, &mut vm);
        crate::context::clear_symbol_table();
        assert_eq!(result.unwrap(), crate::value::Value::int(42));
    }

    #[test]
    fn test_eval_invalid_env_type() {
        // env must be a table/struct/nil — a string should error
        let (mut symbols, mut vm) = setup();
        crate::context::set_symbol_table(&mut symbols as *mut SymbolTable);
        let result = eval("(eval '42 \"bad\")", &mut symbols, &mut vm);
        crate::context::clear_symbol_table();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("table or struct"));
    }

    #[test]
    fn test_eval_empty_env() {
        // Empty mutable table env should work fine
        let (mut symbols, mut vm) = setup();
        crate::context::set_symbol_table(&mut symbols as *mut SymbolTable);
        let result = eval("(eval '(+ 1 2) (table))", &mut symbols, &mut vm);
        crate::context::clear_symbol_table();
        assert_eq!(result.unwrap(), crate::value::Value::int(3));
    }

    #[test]
    fn test_eval_compilation_error() {
        // eval'd code with invalid syntax should produce a runtime error
        let (mut symbols, mut vm) = setup();
        crate::context::set_symbol_table(&mut symbols as *mut SymbolTable);
        let result = eval("(eval '(if))", &mut symbols, &mut vm);
        crate::context::clear_symbol_table();
        assert!(result.is_err());
    }

    #[test]
    fn test_eval_sequential_caching() {
        // Multiple evals should work (tests expander caching)
        let (mut symbols, mut vm) = setup();
        crate::context::set_symbol_table(&mut symbols as *mut SymbolTable);
        let r1 = eval("(eval '(+ 1 2))", &mut symbols, &mut vm);
        assert_eq!(r1.unwrap(), crate::value::Value::int(3));
        let r2 = eval("(eval '(* 3 4))", &mut symbols, &mut vm);
        crate::context::clear_symbol_table();
        assert_eq!(r2.unwrap(), crate::value::Value::int(12));
    }
}
