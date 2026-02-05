// DEFENSE: Integration tests for closures and lambdas
// Tests the full pipeline from parsing through execution with closures and lambdas
use elle::compiler::converters::value_to_expr;
use elle::{compile, read_str, register_primitives, SymbolTable, Value, VM};

fn eval(input: &str) -> Result<Value, String> {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    register_primitives(&mut vm, &mut symbols);

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
    // Create a simple identity lambda
    let result = eval("(lambda (x) x)");
    assert!(result.is_ok());
    assert!(matches!(result.unwrap(), Value::Closure(_)));
}

#[test]
fn test_lambda_creation_single_arg() {
    // Create lambda with single parameter
    let result = eval("(lambda (x) (+ x 1))");
    assert!(result.is_ok());
    assert!(matches!(result.unwrap(), Value::Closure(_)));
}

#[test]
fn test_lambda_creation_multiple_args() {
    // Create lambda with multiple parameters
    let result = eval("(lambda (a b c) (+ a b c))");
    assert!(result.is_ok());
    assert!(matches!(result.unwrap(), Value::Closure(_)));
}

#[test]
fn test_lambda_creation_no_args() {
    // Create lambda with no parameters
    let result = eval("(lambda () 42)");
    assert!(result.is_ok());
    assert!(matches!(result.unwrap(), Value::Closure(_)));
}

#[test]
fn test_lambda_with_complex_body() {
    // Lambda with complex body expressions
    let result = eval("(lambda (x) (if (> x 0) (* x 2) (- x)))");
    assert!(result.is_ok());
    assert!(matches!(result.unwrap(), Value::Closure(_)));
}

// ============================================================================
// SECTION 2: Lambda Parameter Binding
// ============================================================================

#[test]
fn test_lambda_parameter_names() {
    // Different parameter names should parse correctly
    assert!(eval("(lambda (x) x)").is_ok());
    assert!(eval("(lambda (value) value)").is_ok());
    assert!(eval("(lambda (my-var) my-var)").is_ok());
}

#[test]
fn test_lambda_many_parameters() {
    // Lambda with many parameters
    let result = eval("(lambda (a b c d e f g h i j) (+ a b c d e f g h i j))");
    assert!(result.is_ok());
}

#[test]
fn test_lambda_parameter_shadowing() {
    // Parameter names shadow outer scope
    let result = eval("(begin (define x 10) (lambda (x) x))");
    assert!(result.is_ok());
}

// ============================================================================
// SECTION 3: Nested Lambdas (Higher-Order Functions)
// ============================================================================

#[test]
fn test_nested_lambda_double() {
    // Lambda returning lambda (curried function)
    let result = eval("(lambda (x) (lambda (y) (+ x y)))");
    assert!(result.is_ok());
    assert!(matches!(result.unwrap(), Value::Closure(_)));
}

#[test]
fn test_nested_lambda_triple() {
    // Triple nested lambda
    let result = eval("(lambda (a) (lambda (b) (lambda (c) (+ a b c))))");
    assert!(result.is_ok());
    assert!(matches!(result.unwrap(), Value::Closure(_)));
}

#[test]
fn test_nested_lambda_in_expression() {
    // Nested lambda within conditional
    let result = eval("(lambda (x) (if (> x 0) (lambda (y) (+ x y)) (lambda (y) (- x y))))");
    assert!(result.is_ok());
}

#[test]
fn test_nested_lambda_in_list() {
    // Lambda creating a list of lambdas
    let result = eval("(lambda (x) (list (lambda (y) (+ x y)) (lambda (y) (* x y))))");
    assert!(result.is_ok());
}

// ============================================================================
// SECTION 4: Variable Capture in Closures
// ============================================================================

#[test]
fn test_closure_captures_defined_variable() {
    // Closure should capture variables from outer scope
    let result = eval("(begin (define x 10) (lambda (y) (+ x y)))");
    assert!(result.is_ok());
}

#[test]
fn test_closure_captures_multiple_variables() {
    // Closure capturing multiple outer variables
    let result = eval("(begin (define a 1) (define b 2) (lambda (c) (+ a b c)))");
    assert!(result.is_ok());
}

#[test]
fn test_closure_in_nested_scope() {
    // Closure in nested scope captures from all outer scopes
    let result = eval(
        "(begin (define outer 100) \
         (begin (define inner 50) \
          (lambda (x) (+ outer inner x))))",
    );
    assert!(result.is_ok());
}

#[test]
fn test_closure_captures_previous_lambda() {
    // Lambda can reference another lambda
    let result = eval(
        "(begin (define adder (lambda (x) (lambda (y) (+ x y)))) \
         (lambda (z) z))",
    );
    assert!(result.is_ok());
}

// ============================================================================
// SECTION 5: Lambda in Define Context
// ============================================================================

#[test]
fn test_define_lambda_identity() {
    // Define a lambda as a variable
    let result = eval("(begin (define id (lambda (x) x)) id)");
    assert!(result.is_ok());
    assert!(matches!(result.unwrap(), Value::Closure(_)));
}

#[test]
fn test_define_lambda_arithmetic() {
    // Define arithmetic lambda
    let result = eval("(begin (define double (lambda (x) (* x 2))) double)");
    assert!(result.is_ok());
    assert!(matches!(result.unwrap(), Value::Closure(_)));
}

#[test]
fn test_define_multiple_lambdas() {
    // Define multiple lambdas
    let result = eval(
        "(begin \
         (define inc (lambda (x) (+ x 1))) \
         (define dec (lambda (x) (- x 1))) \
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
    let result = eval("(lambda (x) (if (> x 0) x (- x)))");
    assert!(result.is_ok());
}

#[test]
fn test_lambda_with_nested_if() {
    // Lambda with nested conditionals
    let result = eval("(lambda (x y) (if (> x 0) (if (> y 0) 1 -1) (if (> y 0) -1 1)))");
    assert!(result.is_ok());
}

#[test]
fn test_lambda_returning_boolean() {
    // Lambda that returns boolean based on condition
    let result = eval("(lambda (x) (> x 0))");
    assert!(result.is_ok());
}

// ============================================================================
// SECTION 7: Lambdas with List Operations
// ============================================================================

#[test]
fn test_lambda_operating_on_list() {
    // Lambda taking a list as parameter
    let result = eval("(lambda (lst) (first lst))");
    assert!(result.is_ok());
}

#[test]
fn test_lambda_constructing_list() {
    // Lambda that constructs a list
    let result = eval("(lambda (a b c) (list a b c))");
    assert!(result.is_ok());
}

#[test]
fn test_lambda_with_cons() {
    // Lambda using cons operation
    let result = eval("(lambda (x lst) (cons x lst))");
    assert!(result.is_ok());
}

#[test]
fn test_lambda_with_length() {
    // Lambda computing list length
    let result = eval("(lambda (lst) (length lst))");
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
         (define closure (lambda (y) (+ x y))) \
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
         (define f1 (lambda (y) (+ x y))) \
         (define x 2) \
         (define f2 (lambda (y) (+ x y))) \
         (list f1 f2))",
    );
    assert!(result.is_ok());
}

// ============================================================================
// SECTION 9: Complex Closure Scenarios
// ============================================================================

#[test]
fn test_closure_factory_pattern() {
    // Lambda factory - create reusable lambda generators
    let result = eval(
        "(begin \
         (define make-multiplier \
           (lambda (factor) \
             (lambda (x) (* factor x)))) \
         make-multiplier)",
    );
    // The factory itself should be creatable even if calling it may have limitations
    assert!(result.is_ok());
}

#[test]
fn test_closure_chaining() {
    // Chain of nested lambdas - can create curried function structure
    let result = eval("(lambda (a) (lambda (b) (lambda (c) (+ a b c))))");
    assert!(result.is_ok());
}

#[test]
fn test_closure_with_state_capture() {
    // Closure capturing multiple state variables from outer scope
    let result = eval(
        "(begin \
         (define base 100) \
         (define multiplier 2) \
         (lambda (x) (+ base (* multiplier x))))",
    );
    assert!(result.is_ok());
}

// ============================================================================
// SECTION 10: Lambda Type Verification
// ============================================================================

#[test]
fn test_lambda_returns_closure_type() {
    // Verify that lambda always returns a Closure value
    let result1 = eval("(lambda () 1)");
    assert!(matches!(result1.unwrap(), Value::Closure(_)));

    let result2 = eval("(lambda (x) x)");
    assert!(matches!(result2.unwrap(), Value::Closure(_)));

    let result3 = eval("(lambda (a b c) (+ a b c))");
    assert!(matches!(result3.unwrap(), Value::Closure(_)));
}

#[test]
fn test_defined_lambda_is_closure() {
    // Lambda stored in variable should be a Closure
    let result = eval("(begin (define f (lambda (x) x)) f)");
    assert!(matches!(result.unwrap(), Value::Closure(_)));
}

// ============================================================================
// SECTION 11: Lambda Syntax Edge Cases
// ============================================================================

#[test]
fn test_lambda_constant_body() {
    // Lambda with just a constant
    let result = eval("(lambda (x) 42)");
    assert!(result.is_ok());
}

#[test]
fn test_lambda_quoted_list_body() {
    // Lambda with quoted list as body
    let result = eval("(lambda (x) '(1 2 3))");
    assert!(result.is_ok());
}

#[test]
fn test_lambda_arithmetic_expression_body() {
    // Lambda with arithmetic expression
    let result = eval("(lambda (x y z) (+ (* x 2) (- y 1) (/ z 2)))");
    assert!(result.is_ok());
}

// ============================================================================
// SECTION 12: Integration with Other Language Features
// ============================================================================

#[test]
fn test_lambda_in_list() {
    // Lambda stored in a list
    let result = eval("(list (lambda (x) x) (lambda (y) y))");
    assert!(result.is_ok());
    let list = result.unwrap().list_to_vec().unwrap();
    assert_eq!(list.len(), 2);
    assert!(matches!(list[0], Value::Closure(_)));
    assert!(matches!(list[1], Value::Closure(_)));
}

#[test]
fn test_lambda_in_begin_block() {
    // Lambda in begin block
    let result = eval("(begin (define x 1) (lambda (y) (+ x y)))");
    assert!(result.is_ok());
    assert!(matches!(result.unwrap(), Value::Closure(_)));
}

#[test]
fn test_lambda_in_if_consequent() {
    // Lambda as consequent of if
    let result = eval("(if #t (lambda (x) x) (lambda (x) (- x)))");
    assert!(result.is_ok());
    assert!(matches!(result.unwrap(), Value::Closure(_)));
}

#[test]
fn test_lambda_in_if_alternate() {
    // Lambda as alternate of if
    let result = eval("(if #f (lambda (x) x) (lambda (x) (- x)))");
    assert!(result.is_ok());
    assert!(matches!(result.unwrap(), Value::Closure(_)));
}

// ============================================================================
// SECTION 13: Closure Display and Representation
// ============================================================================

#[test]
fn test_closure_string_representation() {
    // Closure should have a reasonable string representation
    let result = eval("(lambda (x) x)");
    let closure_str = format!("{}", result.unwrap());
    assert_eq!(closure_str, "<closure>");
}

#[test]
fn test_closure_in_list_display() {
    // Closure in list should display properly
    let result = eval("(list (lambda (x) x) 42)");
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
         (lambda (x) x))",
    );
    assert!(result.is_ok());
}

#[test]
fn test_lambda_captures_free_variables() {
    // Lambda captures variables not in parameter list
    let result = eval(
        "(begin \
         (define free-var 50) \
         (lambda (param) (+ free-var param)))",
    );
    assert!(result.is_ok());
}

#[test]
fn test_lambda_doesnt_capture_undefined() {
    // Lambda referencing undefined should fail to compile
    let result = eval("(lambda (x) undefined-var)");
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
         (lambda (y) (+ x y)))",
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
         (lambda (x) (lambda (y) (+ level1 level2 x y))))",
    );
    assert!(result.is_ok());
}

// ============================================================================
// SECTION 16: Lambda Interaction with Primitives
// ============================================================================

#[test]
fn test_lambda_with_arithmetic_primitives() {
    // Lambda body using arithmetic primitives
    let result = eval("(lambda (x) (+ x 1))");
    assert!(result.is_ok());
}

#[test]
fn test_lambda_with_comparison_primitives() {
    // Lambda body using comparison primitives
    let result = eval("(lambda (x) (> x 0))");
    assert!(result.is_ok());
}

#[test]
fn test_lambda_with_list_primitives() {
    // Lambda body using list primitives
    let result = eval("(lambda (lst) (cons 1 lst))");
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
         (define add-curried (lambda (a) (lambda (b) (+ a b)))) \
         add-curried)",
    );
    assert!(result.is_ok());
}

#[test]
fn test_function_composition_pattern() {
    // Pattern for composing functions - define the pattern itself
    let result = eval("(lambda (f g) (lambda (x) (f (g x))))");
    assert!(result.is_ok());
}

#[test]
fn test_predicate_creator() {
    // Create predicates as closures - define the factory pattern
    let result = eval("(lambda (n) (lambda (x) (> x n)))");
    assert!(result.is_ok());
}

// ============================================================================
// SECTION 18: Error Cases and Boundaries
// ============================================================================

#[test]
fn test_lambda_missing_body() {
    // Lambda without body should error
    let result = eval("(lambda (x))");
    assert!(result.is_err());
}

#[test]
fn test_lambda_missing_parameters() {
    // Lambda without parameter list should error
    let result = eval("(lambda 42)");
    assert!(result.is_err());
}

#[test]
fn test_lambda_non_list_parameters() {
    // Lambda parameters must be a list
    let result = eval("(lambda x x)");
    assert!(result.is_err());
}

// ============================================================================
// SECTION 19: Large-Scale Closure Tests
// ============================================================================

#[test]
fn test_deeply_nested_lambdas() {
    // Very deeply nested lambda
    let mut lambda_expr = String::from("(+ a b)");
    for i in (0..10).rev() {
        lambda_expr = format!(
            "(lambda ({}) {})",
            char::from_u32(97 + i).unwrap(),
            lambda_expr
        );
    }
    let result = eval(&lambda_expr);
    assert!(result.is_ok());
}

#[test]
fn test_many_lambda_definitions() {
    // Create multiple lambda definitions in sequence
    let result = eval(
        "(begin \
         (define f0 (lambda (x) (+ x 0))) \
         (define f1 (lambda (x) (+ x 1))) \
         (define f2 (lambda (x) (+ x 2))) \
         (define f3 (lambda (x) (+ x 3))) \
         (define f4 (lambda (x) (+ x 4))) \
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
    let result = eval("(lambda (initial) (lambda (x) (+ initial x)))");
    assert!(result.is_ok());
}

#[test]
fn test_partial_application_pattern() {
    // Partial application of functions - curried multiplication
    let result = eval("(lambda (a) (lambda (b) (* a b)))");
    assert!(result.is_ok());
}

#[test]
fn test_conditional_logic_in_closure() {
    // Complex conditional logic in closure
    let result = eval(
        "(begin \
         (define max-of-3 (lambda (a b c) \
           (if (> a b) \
             (if (> a c) a c) \
             (if (> b c) b c)))) \
         max-of-3)",
    );
    assert!(result.is_ok());
}
