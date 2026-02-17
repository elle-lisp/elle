// DEFENSE: Integration tests for core compiler modules
// Tests capture_resolution, converters, and analysis modules
use elle::compiler::converters::value_to_expr;
use elle::{compile, init_stdlib, read_str, register_primitives, SymbolTable, Value, VM};

fn eval(input: &str) -> Result<Value, String> {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    register_primitives(&mut vm, &mut symbols);
    init_stdlib(&mut vm, &mut symbols);

    let value = read_str(input, &mut symbols)?;
    let expr = value_to_expr(&value, &mut symbols)?;
    let bytecode = compile(&expr);
    vm.execute(&bytecode)
}

// ============================================================================
// SECTION 1: Capture Resolution Tests
// ============================================================================
// Tests for src/compiler/capture_resolution.rs
// These tests verify that nested lambdas correctly resolve variable captures
// and that capture indices are properly adjusted for environment layout.

#[test]
fn test_simple_closure_capture() {
    // Single variable capture in nested lambda (test closure creation)
    let result = eval("(begin (define x 5) (lambda (y) (+ x y)))");
    assert!(result.is_ok());
    let val = result.unwrap();
    assert!((val).is_closure());
}

#[test]
fn test_closure_captures_defined_variable() {
    // Basic closure capture
    let result = eval("(begin (define x 10) (lambda (y) (+ x y)))");
    assert!(result.is_ok());
    assert!((result.unwrap()).is_closure());
}

#[test]
fn test_closure_captures_multiple_variables() {
    // Multiple variable captures
    let result = eval("(begin (define a 1) (define b 2) (lambda (c) (+ a b c)))");
    assert!(result.is_ok());
    assert!((result.unwrap()).is_closure());
}

#[test]
fn test_nested_lambda_creation() {
    // Double nested lambda creation
    let result = eval("(lambda (x) (lambda (y) (+ x y)))");
    assert!(result.is_ok());
    assert!((result.unwrap()).is_closure());
}

#[test]
fn test_triple_nested_lambda_creation() {
    // Triple nested lambda
    let result = eval("(lambda (a) (lambda (b) (lambda (c) (+ a b c))))");
    assert!(result.is_ok());
    assert!((result.unwrap()).is_closure());
}

#[test]
fn test_capture_in_lambda_body() {
    // Capture referenced in lambda body
    let result = eval("(begin (define shared 100) (lambda (x) (+ x shared)))");
    assert!(result.is_ok());
    assert!((result.unwrap()).is_closure());
}

#[test]
fn test_nested_scope_capture() {
    // Closure in nested scope captures from all outer scopes
    let result = eval(
        "(begin (define outer 100) \
         (begin (define inner 50) \
          (lambda (x) (+ outer inner x))))",
    );
    assert!(result.is_ok());
    assert!((result.unwrap()).is_closure());
}

#[test]
fn test_multiple_lambdas_same_scope() {
    // Multiple lambdas capturing from same scope
    let result = eval(
        "(begin (define shared 100) \
         (begin \
          (define add-shared (lambda (x) (+ x shared))) \
          (define mul-shared (lambda (x) (* x shared))) \
          (list (add-shared 5) (mul-shared 3))))",
    );
    assert!(result.is_ok());
}

#[test]
fn test_lambda_parameter_shadowing() {
    // Parameter names shadow outer scope
    let result = eval("(begin (define x 10) (lambda (x) x))");
    assert!(result.is_ok());
}

// ============================================================================
// SECTION 2: Converter Tests (value_to_expr)
// ============================================================================
// Tests for src/compiler/converters.rs
// These tests verify that various Lisp values are correctly converted to AST expressions

#[test]
fn test_convert_literal_integers() {
    // Integer literals should convert correctly
    assert!(eval("42").is_ok());
    assert!(eval("-100").is_ok());
    assert!(eval("0").is_ok());
}

#[test]
fn test_convert_literal_floats() {
    // Float literals should convert correctly
    let result = eval("3.14");
    assert!(result.is_ok());
    assert!((result.unwrap()).is_float());
}

#[test]
fn test_convert_literal_strings() {
    // String literals should convert correctly
    let result = eval("\"hello\"");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::string("hello"));
}

#[test]
fn test_convert_literal_symbols() {
    // Symbol literals should convert correctly
    let result = eval("'quoted-symbol");
    assert!(result.is_ok());
}

#[test]
fn test_convert_nil() {
    // nil literal should convert correctly
    let result = eval("nil");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::NIL);
}

#[test]
fn test_convert_if_expression() {
    // If expressions should convert correctly
    let result = eval("(if (> 2 1) 42 0)");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_convert_define_binding() {
    // Define bindings should convert correctly
    let result = eval("(begin (define x 10) x)");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::int(10));
}

#[test]
fn test_convert_let_binding() {
    // Let bindings should convert correctly
    let result = eval("(let ((x 10)) (+ x 5))");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::int(15));
}

#[test]
fn test_convert_nested_let() {
    // Nested let bindings should convert correctly
    let result = eval("(let ((x 10)) (let ((y 5)) (+ x y)))");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::int(15));
}

#[test]
fn test_convert_cond_expression() {
    // Cond expressions should convert correctly
    let result = eval("(cond ((= 1 2) 10) ((= 2 2) 20) (else 30))");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::int(20));
}

#[test]
fn test_convert_begin_block() {
    // Begin blocks should convert correctly
    let result = eval("(begin 1 2 3)");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::int(3));
}

#[test]
fn test_convert_nested_function_calls() {
    // Nested function calls should convert correctly
    let result = eval("(+ (* 2 3) (* 4 5))");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::int(26)); // (2*3) + (4*5) = 6 + 20 = 26
}

#[test]
fn test_convert_lambda_expression() {
    // Lambda expressions should convert correctly
    let result = eval("(lambda (x y) (+ x y))");
    assert!(result.is_ok());
    assert!((result.unwrap()).is_closure());
}

#[test]
fn test_convert_complex_expression() {
    // Complex nested expressions should convert correctly
    let result = eval(
        "(let ((x 10)) \
         (if (> x 5) \
          (lambda (y) (+ x y)) \
          (lambda (y) (- y x))))",
    );
    assert!(result.is_ok());
}

#[test]
fn test_convert_list_literal() {
    // List literal should convert correctly
    let result = eval("'(1 2 3)");
    assert!(result.is_ok());
}

#[test]
fn test_convert_function_call() {
    // Function calls should convert to Call expressions
    let result = eval("(+ 1 2)");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::int(3));
}

// ============================================================================
// SECTION 3: Analysis Tests (capture usage analysis)
// ============================================================================
// Tests for src/compiler/analysis.rs
// These tests verify variable usage analysis for dead capture elimination

#[test]
fn test_unused_capture_elimination() {
    // Lambda should work even if outer variable is not used
    let result = eval("(begin (define unused 999) ((lambda () 42)))");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_used_capture_retained() {
    // Lambda should retain capture of used variable
    let result = eval("(begin (define x 100) ((lambda () x)))");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::int(100));
}

#[test]
fn test_partial_capture_analysis() {
    // Analysis should distinguish used vs unused captures
    let result = eval(
        "(begin (define a 10) (define b 20) \
         ((lambda () b)))",
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::int(20));
}

#[test]
fn test_capture_analysis_with_local_bindings() {
    // Variables should not be captured if locally bound
    let result = eval(
        "(begin (define x 100) \
         ((lambda () (let ((x 50)) x))))",
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::int(50)); // Local x shadows outer x
}

#[test]
fn test_capture_analysis_preserves_free_variables() {
    // Free variables should be properly identified
    let result = eval(
        "(begin (define x 100) (define y 200) \
         ((lambda () (+ x y))))",
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::int(300));
}

#[test]
fn test_multiple_uses_single_capture() {
    // Single capture used multiple times
    let result = eval(
        "(begin (define x 5) \
         ((lambda () (+ x x x))))",
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::int(15)); // 5+5+5
}

// ============================================================================
// SECTION 4: Integration Tests (Combined Compiler Phases)
// ============================================================================

#[test]
fn test_complex_closure_pipeline() {
    // Full pipeline: parsing -> conversion -> analysis -> capture resolution -> compilation
    let result = eval(
        "(begin \
         (define make-adder (lambda (x) (lambda (y) (+ x y)))) \
         (define add5 (make-adder 5)) \
         (add5 10))",
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::int(15));
}

#[test]
fn test_higher_order_function_factory() {
    // Higher-order functions with multiple capture levels
    let result = eval(
        "(begin \
         (define make-multiplier (lambda (n) \
          (lambda (x) (* n x)))) \
         (define times3 (make-multiplier 3)) \
         (define times5 (make-multiplier 5)) \
         (list (times3 4) (times5 4)))",
    );
    assert!(result.is_ok());
}

#[test]
fn test_map_with_captured_closure() {
    // Map function with closure capturing outer variable
    let result = eval(
        "(begin \
         (define scale 2) \
         (map (lambda (x) (* x scale)) '(1 2 3 4 5)))",
    );
    assert!(result.is_ok());
}

#[test]
fn test_filter_with_captured_closure() {
    // Filter function with closure capturing outer variable
    let result = eval(
        "(begin \
         (define threshold 3) \
         (filter (lambda (x) (> x threshold)) '(1 2 3 4 5)))",
    );
    assert!(result.is_ok());
}

#[test]
fn test_fold_with_captured_closure() {
    // Fold function with closure capturing outer variable
    let result = eval(
        "(begin \
         (define initial 100) \
         (fold (lambda (acc x) (+ acc x)) initial '(1 2 3 4 5)))",
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::int(115)); // 100 + (1+2+3+4+5)
}

#[test]
fn test_recursive_function_with_closure() {
    // Recursive function interacting with closures
    let result = eval(
        "(begin \
         (define factorial (lambda (n) \
          (if (<= n 1) 1 (* n (factorial (- n 1)))))) \
         (factorial 5))",
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::int(120)); // 5!
}

#[test]
fn test_mutual_recursion_with_closures() {
    // Mutual recursion definitions with closures
    let result = eval(
        "(begin \
         (define even-check (lambda (n) (= n 0))) \
         (define odd-check (lambda (n) (> n 0))) \
         (list (even-check 0) (odd-check 1)))",
    );
    assert!(result.is_ok());
}

#[test]
fn test_closure_capture_in_conditional() {
    // Closure properly captures variables used in conditionals
    let result = eval(
        "(begin (define x 10) \
         ((lambda (y) \
          (if (> y 0) (lambda () x) (lambda () y))) 5))",
    );
    assert!(result.is_ok());
    assert!((result.unwrap()).is_closure());
}

#[test]
fn test_capture_in_nested_conditional() {
    // Captures across nested if expressions
    let result = eval(
        "(begin (define base 5) \
         (lambda (x) \
          (if (> x 0) (+ x base) (- base x))))",
    );
    assert!(result.is_ok());
    assert!((result.unwrap()).is_closure());
}

#[test]
fn test_capture_in_begin_with_defines() {
    // Captures work correctly with begin and define
    let result = eval(
        "(begin (define x 42) \
         (begin \
          (define f (lambda () x)) \
          (f)))",
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_lambda_in_list_context() {
    // Lambdas with captures in list contexts
    let result = eval(
        "(begin (define base 10) \
         (list (lambda (x) (+ x base)) \
               (lambda (x) (* x base))))",
    );
    assert!(result.is_ok());
}

#[test]
fn test_nested_lambda_in_conditional_body() {
    // Nested lambda in conditional branch
    let result = eval("(lambda (x) (if (> x 0) (lambda (y) (+ x y)) (lambda (y) (- x y))))");
    assert!(result.is_ok());
    assert!((result.unwrap()).is_closure());
}

#[test]
fn test_capture_with_multiple_parameters() {
    // Lambdas with multiple parameters and captures
    let result = eval(
        "(begin (define base 5) \
         (lambda (x y z) (+ base x y z)))",
    );
    assert!(result.is_ok());
    assert!((result.unwrap()).is_closure());
}

#[test]
fn test_curried_function_multiple_levels() {
    // Curried functions with captures at each level
    let result = eval(
        "(begin \
         (define x 100) \
         (lambda (a) \
          (lambda (b) \
          (lambda (c) \
          (+ x a b c)))))",
    );
    assert!(result.is_ok());
    assert!((result.unwrap()).is_closure());
}

#[test]
fn test_closure_creation_preserves_captures() {
    // Test that closure creation properly preserves all captures
    let result = eval(
        "(begin \
         (define a 1) \
         (define b 2) \
         (define c 3) \
         (lambda (x) (+ a b c x)))",
    );
    assert!(result.is_ok());
    assert!((result.unwrap()).is_closure());
}

#[test]
fn test_lambda_in_arithmetic_context() {
    // Lambdas created but not immediately called
    let result = eval(
        "(begin (define multiplier 5) \
         (lambda (x) (* multiplier x)))",
    );
    assert!(result.is_ok());
    assert!((result.unwrap()).is_closure());
}
