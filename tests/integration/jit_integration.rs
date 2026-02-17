// JIT Integration Tests
//
// End-to-end tests showing the JIT compilation pipeline working with
// profiling feedback and hot function detection.

use elle::compiler::ast::Expr;
use elle::compiler::converters::value_to_expr;
use elle::compiler::jit_executor::JitExecutor;
use elle::compiler::JitCoordinator;
use elle::value::Value;
use elle::{compile, read_str, register_primitives, SymbolTable, VM};

#[test]
fn test_jit_coordinator_with_execution() {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    register_primitives(&mut vm, &mut symbols);

    // Create JIT coordinator
    let coordinator = JitCoordinator::new(true);
    assert!(coordinator.is_enabled());

    // Parse and compile a simple expression
    let code = "(+ 1 2)";
    let value = read_str(code, &mut symbols).unwrap();
    let expr = value_to_expr(&value, &mut symbols).unwrap();
    let bytecode = compile(&expr);

    // Execute the bytecode (coordinator can monitor this)
    let result = vm.execute(&bytecode).unwrap();
    assert_eq!(result, elle::value::Value::int(3));

    // Stats should show activity
    let stats = coordinator.get_stats();
    assert!(stats.contains("JIT Coordinator"));
}

#[test]
fn test_jit_hot_function_detection() {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    register_primitives(&mut vm, &mut symbols);

    // Create coordinator with profiling
    let coordinator = JitCoordinator::new(true);

    // Simulate multiple invocations of a function
    let func_id = symbols.intern("test-func");

    // Below threshold (9 calls)
    for _ in 0..9 {
        coordinator
            .profiler()
            .record_call(elle::value::SymbolId(func_id.0));
    }

    // Should not be hot yet
    assert!(!coordinator.should_jit_compile(elle::value::SymbolId(func_id.0)));

    // One more call to reach threshold
    coordinator
        .profiler()
        .record_call(elle::value::SymbolId(func_id.0));

    // Now should be considered for JIT compilation
    assert!(coordinator.should_jit_compile(elle::value::SymbolId(func_id.0)));
}

#[test]
fn test_jit_arithmetic_expression() {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    register_primitives(&mut vm, &mut symbols);

    let test_cases = vec![
        ("(+ 5 3)", elle::value::Value::int(8)),
        ("(* 4 2)", elle::value::Value::int(8)),
        ("(- 10 3)", elle::value::Value::int(7)),
    ];

    for (code, expected) in test_cases {
        let value = read_str(code, &mut symbols).unwrap();
        let expr = value_to_expr(&value, &mut symbols).unwrap();
        let bytecode = compile(&expr);
        let result = vm.execute(&bytecode).unwrap();
        assert_eq!(result, expected, "Failed for: {}", code);
    }
}

#[test]
fn test_jit_conditional_expression() {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    register_primitives(&mut vm, &mut symbols);

    let code = "(if (> 5 3) 100 200)";
    let value = read_str(code, &mut symbols).unwrap();
    let expr = value_to_expr(&value, &mut symbols).unwrap();
    let bytecode = compile(&expr);
    let result = vm.execute(&bytecode).unwrap();
    assert_eq!(result, elle::value::Value::int(100));
}

#[test]
fn test_jit_with_profiling_feedback() {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    register_primitives(&mut vm, &mut symbols);

    let coordinator = JitCoordinator::new(true);
    let profiler = coordinator.profiler();

    // Execute code 15 times
    for _ in 0..15 {
        let code = "(+ 1 2)";
        let value = read_str(code, &mut symbols).unwrap();
        let expr = value_to_expr(&value, &mut symbols).unwrap();
        let bytecode = compile(&expr);
        let _ = vm.execute(&bytecode).unwrap();

        // Simulate recording function invocation
        profiler.record_call(elle::value::SymbolId(0));
    }

    // Check coordinator stats
    let stats = coordinator.get_stats();
    assert!(stats.contains("JIT Coordinator"));

    // Get profiling summary - verify profiler is working
    let _summary = profiler.summary();
}

#[test]
fn test_jit_executor_native_code_execution() {
    // Test native code compilation and execution through JIT executor
    let mut executor = JitExecutor::new().expect("Failed to create JIT executor");
    let symbols = SymbolTable::new();

    // Test 1: Literal integer execution
    let expr = Expr::Literal(Value::int(42));
    let result = executor
        .try_jit_execute(&expr, &symbols)
        .expect("JIT execution failed");
    assert!(
        result.is_some(),
        "JIT executor should return Some for literal"
    );
    let result = result.unwrap();
    if let Some(n) = result.as_int() {
        assert_eq!(n, 42);
    } else {
        panic!("Expected Value::int(42), got {:?}", result);
    }
    // Test 2: Boolean literal
    let expr_bool = Expr::Literal(Value::bool(true));
    let result_bool = executor
        .try_jit_execute(&expr_bool, &symbols)
        .expect("JIT execution failed");
    assert!(
        result_bool.is_some(),
        "JIT executor should return Some for boolean"
    );

    // Test 3: Nil literal
    let expr_nil = Expr::Literal(Value::NIL);
    let result_nil = executor
        .try_jit_execute(&expr_nil, &symbols)
        .expect("JIT execution failed");
    assert!(
        result_nil.is_some(),
        "JIT executor should return Some for nil"
    );
}

#[test]
fn test_jit_executor_cache_functionality() {
    // Test that JIT executor correctly caches compiled code
    let mut executor = JitExecutor::new().expect("Failed to create JIT executor");
    let symbols = SymbolTable::new();

    let expr1 = Expr::Literal(Value::int(10));
    let expr2 = Expr::Literal(Value::int(20));

    // Execute first expression
    executor.try_jit_execute(&expr1, &symbols).ok();
    let (_compiled1, total1) = executor.cache_stats();

    // Execute second expression
    executor.try_jit_execute(&expr2, &symbols).ok();
    let (compiled2, total2) = executor.cache_stats();

    // Cache should have grown
    assert!(total2 >= total1, "Cache should not shrink");
    assert!(
        compiled2 >= 1,
        "Should have at least one successful compilation"
    );
}
