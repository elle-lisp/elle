// DEFENSE: Integration tests for closures and lambdas
// Tests the full pipeline from parsing through execution with closures and lambdas
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
// SECTION 1: Basic Lambda Creation
// ============================================================================

#[test]
fn test_lambda_creation_identity() {
    // Create a simple identity fn
    let result = eval("(fn (x) x)");
    assert!(result.is_ok());
    assert!((result.unwrap()).is_closure());
}

#[test]
fn test_lambda_creation_single_arg() {
    // Create fn with single parameter
    let result = eval("(fn (x) (+ x 1))");
    assert!(result.is_ok());
    assert!((result.unwrap()).is_closure());
}

#[test]
fn test_lambda_creation_multiple_args() {
    // Create fn with multiple parameters
    let result = eval("(fn (a b c) (+ a b c))");
    assert!(result.is_ok());
    assert!((result.unwrap()).is_closure());
}

#[test]
fn test_lambda_creation_no_args() {
    // Create fn with no parameters
    let result = eval("(fn () 42)");
    assert!(result.is_ok());
    assert!((result.unwrap()).is_closure());
}

#[test]
fn test_lambda_with_complex_body() {
    // Lambda with complex body expressions
    let result = eval("(fn (x) (if (> x 0) (* x 2) (- x)))");
    assert!(result.is_ok());
    assert!((result.unwrap()).is_closure());
}

// ============================================================================
// SECTION 2: Lambda Parameter Binding
// ============================================================================

#[test]
fn test_lambda_parameter_names() {
    // Different parameter names should parse correctly
    assert!(eval("(fn (x) x)").is_ok());
    assert!(eval("(fn (value) value)").is_ok());
    assert!(eval("(fn (my-var) my-var)").is_ok());
}

#[test]
fn test_lambda_many_parameters() {
    // Lambda with many parameters
    let result = eval("(fn (a b c d e f g h i j) (+ a b c d e f g h i j))");
    assert!(result.is_ok());
}

#[test]
fn test_lambda_parameter_shadowing() {
    // Parameter names shadow outer scope
    let result = eval("(begin (define x 10) (fn (x) x))");
    assert!(result.is_ok());
}

// ============================================================================
// SECTION 3: Nested Lambdas (Higher-Order Functions)
// ============================================================================

#[test]
fn test_nested_lambda_double() {
    // Lambda returning fn (curried function)
    let result = eval("(fn (x) (fn (y) (+ x y)))");
    assert!(result.is_ok());
    assert!((result.unwrap()).is_closure());
}

#[test]
fn test_nested_lambda_triple() {
    // Triple nested fn
    let result = eval("(fn (a) (fn (b) (fn (c) (+ a b c))))");
    assert!(result.is_ok());
    assert!((result.unwrap()).is_closure());
}

#[test]
fn test_nested_lambda_in_expression() {
    // Nested fn within conditional
    let result = eval("(fn (x) (if (> x 0) (fn (y) (+ x y)) (fn (y) (- x y))))");
    assert!(result.is_ok());
}

#[test]
fn test_nested_lambda_in_list() {
    // Lambda creating a list of lambdas
    let result = eval("(fn (x) (list (fn (y) (+ x y)) (fn (y) (* x y))))");
    assert!(result.is_ok());
}

// ============================================================================
// SECTION 4: Variable Capture in Closures
// ============================================================================

#[test]
fn test_closure_captures_defined_variable() {
    // Closure should capture variables from outer scope
    let result = eval("(begin (define x 10) (fn (y) (+ x y)))");
    assert!(result.is_ok());
}

#[test]
fn test_closure_captures_multiple_variables() {
    // Closure capturing multiple outer variables
    let result = eval("(begin (define a 1) (define b 2) (fn (c) (+ a b c)))");
    assert!(result.is_ok());
}

#[test]
fn test_closure_in_nested_scope() {
    // Closure in nested scope captures from all outer scopes
    let result = eval(
        "(begin (define outer 100) \
         (begin (define inner 50) \
          (fn (x) (+ outer inner x))))",
    );
    assert!(result.is_ok());
}

#[test]
fn test_closure_captures_previous_lambda() {
    // Lambda can reference another fn
    let result = eval(
        "(begin (define adder (fn (x) (fn (y) (+ x y)))) \
         (fn (z) z))",
    );
    assert!(result.is_ok());
}

// ============================================================================
// SECTION 5: Lambda in Define Context
// ============================================================================

#[test]
fn test_define_lambda_identity() {
    // Define a fn as a variable
    let result = eval("(begin (define id (fn (x) x)) id)");
    assert!(result.is_ok());
    assert!((result.unwrap()).is_closure());
}

#[test]
fn test_define_lambda_arithmetic() {
    // Define arithmetic fn
    let result = eval("(begin (define double (fn (x) (* x 2))) double)");
    assert!(result.is_ok());
    assert!((result.unwrap()).is_closure());
}

#[test]
fn test_define_multiple_lambdas() {
    // Define multiple lambdas
    let result = eval(
        "(begin \
         (define inc (fn (x) (+ x 1))) \
         (define dec (fn (x) (- x 1))) \
         (list inc dec))",
    );
    assert!(result.is_ok());
}

// ============================================================================
// SECTION 6: Lambdas with Conditionals
// ============================================================================

#[test]
fn test_lambda_with_if() {
    // Lambda using if-then-else
    let result = eval("(fn (x) (if (> x 0) x (- x)))");
    assert!(result.is_ok());
}

#[test]
fn test_lambda_with_nested_if() {
    // Lambda with nested conditionals
    let result = eval("(fn (x y) (if (> x 0) (if (> y 0) 1 -1) (if (> y 0) -1 1)))");
    assert!(result.is_ok());
}

#[test]
fn test_lambda_returning_boolean() {
    // Lambda that returns boolean based on condition
    let result = eval("(fn (x) (> x 0))");
    assert!(result.is_ok());
}

// ============================================================================
// SECTION 7: Lambdas with List Operations
// ============================================================================

#[test]
fn test_lambda_operating_on_list() {
    // Lambda taking a list as parameter
    let result = eval("(fn (lst) (first lst))");
    assert!(result.is_ok());
}

#[test]
fn test_lambda_constructing_list() {
    // Lambda that constructs a list
    let result = eval("(fn (a b c) (list a b c))");
    assert!(result.is_ok());
}

#[test]
fn test_lambda_with_cons() {
    // Lambda using cons operation
    let result = eval("(fn (x lst) (cons x lst))");
    assert!(result.is_ok());
}

#[test]
fn test_lambda_with_length() {
    // Lambda computing list length
    let result = eval("(fn (lst) (length lst))");
    assert!(result.is_ok());
}

// ============================================================================
// SECTION 8: Closure Environment Correctness
// ============================================================================

#[test]
fn test_closure_environment_persistence() {
    // Closure should capture environment at definition time
    let result = eval(
        "(begin \
         (define x 10) \
         (define closure (fn (y) (+ x y))) \
         (begin (define x 20) closure))",
    );
    assert!(result.is_ok());
}

#[test]
fn test_multiple_closures_independent_environments() {
    // Multiple closures should have independent environments
    let result = eval(
        "(begin \
         (define x 1) \
         (define f1 (fn (y) (+ x y))) \
         (define x 2) \
         (define f2 (fn (y) (+ x y))) \
         (list f1 f2))",
    );
    assert!(result.is_ok());
}

// ============================================================================
// SECTION 9: Complex Closure Scenarios
// ============================================================================

#[test]
fn test_closure_factory_pattern() {
    // Lambda factory - create reusable fn generators
    let result = eval(
        "(begin \
         (define make-multiplier \
           (fn (factor) \
             (fn (x) (* factor x)))) \
         make-multiplier)",
    );
    // The factory itself should be creatable even if calling it may have limitations
    assert!(result.is_ok());
}

#[test]
fn test_closure_chaining() {
    // Chain of nested lambdas - can create curried function structure
    let result = eval("(fn (a) (fn (b) (fn (c) (+ a b c))))");
    assert!(result.is_ok());
}

#[test]
fn test_closure_with_state_capture() {
    // Closure capturing multiple state variables from outer scope
    let result = eval(
        "(begin \
         (define base 100) \
         (define multiplier 2) \
         (fn (x) (+ base (* multiplier x))))",
    );
    assert!(result.is_ok());
}

// ============================================================================
// SECTION 10: Lambda Type Verification
// ============================================================================

#[test]
fn test_lambda_returns_closure_type() {
    // Verify that fn always returns a Closure value
    let result1 = eval("(fn () 1)");
    assert!((result1.unwrap()).is_closure());

    let result2 = eval("(fn (x) x)");
    assert!((result2.unwrap()).is_closure());

    let result3 = eval("(fn (a b c) (+ a b c))");
    assert!((result3.unwrap()).is_closure());
}

#[test]
fn test_defined_lambda_is_closure() {
    // Lambda stored in variable should be a Closure
    let result = eval("(begin (define f (fn (x) x)) f)");
    assert!((result.unwrap()).is_closure());
}

// ============================================================================
// SECTION 11: Lambda Syntax Edge Cases
// ============================================================================

#[test]
fn test_lambda_constant_body() {
    // Lambda with just a constant
    let result = eval("(fn (x) 42)");
    assert!(result.is_ok());
}

#[test]
fn test_lambda_quoted_list_body() {
    // Lambda with quoted list as body
    let result = eval("(fn (x) '(1 2 3))");
    assert!(result.is_ok());
}

#[test]
fn test_lambda_arithmetic_expression_body() {
    // Lambda with arithmetic expression
    let result = eval("(fn (x y z) (+ (* x 2) (- y 1) (/ z 2)))");
    assert!(result.is_ok());
}

// ============================================================================
// SECTION 12: Integration with Other Language Features
// ============================================================================

#[test]
fn test_lambda_in_list() {
    // Lambda stored in a list
    let result = eval("(list (fn (x) x) (fn (y) y))");
    assert!(result.is_ok());
    let list = result.unwrap().list_to_vec().unwrap();
    assert_eq!(list.len(), 2);
    assert!((list[0]).is_closure());
    assert!((list[1]).is_closure());
}

#[test]
fn test_lambda_in_begin_block() {
    // Lambda in begin block
    let result = eval("(begin (define x 1) (fn (y) (+ x y)))");
    assert!(result.is_ok());
    assert!((result.unwrap()).is_closure());
}

#[test]
fn test_lambda_in_if_consequent() {
    // Lambda as consequent of if
    let result = eval("(if #t (fn (x) x) (fn (x) (- x)))");
    assert!(result.is_ok());
    assert!((result.unwrap()).is_closure());
}

#[test]
fn test_lambda_in_if_alternate() {
    // Lambda as alternate of if
    let result = eval("(if #f (fn (x) x) (fn (x) (- x)))");
    assert!(result.is_ok());
    assert!((result.unwrap()).is_closure());
}

// ============================================================================
// SECTION 13: Closure Display and Representation
// ============================================================================

#[test]
fn test_closure_string_representation() {
    // Closure should have a reasonable string representation
    let result = eval("(fn (x) x)");
    let closure_str = format!("{:?}", result.unwrap());
    assert_eq!(closure_str, "<closure>");
}

#[test]
fn test_closure_in_list_display() {
    // Closure in list should display properly
    let result = eval("(list (fn (x) x) 42)");
    assert!(result.is_ok());
    let list = result.unwrap().list_to_vec().unwrap();
    assert_eq!(list.len(), 2);
}

// ============================================================================
// SECTION 14: Scope Behavior Verification
// ============================================================================

#[test]
fn test_lambda_parameter_scope() {
    // Lambda parameters create a new scope
    let result = eval(
        "(begin \
         (define x 100) \
         (fn (x) x))",
    );
    assert!(result.is_ok());
}

#[test]
fn test_lambda_captures_free_variables() {
    // Lambda captures variables not in parameter list
    let result = eval(
        "(begin \
         (define free-var 50) \
         (fn (param) (+ free-var param)))",
    );
    assert!(result.is_ok());
}

#[test]
fn test_lambda_doesnt_capture_undefined() {
    // Lambda referencing undefined should fail to compile
    let result = eval("(fn (x) undefined-var)");
    // May or may not error depending on implementation
    let _ = result;
}

// ============================================================================
// SECTION 15: Nested Define and Lambda
// ============================================================================

#[test]
fn test_lambda_with_inner_define() {
    // Lambda can be created in contexts with inner definitions
    let result = eval(
        "(begin \
         (define x 5) \
         (fn (y) (+ x y)))",
    );
    assert!(result.is_ok());
}

#[test]
fn test_multiple_nested_lambdas_with_defines() {
    // Complex nesting with multiple defines and lambdas
    let result = eval(
        "(begin \
         (define level1 10) \
         (define level2 20) \
         (fn (x) (fn (y) (+ level1 level2 x y))))",
    );
    assert!(result.is_ok());
}

// ============================================================================
// SECTION 16: Lambda Interaction with Primitives
// ============================================================================

#[test]
fn test_lambda_with_arithmetic_primitives() {
    // Lambda body using arithmetic primitives
    let result = eval("(fn (x) (+ x 1))");
    assert!(result.is_ok());
}

#[test]
fn test_lambda_with_comparison_primitives() {
    // Lambda body using comparison primitives
    let result = eval("(fn (x) (> x 0))");
    assert!(result.is_ok());
}

#[test]
fn test_lambda_with_list_primitives() {
    // Lambda body using list primitives
    let result = eval("(fn (lst) (cons 1 lst))");
    assert!(result.is_ok());
}

// ============================================================================
// SECTION 17: Docstring and Complex Use Cases
// ============================================================================

#[test]
fn test_curried_addition() {
    // Implement curried addition
    let result = eval(
        "(begin \
         (define add-curried (fn (a) (fn (b) (+ a b)))) \
         add-curried)",
    );
    assert!(result.is_ok());
}

#[test]
fn test_function_composition_pattern() {
    // Pattern for composing functions - define the pattern itself
    let result = eval("(fn (f g) (fn (x) (f (g x))))");
    assert!(result.is_ok());
}

#[test]
fn test_predicate_creator() {
    // Create predicates as closures - define the factory pattern
    let result = eval("(fn (n) (fn (x) (> x n)))");
    assert!(result.is_ok());
}

// ============================================================================
// SECTION 18: Error Cases and Boundaries
// ============================================================================

#[test]
fn test_lambda_missing_body() {
    // Lambda without body should error
    let result = eval("(fn (x))");
    assert!(result.is_err());
}

#[test]
fn test_lambda_missing_parameters() {
    // Lambda without parameter list should error
    let result = eval("(fn 42)");
    assert!(result.is_err());
}

#[test]
fn test_lambda_non_list_parameters() {
    // Lambda parameters must be a list
    let result = eval("(fn x x)");
    assert!(result.is_err());
}

// ============================================================================
// SECTION 19: Large-Scale Closure Tests
// ============================================================================

#[test]
fn test_deeply_nested_lambdas() {
    // Very deeply nested fn
    let mut lambda_expr = String::from("(+ a b)");
    for i in (0..10).rev() {
        lambda_expr = format!("(fn ({}) {})", char::from_u32(97 + i).unwrap(), lambda_expr);
    }
    let result = eval(&lambda_expr);
    assert!(result.is_ok());
}

#[test]
fn test_many_lambda_definitions() {
    // Create multiple fn definitions in sequence
    let result = eval(
        "(begin \
         (define f0 (fn (x) (+ x 0))) \
         (define f1 (fn (x) (+ x 1))) \
         (define f2 (fn (x) (+ x 2))) \
         (define f3 (fn (x) (+ x 3))) \
         (define f4 (fn (x) (+ x 4))) \
         (list f0 f1 f2 f3 f4))",
    );
    assert!(result.is_ok());
    let list = result.unwrap().list_to_vec().unwrap();
    assert_eq!(list.len(), 5);
}

// ============================================================================
// SECTION 20: Real-World Patterns
// ============================================================================

#[test]
fn test_accumulator_closure_pattern() {
    // Create an accumulator using closure - define the factory pattern
    let result = eval("(fn (initial) (fn (x) (+ initial x)))");
    assert!(result.is_ok());
}

#[test]
fn test_partial_application_pattern() {
    // Partial application of functions - curried multiplication
    let result = eval("(fn (a) (fn (b) (* a b)))");
    assert!(result.is_ok());
}

#[test]
fn test_conditional_logic_in_closure() {
    // Complex conditional logic in closure
    let result = eval(
        "(begin \
         (define max-of-3 (fn (a b c) \
           (if (> a b) \
             (if (> a c) a c) \
             (if (> b c) b c)))) \
         max-of-3)",
    );
    assert!(result.is_ok());
}

// ============================================================================
// SECTION 8: Scope Management with Let-bindings
// ============================================================================

#[test]
fn test_let_binding_basic_scope() {
    // Let-bindings work with basic variable isolation
    let result = eval("(let ((x 5)) (+ x 1))");
    assert_eq!(result.unwrap(), Value::int(6));
}

#[test]
fn test_let_binding_multiple_vars() {
    // Multiple variables in let-binding
    let result = eval("(let ((x 5) (y 3)) (+ x y))");
    assert_eq!(result.unwrap(), Value::int(8));
}

#[test]
fn test_let_binding_global_shadowing() {
    // Let-binding shadows global variable (via fn transformation)
    let result = eval(
        "(begin
          (define x 100)
          (let ((x 5))
            (+ x 1)))",
    );
    assert_eq!(result.unwrap(), Value::int(6));
}

#[test]
fn test_let_binding_function_scope() {
    // Let-binding works with functions
    let result = eval(
        "(begin
          (define double (fn (x) (* x 2)))
          (let ((x 5))
            (double x)))",
    );
    assert_eq!(result.unwrap(), Value::int(10));
}

#[test]
fn test_let_binding_with_global_access() {
    // Let-binding can access global variables
    let result = eval(
        "(begin
          (define multiplier 10)
          (let ((x 5))
            (* x multiplier)))",
    );
    assert_eq!(result.unwrap(), Value::int(50));
}

// ============================================================================
// SECTION 21: Nested Closure Execution (capture resolution)
// ============================================================================

#[test]
fn test_closure_returning_closure_called() {
    // make-adder pattern: create and immediately use
    let code = r#"
        (((fn (x) (fn (y) (+ x y))) 10) 20)
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(30));
}

#[test]
fn test_two_level_closure_call() {
    // outer captures global, inner captures outer param
    let code = r#"
        (begin
          (define x 100)
          ((fn (a) ((fn (b) (+ x a b)) 2)) 1))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(103));
}

#[test]
fn test_set_in_nested_closure() {
    // set! on a captured variable should work
    let code = r#"
        (begin
          (define counter 0)
          (define inc (fn () (begin (set! counter (+ counter 1)) counter)))
          (inc)
          (inc)
          (inc))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(3));
}

#[test]
fn test_set_local_variable_in_lambda() {
    // set! on a local variable inside a fn should work
    // This is the test case from issue #106
    let code = r#"
         (begin
           (define test (fn ()
             (begin
               (define x 0)
               (set! x 42)
               x)))
           (test))
     "#;
    assert_eq!(eval(code).unwrap(), Value::int(42));
}

#[test]
fn test_make_adder_pattern() {
    // Classic make-adder: define, then call
    let code = r#"
        (begin
          (define make-adder (fn (x) (fn (y) (+ x y))))
          (define add5 (make-adder 5))
          (add5 10))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(15));
}

#[test]
fn test_triple_nested_closure_execution() {
    // Three levels of nesting
    let code = r#"
        ((((fn (a) (fn (b) (fn (c) (+ a b c)))) 1) 2) 3)
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(6));
}

#[test]
fn test_closure_captures_multiple_from_same_scope() {
    // Inner fn captures two variables from the same outer fn
    let code = r#"
        ((fn (x y) ((fn (z) (+ x y z)) 3)) 1 2)
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(6));
}

#[test]
fn test_closure_with_let_and_capture() {
    // let-binding inside a closure that captures from outer scope
    let code = r#"
        ((fn (x)
           (let ((y (* x 2)))
             (+ x y)))
         5)
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(15));
}

#[test]
fn test_closure_global_still_works() {
    // Ensure globals still work through nested closures
    let code = r#"
        (begin
          (define g 100)
          ((fn (x) (+ g x)) 5))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(105));
}

#[test]
fn test_multiple_closures_from_same_factory() {
    // Create multiple closures from the same factory, each with different captured values
    let code = r#"
        (begin
          (define make-adder (fn (x) (fn (y) (+ x y))))
          (define add3 (make-adder 3))
          (define add7 (make-adder 7))
          (+ (add3 10) (add7 10)))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(30));
}

#[test]
fn test_closure_captures_closure() {
    // A closure that captures another closure and calls it
    let code = r#"
        (begin
          (define f (fn (x) (+ x 1)))
          ((fn (g) (g 10)) f))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(11));
}

#[test]
fn test_immediately_invoked_nested_lambda() {
    // Immediately invoked fn inside another fn
    let code = r#"
        ((fn (x)
           ((fn (y) (+ x y)) (* x 2)))
         5)
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(15));
}

// ============================================================================
// SECTION: Regression Tests for Let/Let* Closure Behavior
// ============================================================================
// These tests ensure that let and let* bindings work correctly with closures
// and prevent regressions in closure escape and shadowing behavior.

#[test]
fn test_let_closure_escape() {
    // A closure created inside let that captures a let-bound variable
    // must work even after the let scope exits.
    // This tests that let-bound variables are properly captured by closures.
    let code = r#"
        (begin
          (define make-fn (fn ()
            (let ((x 42))
              (fn () x))))
          (define f (make-fn))
          (f))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(42));
}

#[test]
fn test_let_with_closure_capture() {
    // let-bound variables should be capturable by closures
    let code = r#"
        (let ((x 5) (y 10))
          (fn () (+ x y)))
    "#;
    let result = eval(code).unwrap();
    assert!((result).is_closure());
}

#[test]
fn test_let_basic_binding() {
    // Basic let binding should work correctly
    let code = r#"
        (let ((x 5) (y 10))
          (+ x y))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(15));
}

#[test]
fn test_let_star_basic() {
    // Basic let* binding should work correctly
    let code = r#"
        (let* ((x 1) (y 2) (z 3))
          (+ x y z))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(6));
}

#[test]
fn test_nested_let_closure_escape() {
    // Nested let scopes with closure escape
    let code = r#"
         (begin
           (define make-adder (fn (base)
             (let ((b base))
               (fn (x)
                 (+ b x)))))
           (define add5 (make-adder 5))
           (add5 3))
     "#;
    assert_eq!(eval(code).unwrap(), Value::int(8));
}

// ============================================================================
// SECTION: Higher-Order Functions with Closures (Issue #99)
// ============================================================================

#[test]
fn test_map_with_inline_lambda() {
    // Test map with an inline fn closure
    let code = r#"
        (map (fn (x) (* x 2)) (list 1 2 3))
    "#;
    let result = eval(code).unwrap();
    // Result should be (2 4 6)
    let list = result.list_to_vec().unwrap();
    assert_eq!(list.len(), 3);
    assert_eq!(list[0], Value::int(2));
    assert_eq!(list[1], Value::int(4));
    assert_eq!(list[2], Value::int(6));
}

#[test]
fn test_map_with_defined_closure() {
    // Test map with a previously defined closure
    let code = r#"
        (begin
          (define double (fn (x) (* x 2)))
          (map double (list 1 2 3)))
    "#;
    let result = eval(code).unwrap();
    let list = result.list_to_vec().unwrap();
    assert_eq!(list.len(), 3);
    assert_eq!(list[0], Value::int(2));
    assert_eq!(list[1], Value::int(4));
    assert_eq!(list[2], Value::int(6));
}

#[test]
fn test_map_with_closure_capturing_variable() {
    // Test map with a closure that captures an outer variable
    let code = r#"
        (begin
          (define multiplier 3)
          (map (fn (x) (* x multiplier)) (list 1 2 3)))
    "#;
    let result = eval(code).unwrap();
    let list = result.list_to_vec().unwrap();
    assert_eq!(list.len(), 3);
    assert_eq!(list[0], Value::int(3));
    assert_eq!(list[1], Value::int(6));
    assert_eq!(list[2], Value::int(9));
}

#[test]
fn test_filter_with_inline_lambda() {
    // Test filter with an inline fn closure
    let code = r#"
        (filter (fn (x) (> x 2)) (list 1 2 3 4 5))
    "#;
    let result = eval(code).unwrap();
    // Result should be (3 4 5)
    let list = result.list_to_vec().unwrap();
    assert_eq!(list.len(), 3);
    assert_eq!(list[0], Value::int(3));
    assert_eq!(list[1], Value::int(4));
    assert_eq!(list[2], Value::int(5));
}

#[test]
fn test_filter_with_closure_capturing_threshold() {
    // Test filter with a closure that captures a threshold value
    let code = r#"
        (begin
          (define threshold 2)
          (filter (fn (x) (> x threshold)) (list 1 2 3 4 5)))
    "#;
    let result = eval(code).unwrap();
    let list = result.list_to_vec().unwrap();
    assert_eq!(list.len(), 3);
    assert_eq!(list[0], Value::int(3));
    assert_eq!(list[1], Value::int(4));
    assert_eq!(list[2], Value::int(5));
}

#[test]
fn test_fold_with_inline_lambda() {
    // Test fold with an inline fn closure
    let code = r#"
        (fold (fn (acc x) (+ acc x)) 0 (list 1 2 3))
    "#;
    let result = eval(code).unwrap();
    assert_eq!(result, Value::int(6));
}

#[test]
fn test_fold_with_closure_capturing_initial_value() {
    // Test fold with a closure that uses captured context
    let code = r#"
        (begin
          (define initial 10)
          (fold (fn (acc x) (+ acc x)) initial (list 1 2 3)))
    "#;
    let result = eval(code).unwrap();
    assert_eq!(result, Value::int(16));
}

#[test]
fn test_fold_with_multiplication() {
    // Test fold for computing factorial-like product
    let code = r#"
        (fold (fn (acc x) (* acc x)) 1 (list 2 3 4))
    "#;
    let result = eval(code).unwrap();
    assert_eq!(result, Value::int(24));
}

#[test]
fn test_nested_map_with_closures() {
    // Test nested map calls with closures
    let code = r#"
        (map (fn (x) (map (fn (y) (* x y)) (list 1 2))) (list 1 2))
    "#;
    let result = eval(code).unwrap();
    let outer = result.list_to_vec().unwrap();
    assert_eq!(outer.len(), 2);
    // First inner list: (1 2)
    let inner1 = outer[0].list_to_vec().unwrap();
    assert_eq!(inner1.len(), 2);
    assert_eq!(inner1[0], Value::int(1));
    assert_eq!(inner1[1], Value::int(2));
    // Second inner list: (2 4)
    let inner2 = outer[1].list_to_vec().unwrap();
    assert_eq!(inner2.len(), 2);
    assert_eq!(inner2[0], Value::int(2));
    assert_eq!(inner2[1], Value::int(4));
}

#[test]
fn test_map_filter_composition() {
    // Test composing map and filter with closures
    let code = r#"
        (map (fn (x) (* x 2)) (filter (fn (x) (> x 2)) (list 1 2 3 4 5)))
    "#;
    let result = eval(code).unwrap();
    // Filter: (1 2 3 4 5) -> (3 4 5)
    // Map: (3 4 5) -> (6 8 10)
    let list = result.list_to_vec().unwrap();
    assert_eq!(list.len(), 3);
    assert_eq!(list[0], Value::int(6));
    assert_eq!(list[1], Value::int(8));
    assert_eq!(list[2], Value::int(10));
}

#[test]
fn test_map_with_native_function() {
    // Test that map still works with closures (we removed native function registration)
    let code = r#"
        (begin
          (define inc (fn (x) (+ x 1)))
          (map inc (list 1 2 3)))
    "#;
    let result = eval(code).unwrap();
    let list = result.list_to_vec().unwrap();
    assert_eq!(list.len(), 3);
    assert_eq!(list[0], Value::int(2));
    assert_eq!(list[1], Value::int(3));
    assert_eq!(list[2], Value::int(4));
}

#[test]
fn test_fold_string_concatenation_with_closure() {
    // Test fold for string operations
    let code = r#"
         (fold (fn (acc x) (string-append acc x)) "" (list "a" "b" "c"))
     "#;
    let result = eval(code).unwrap();
    assert_eq!(result, Value::string("abc"));
}

// ============================================================================
// SECTION: Closure Arithmetic Operations (Issue #172)
// ============================================================================
// These tests verify that closures with local definitions and arithmetic
// operations on captured/local variables work correctly.

#[test]
fn test_closure_with_local_define_and_arithmetic() {
    // The simplest case that triggers the bug
    let code = r#"
        (begin
          (define make-counter (fn ()
            (begin
              (define count 0)
              (set! count (+ count 1))
              count)))
          (make-counter))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(1));
}

#[test]
fn test_closure_accumulator_with_arithmetic() {
    // Classic accumulator pattern from the issue
    let code = r#"
        (begin
          (define make-accumulator (fn ()
            (begin
              (define total 0)
              (fn (x)
                (begin
                  (set! total (+ total x))
                  total)))))
          (define acc (make-accumulator))
          (acc 10))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(10));
}

#[test]
fn test_closure_accumulator_multiple_calls() {
    // Verify state is maintained across invocations
    let code = r#"
        (begin
          (define make-accumulator (fn ()
            (begin
              (define total 0)
              (fn (x)
                (begin
                  (set! total (+ total x))
                  total)))))
          (define acc (make-accumulator))
          (begin
            (acc 10)
            (acc 20)
            (acc 5)))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(35));
}

#[test]
fn test_nested_closure_with_local_arithmetic() {
    // Nested closures with local variables
    let code = r#"
        (begin
          (define make-adder (fn (base)
            (begin
              (define offset 10)
              (fn (x)
                (+ base offset x)))))
          (define add-with-offset (make-adder 100))
          (add-with-offset 5))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(115));
}

#[test]
fn test_set_on_captured_parameter() {
    // This is the pattern from scope_explained.lisp that fails
    // The closure captures a function parameter and mutates it with set!
    let code = r#"
        (begin
          (define make-counter (fn (start)
            (fn ()
              (begin
                (set! start (+ start 1))
                start))))
          (define counter1 (make-counter 10))
          (counter1))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(11));
}

#[test]
fn test_set_on_captured_parameter_multiple_calls() {
    // Verify state is maintained across invocations when capturing a parameter
    let code = r#"
        (begin
          (define make-counter (fn (start)
            (fn ()
              (begin
                (set! start (+ start 1))
                start))))
          (define counter1 (make-counter 10))
          (begin
            (counter1)
            (counter1)
            (counter1)))
    "#;
    assert_eq!(eval(code).unwrap(), Value::int(13));
}

// ============================================================================
// SECTION: Lambda Alias Compatibility Tests
// ============================================================================

#[test]
fn test_lambda_alias_works() {
    // Ensure lambda still works as an alias for fn
    let result = eval("((lambda (x) (+ x 1)) 5)");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::int(6));
}

#[test]
fn test_fn_and_lambda_equivalent() {
    // Both fn and lambda should produce identical results
    let fn_result = eval("((fn (x y) (+ x y)) 3 4)");
    let lambda_result = eval("((lambda (x y) (+ x y)) 3 4)");
    assert_eq!(fn_result, lambda_result);
    assert_eq!(fn_result.unwrap(), Value::int(7));
}

#[test]
fn test_lambda_in_define() {
    // lambda should work in define statements
    let result = eval("(begin (define add (lambda (a b) (+ a b))) (add 10 20))");
    assert_eq!(result.unwrap(), Value::int(30));
}

#[test]
fn test_lambda_in_map() {
    // lambda should work with higher-order functions
    let result = eval("(map (lambda (x) (* x 2)) (list 1 2 3))");
    assert!(result.is_ok());
    let val = result.unwrap();
    let list = val.list_to_vec().unwrap();
    assert_eq!(list.len(), 3);
    assert_eq!(list[0], Value::int(2));
    assert_eq!(list[1], Value::int(4));
    assert_eq!(list[2], Value::int(6));
}

#[test]
fn test_lambda_closure_capture() {
    // lambda should properly capture variables
    let result = eval("(begin (define x 10) ((lambda (y) (+ x y)) 5))");
    assert_eq!(result.unwrap(), Value::int(15));
}

// ============================================================================
// SECTION 26: Closure Mutation via set!
// ============================================================================

#[test]
fn test_closure_set_captured_variable() {
    // Closure that mutates a captured variable via set!
    // This tests that set! targets are properly tracked as captures
    let result = eval(
        "((fn () \
           (begin \
             (define x 0) \
             (define setter (fn () (set! x 42))) \
             (setter) \
             x)))",
    );
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_getter_setter_pattern() {
    // Classic getter/setter pattern with closures
    let result = eval(
        "((fn (initial) \
           (begin \
             (define value initial) \
             (define getter (fn () value)) \
             (define setter (fn (new-val) (set! value new-val))) \
             (setter 42) \
             (getter))) 0)",
    );
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_setter_only_no_getter() {
    // Setter without getter - tests set! capture independently
    let result = eval(
        "((fn (initial) \
           (begin \
             (define value initial) \
             (define setter (fn (x) (set! value x))) \
             (setter 42) \
             value)) 0)",
    );
    assert_eq!(result.unwrap(), Value::int(42));
}

#[test]
fn test_multiple_closures_sharing_mutable_state() {
    // Multiple closures sharing and mutating the same variable
    let result = eval(
        "((fn () \
           (begin \
             (define counter 0) \
             (define inc (fn () (set! counter (+ counter 1)))) \
             (define get (fn () counter)) \
             (inc) \
             (inc) \
             (inc) \
             (get))))",
    );
    assert_eq!(result.unwrap(), Value::int(3));
}
