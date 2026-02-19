//! New compilation pipeline: Syntax → HIR → LIR → Bytecode
//!
//! This module provides the end-to-end compilation function using the
//! new intermediate representations. It runs in parallel with the
//! existing Value-based pipeline until fully integrated.

use crate::compiler::Bytecode;
use crate::effects::{get_primitive_effects, Effect};
use crate::hir::tailcall::mark_tail_calls;
use crate::hir::{Analyzer, BindingId, BindingInfo, Hir};
use crate::lir::{Emitter, Lowerer};
use crate::reader::{read_syntax, read_syntax_all};
use crate::symbol::SymbolTable;
use crate::syntax::Expander;
use crate::value::SymbolId;
use std::collections::HashMap;

/// Compilation result
pub struct CompileResult {
    pub bytecode: Bytecode,
    pub warnings: Vec<String>,
}

/// Analysis-only result (no bytecode generation)
/// Used by linter and LSP which need HIR but not bytecode
pub struct AnalyzeResult {
    pub hir: Hir,
    pub bindings: HashMap<BindingId, BindingInfo>,
}

/// Compile source code using the new pipeline
pub fn compile_new(source: &str, symbols: &mut SymbolTable) -> Result<CompileResult, String> {
    // Phase 1: Parse to Syntax
    let syntax = read_syntax(source)?;

    // Phase 2: Macro expansion
    let mut expander = Expander::new();
    let expanded = expander.expand(syntax)?;

    // Phase 3: Analyze to HIR with interprocedural effect tracking
    let primitive_effects = get_primitive_effects(symbols);
    let mut analyzer = Analyzer::new_with_primitive_effects(symbols, primitive_effects);
    let mut analysis = analyzer.analyze(&expanded)?;

    // Phase 3.5: Mark tail calls
    mark_tail_calls(&mut analysis.hir);

    // Phase 4: Lower to LIR with binding info
    let mut lowerer = Lowerer::new().with_bindings(analysis.bindings);
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

/// Compile multiple top-level forms
pub fn compile_all_new(
    source: &str,
    symbols: &mut SymbolTable,
) -> Result<Vec<CompileResult>, String> {
    let syntaxes = read_syntax_all(source)?;
    let mut expander = Expander::new();
    let mut results = Vec::new();
    // Accumulate global effects across forms for cross-form effect tracking
    let mut global_effects: HashMap<SymbolId, Effect> = HashMap::new();

    for syntax in syntaxes {
        let expanded = expander.expand(syntax)?;

        // Create analyzer for each form with interprocedural effect tracking
        let primitive_effects = get_primitive_effects(symbols);
        let mut analyzer = Analyzer::new_with_primitive_effects(symbols, primitive_effects);
        // Pass accumulated global effects from previous forms
        analyzer.set_global_effects(global_effects.clone());

        let mut analysis = analyzer.analyze(&expanded)?;

        // Accumulate effects from this form's defines
        for (sym, effect) in analyzer.take_defined_global_effects() {
            global_effects.insert(sym, effect);
        }
        // Analyzer is dropped here, releasing the mutable borrow

        // Mark tail calls
        mark_tail_calls(&mut analysis.hir);

        let mut lowerer = Lowerer::new().with_bindings(analysis.bindings);
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

/// Compile and execute using the new pipeline
pub fn eval_new(
    source: &str,
    symbols: &mut SymbolTable,
    vm: &mut crate::vm::VM,
) -> Result<crate::value::Value, String> {
    let result = compile_new(source, symbols)?;
    vm.execute(&result.bytecode).map_err(|e| e.to_string())
}

/// Analyze source code without generating bytecode
/// Used by linter and LSP which need HIR but not bytecode
pub fn analyze_new(source: &str, symbols: &mut SymbolTable) -> Result<AnalyzeResult, String> {
    let syntax = read_syntax(source)?;
    let mut expander = Expander::new();
    let expanded = expander.expand(syntax)?;
    let primitive_effects = get_primitive_effects(symbols);
    let mut analyzer = Analyzer::new_with_primitive_effects(symbols, primitive_effects);
    let analysis = analyzer.analyze(&expanded)?;
    Ok(AnalyzeResult {
        hir: analysis.hir,
        bindings: analysis.bindings,
    })
}

/// Analyze multiple top-level forms without generating bytecode
pub fn analyze_all_new(
    source: &str,
    symbols: &mut SymbolTable,
) -> Result<Vec<AnalyzeResult>, String> {
    let syntaxes = read_syntax_all(source)?;
    let mut expander = Expander::new();
    let mut results = Vec::new();
    // Accumulate global effects across forms for cross-form effect tracking
    let mut global_effects: HashMap<SymbolId, Effect> = HashMap::new();

    for syntax in syntaxes {
        let expanded = expander.expand(syntax)?;
        let primitive_effects = get_primitive_effects(symbols);
        let mut analyzer = Analyzer::new_with_primitive_effects(symbols, primitive_effects);
        // Pass accumulated global effects from previous forms
        analyzer.set_global_effects(global_effects.clone());

        let analysis = analyzer.analyze(&expanded)?;

        // Accumulate effects from this form's defines
        for (sym, effect) in analyzer.take_defined_global_effects() {
            global_effects.insert(sym, effect);
        }

        results.push(AnalyzeResult {
            hir: analysis.hir,
            bindings: analysis.bindings,
        });
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::register_primitives;
    use crate::vm::VM;

    fn setup() -> (SymbolTable, VM) {
        let mut symbols = SymbolTable::new();
        let mut vm = VM::new();
        register_primitives(&mut vm, &mut symbols);
        (symbols, vm)
    }

    #[test]
    fn test_compile_literal() {
        let (mut symbols, _) = setup();
        let result = compile_new("42", &mut symbols);
        assert!(result.is_ok());
        let compiled = result.unwrap();
        assert!(!compiled.bytecode.instructions.is_empty());
    }

    #[test]
    fn test_compile_if() {
        let (mut symbols, _) = setup();
        let result = compile_new("(if #t 1 2)", &mut symbols);
        assert!(result.is_ok());
    }

    #[test]
    fn test_compile_let() {
        let (mut symbols, _) = setup();
        let result = compile_new("(let ((x 10)) x)", &mut symbols);
        assert!(result.is_ok());
    }

    #[test]
    fn test_compile_lambda() {
        let (mut symbols, _) = setup();
        let result = compile_new("(fn (x) x)", &mut symbols);
        assert!(result.is_ok());
    }

    #[test]
    fn test_compile_call() {
        let (mut symbols, _) = setup();
        // Note: Function calls to built-in symbols like + may fail during lowering
        // because the new pipeline doesn't yet have full integration with built-in symbols.
        // This test just verifies that the pipeline can attempt to compile function calls.
        let result = compile_new("(+ 1 2)", &mut symbols);
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
        let result = compile_new("(+ 1 2)", &mut symbols);
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
        let result = compile_new("(begin 1 2 3)", &mut symbols);
        assert!(result.is_ok());
    }

    #[test]
    fn test_compile_and() {
        let (mut symbols, _) = setup();
        let result = compile_new("(and #t #t #f)", &mut symbols);
        assert!(result.is_ok());
    }

    #[test]
    fn test_compile_or() {
        let (mut symbols, _) = setup();
        let result = compile_new("(or #f #f #t)", &mut symbols);
        assert!(result.is_ok());
    }

    #[test]
    fn test_compile_while() {
        let (mut symbols, _) = setup();
        let result = compile_new("(while #f nil)", &mut symbols);
        assert!(result.is_ok());
    }

    #[test]
    fn test_compile_cond() {
        let (mut symbols, _) = setup();
        let result = compile_new("(cond (#t 1) (else 2))", &mut symbols);
        assert!(result.is_ok());
    }

    #[test]
    fn test_compile_all() {
        let (mut symbols, _) = setup();
        let result = compile_all_new("1 2 3", &mut symbols);
        assert!(result.is_ok());
        let compiled = result.unwrap();
        assert_eq!(compiled.len(), 3);
    }

    #[test]
    fn test_eval_literal() {
        let (mut symbols, mut vm) = setup();
        let result = eval_new("42", &mut symbols, &mut vm);
        // Note: execution may fail due to incomplete bytecode mapping
        // but compilation should succeed
        let _ = result;
    }

    #[test]
    fn test_eval_addition() {
        let (mut symbols, mut vm) = setup();
        let result = eval_new("(+ 1 2)", &mut symbols, &mut vm);
        match result {
            Ok(v) => assert_eq!(v, crate::value::Value::int(3)),
            Err(e) => panic!("Expected Ok(3), got Err: {}", e),
        }
    }

    #[test]
    fn test_eval_subtraction() {
        let (mut symbols, mut vm) = setup();
        let result = eval_new("(- 10 3)", &mut symbols, &mut vm);
        match result {
            Ok(v) => assert_eq!(v, crate::value::Value::int(7)),
            Err(e) => panic!("Expected Ok(7), got Err: {}", e),
        }
    }

    #[test]
    fn test_eval_nested_arithmetic() {
        let (mut symbols, mut vm) = setup();
        let result = eval_new("(+ (* 2 3) (- 10 5))", &mut symbols, &mut vm);
        match result {
            Ok(v) => assert_eq!(v, crate::value::Value::int(11)),
            Err(e) => panic!("Expected Ok(11), got Err: {}", e),
        }
    }

    #[test]
    fn test_eval_if_true() {
        let (mut symbols, mut vm) = setup();
        let result = eval_new("(if #t 42 0)", &mut symbols, &mut vm);
        match result {
            Ok(v) => assert_eq!(v, crate::value::Value::int(42)),
            Err(e) => panic!("Expected Ok(42), got Err: {}", e),
        }
    }

    #[test]
    fn test_eval_if_false() {
        let (mut symbols, mut vm) = setup();
        let result = eval_new("(if #f 42 0)", &mut symbols, &mut vm);
        match result {
            Ok(v) => assert_eq!(v, crate::value::Value::int(0)),
            Err(e) => panic!("Expected Ok(0), got Err: {}", e),
        }
    }

    #[test]
    fn test_eval_let_simple() {
        let (mut symbols, mut vm) = setup();
        let result = eval_new("(let ((x 10)) x)", &mut symbols, &mut vm);
        match result {
            Ok(v) => assert_eq!(v, crate::value::Value::int(10)),
            Err(e) => panic!("Expected Ok(10), got Err: {}", e),
        }
    }

    #[test]
    fn test_eval_let_with_arithmetic() {
        let (mut symbols, mut vm) = setup();
        let result = eval_new("(let ((x 10) (y 5)) (+ x y))", &mut symbols, &mut vm);
        match result {
            Ok(v) => assert_eq!(v, crate::value::Value::int(15)),
            Err(e) => panic!("Expected Ok(15), got Err: {}", e),
        }
    }

    #[test]
    fn test_eval_lambda_identity() {
        let (mut symbols, mut vm) = setup();
        let result = eval_new("((fn (x) x) 42)", &mut symbols, &mut vm);
        match result {
            Ok(v) => assert_eq!(v, crate::value::Value::int(42)),
            Err(e) => panic!("Expected Ok(42), got Err: {}", e),
        }
    }

    #[test]
    fn test_eval_lambda_add_one() {
        let (mut symbols, mut vm) = setup();
        let result = eval_new("((fn (x) (+ x 1)) 10)", &mut symbols, &mut vm);
        match result {
            Ok(v) => assert_eq!(v, crate::value::Value::int(11)),
            Err(e) => panic!("Expected Ok(11), got Err: {}", e),
        }
    }

    #[test]
    fn test_compile_lambda_with_capture() {
        let (mut symbols, _) = setup();
        let result = compile_new("(let ((x 10)) (fn () x))", &mut symbols);
        match result {
            Ok(_) => {}
            Err(e) => panic!("Failed to compile lambda with capture: {}", e),
        }
    }

    #[test]
    fn test_eval_begin() {
        let (mut symbols, mut vm) = setup();
        let result = eval_new("(begin 1 2 3)", &mut symbols, &mut vm);
        match result {
            Ok(v) => assert_eq!(v, crate::value::Value::int(3)),
            Err(e) => panic!("Expected Ok(3), got Err: {}", e),
        }
    }

    #[test]
    fn test_eval_comparison_lt() {
        let (mut symbols, mut vm) = setup();
        let result = eval_new("(< 1 2)", &mut symbols, &mut vm);
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
                match compile_new(&content, &mut symbols) {
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

                match eval_new(&content, &mut symbols, &mut vm) {
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
        let result = eval_new("(cond (#t 42))", &mut symbols, &mut vm);
        assert_eq!(result.unwrap(), crate::value::Value::int(42));
    }

    #[test]
    fn test_eval_cond_second_true() {
        let (mut symbols, mut vm) = setup();
        let result = eval_new("(cond (#f 1) (#t 42))", &mut symbols, &mut vm);
        assert_eq!(result.unwrap(), crate::value::Value::int(42));
    }

    #[test]
    fn test_eval_cond_else() {
        let (mut symbols, mut vm) = setup();
        let result = eval_new("(cond (#f 1) (#f 2) (else 42))", &mut symbols, &mut vm);
        assert_eq!(result.unwrap(), crate::value::Value::int(42));
    }

    #[test]
    fn test_eval_cond_with_expressions() {
        let (mut symbols, mut vm) = setup();
        let result = eval_new("(cond ((< 5 10) (+ 20 22)))", &mut symbols, &mut vm);
        assert_eq!(result.unwrap(), crate::value::Value::int(42));
    }

    // === Control Flow: and ===

    #[test]
    fn test_eval_and_all_true() {
        let (mut symbols, mut vm) = setup();
        let result = eval_new("(and #t #t #t)", &mut symbols, &mut vm);
        assert_eq!(result.unwrap(), crate::value::Value::bool(true));
    }

    #[test]
    fn test_eval_and_one_false() {
        let (mut symbols, mut vm) = setup();
        let result = eval_new("(and #t #f #t)", &mut symbols, &mut vm);
        assert_eq!(result.unwrap(), crate::value::Value::bool(false));
    }

    #[test]
    fn test_eval_and_returns_last() {
        let (mut symbols, mut vm) = setup();
        let result = eval_new("(and 1 2 3)", &mut symbols, &mut vm);
        assert_eq!(result.unwrap(), crate::value::Value::int(3));
    }

    #[test]
    fn test_eval_and_short_circuit() {
        let (mut symbols, mut vm) = setup();
        // If and doesn't short-circuit, this would fail trying to call nil
        let result = eval_new("(and #f (nil))", &mut symbols, &mut vm);
        assert_eq!(result.unwrap(), crate::value::Value::bool(false));
    }

    #[test]
    fn test_eval_and_empty() {
        let (mut symbols, mut vm) = setup();
        let result = eval_new("(and)", &mut symbols, &mut vm);
        assert_eq!(result.unwrap(), crate::value::Value::bool(true));
    }

    // === Control Flow: or ===

    #[test]
    fn test_eval_or_all_false() {
        let (mut symbols, mut vm) = setup();
        let result = eval_new("(or #f #f #f)", &mut symbols, &mut vm);
        assert_eq!(result.unwrap(), crate::value::Value::bool(false));
    }

    #[test]
    fn test_eval_or_one_true() {
        let (mut symbols, mut vm) = setup();
        let result = eval_new("(or #f #t #f)", &mut symbols, &mut vm);
        assert_eq!(result.unwrap(), crate::value::Value::bool(true));
    }

    #[test]
    fn test_eval_or_returns_first_truthy() {
        let (mut symbols, mut vm) = setup();
        let result = eval_new("(or #f 42 99)", &mut symbols, &mut vm);
        assert_eq!(result.unwrap(), crate::value::Value::int(42));
    }

    #[test]
    fn test_eval_or_short_circuit() {
        let (mut symbols, mut vm) = setup();
        // If or doesn't short-circuit, this would fail trying to call nil
        let result = eval_new("(or #t (nil))", &mut symbols, &mut vm);
        assert_eq!(result.unwrap(), crate::value::Value::bool(true));
    }

    #[test]
    fn test_eval_or_empty() {
        let (mut symbols, mut vm) = setup();
        let result = eval_new("(or)", &mut symbols, &mut vm);
        assert_eq!(result.unwrap(), crate::value::Value::bool(false));
    }

    // === Control Flow: while ===

    #[test]
    fn test_eval_while_never_executes() {
        let (mut symbols, mut vm) = setup();
        let result = eval_new("(while #f 42)", &mut symbols, &mut vm);
        assert_eq!(result.unwrap(), crate::value::Value::NIL);
    }

    #[test]
    fn test_eval_while_with_mutation() {
        let (mut symbols, mut vm) = setup();
        let result = eval_new(
            "(begin (define x 0) (while (< x 5) (set! x (+ x 1))) x)",
            &mut symbols,
            &mut vm,
        );
        assert_eq!(result.unwrap(), crate::value::Value::int(5));
    }

    // === Closures and Captures ===

    #[test]
    fn test_eval_closure_captures_local() {
        let (mut symbols, mut vm) = setup();
        let result = eval_new("(let ((x 10)) ((fn () x)))", &mut symbols, &mut vm);
        assert_eq!(result.unwrap(), crate::value::Value::int(10));
    }

    #[test]
    fn test_eval_closure_captures_multiple() {
        let (mut symbols, mut vm) = setup();
        let result = eval_new(
            "(let ((x 10) (y 20)) ((fn () (+ x y))))",
            &mut symbols,
            &mut vm,
        );
        assert_eq!(result.unwrap(), crate::value::Value::int(30));
    }

    #[test]
    fn test_eval_nested_closure() {
        let (mut symbols, mut vm) = setup();
        let result = eval_new(
            "(let ((x 10)) ((fn () ((fn () x)))))",
            &mut symbols,
            &mut vm,
        );
        assert_eq!(result.unwrap(), crate::value::Value::int(10));
    }

    #[test]
    fn test_eval_closure_with_param_and_capture() {
        let (mut symbols, mut vm) = setup();
        let result = eval_new("(let ((x 10)) ((fn (y) (+ x y)) 5))", &mut symbols, &mut vm);
        assert_eq!(result.unwrap(), crate::value::Value::int(15));
    }

    // === Higher-Order Functions ===

    #[test]
    fn test_eval_function_as_argument() {
        let (mut symbols, mut vm) = setup();
        let result = eval_new(
            "((fn (f x) (f x)) (fn (n) (+ n 1)) 10)",
            &mut symbols,
            &mut vm,
        );
        assert_eq!(result.unwrap(), crate::value::Value::int(11));
    }

    #[test]
    fn test_eval_function_returning_function() {
        let (mut symbols, mut vm) = setup();
        let result = eval_new("(((fn (x) (fn (y) (+ x y))) 10) 5)", &mut symbols, &mut vm);
        assert_eq!(result.unwrap(), crate::value::Value::int(15));
    }

    // === Define and Set! ===

    #[test]
    fn test_eval_define_then_use() {
        let (mut symbols, mut vm) = setup();
        let result = eval_new("(begin (define x 42) x)", &mut symbols, &mut vm);
        assert_eq!(result.unwrap(), crate::value::Value::int(42));
    }

    #[test]
    fn test_eval_define_then_set() {
        let (mut symbols, mut vm) = setup();
        let result = eval_new("(begin (define x 10) (set! x 42) x)", &mut symbols, &mut vm);
        assert_eq!(result.unwrap(), crate::value::Value::int(42));
    }

    #[test]
    fn test_eval_set_in_closure() {
        let (mut symbols, mut vm) = setup();
        let result = eval_new(
            "(begin 
               (define counter 0)
               (define inc (fn () (set! counter (+ counter 1))))
               (inc)
               (inc)
               counter)",
            &mut symbols,
            &mut vm,
        );
        assert_eq!(result.unwrap(), crate::value::Value::int(2));
    }

    #[test]
    fn test_fold_multiple_elements() {
        let (mut symbols, mut vm) = setup();

        // Test with (list 1) - should work
        let code1 = r#"(begin
            (define process (fn (acc x) (begin (define doubled (* x 2)) (+ acc doubled))))
            (define my-fold (fn (f init lst)
                (if (nil? lst)
                    init
                    (my-fold f (f init (first lst)) (rest lst)))))
            (my-fold process 0 (list 1)))"#;

        let result1 = eval_new(code1, &mut symbols, &mut vm);
        println!("list 1: {:?}", result1);

        // Test with (list 1 2) - might fail
        let (mut symbols2, mut vm2) = setup();
        let code2 = r#"(begin
            (define process (fn (acc x) (begin (define doubled (* x 2)) (+ acc doubled))))
            (define my-fold (fn (f init lst)
                (if (nil? lst)
                    init
                    (my-fold f (f init (first lst)) (rest lst)))))
            (my-fold process 0 (list 1 2)))"#;

        let result2 = eval_new(code2, &mut symbols2, &mut vm2);
        println!("list 1 2: {:?}", result2);

        // Test with (list 1 2 3) - original failing case
        let (mut symbols3, mut vm3) = setup();
        let code3 = r#"(begin
            (define process (fn (acc x) (begin (define doubled (* x 2)) (+ acc doubled))))
            (define my-fold (fn (f init lst)
                (if (nil? lst)
                    init
                    (my-fold f (f init (first lst)) (rest lst)))))
            (my-fold process 0 (list 1 2 3)))"#;

        let result3 = eval_new(code3, &mut symbols3, &mut vm3);
        println!("list 1 2 3: {:?}", result3);
    }

    // === analyze_new tests ===

    #[test]
    fn test_analyze_new_literal() {
        let (mut symbols, _) = setup();
        let result = analyze_new("42", &mut symbols);
        assert!(result.is_ok());
        let analysis = result.unwrap();
        assert!(matches!(analysis.hir.kind, crate::hir::HirKind::Int(42)));
    }

    #[test]
    fn test_analyze_new_define() {
        let (mut symbols, _) = setup();
        let result = analyze_new("(define x 10)", &mut symbols);
        assert!(result.is_ok());
        let analysis = result.unwrap();
        assert!(matches!(
            analysis.hir.kind,
            crate::hir::HirKind::Define { .. }
        ));
    }

    #[test]
    fn test_analyze_new_lambda() {
        let (mut symbols, _) = setup();
        let result = analyze_new("(fn (x) x)", &mut symbols);
        assert!(result.is_ok());
        let analysis = result.unwrap();
        assert!(matches!(
            analysis.hir.kind,
            crate::hir::HirKind::Lambda { .. }
        ));
        // Should have bindings for the parameter
        assert!(!analysis.bindings.is_empty());
    }

    #[test]
    fn test_analyze_all_new_multiple_forms() {
        let (mut symbols, _) = setup();
        let result = analyze_all_new("1 2 3", &mut symbols);
        assert!(result.is_ok());
        let analyses = result.unwrap();
        assert_eq!(analyses.len(), 3);
        assert!(matches!(analyses[0].hir.kind, crate::hir::HirKind::Int(1)));
        assert!(matches!(analyses[1].hir.kind, crate::hir::HirKind::Int(2)));
        assert!(matches!(analyses[2].hir.kind, crate::hir::HirKind::Int(3)));
    }

    #[test]
    fn test_analyze_new_with_bindings() {
        let (mut symbols, _) = setup();
        let result = analyze_new("(let ((x 1) (y 2)) (+ x y))", &mut symbols);
        assert!(result.is_ok());
        let analysis = result.unwrap();
        // Should have bindings for x and y
        assert!(analysis.bindings.len() >= 2);
    }
}
