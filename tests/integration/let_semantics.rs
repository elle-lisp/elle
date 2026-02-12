// Tests for let and let* binding semantics
//
// These tests define the correct behavior of let and let* forms.

use elle::compiler::converters::value_to_expr;
use elle::reader::OwnedToken;
use elle::{compile, list, register_primitives, Lexer, Reader, SymbolTable, Value, VM};

fn eval(code: &str) -> Result<Value, String> {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    register_primitives(&mut vm, &mut symbols);

    // Tokenize the input
    let mut lexer = Lexer::new(code);
    let mut tokens = Vec::new();
    while let Some(token) = lexer.next_token()? {
        tokens.push(OwnedToken::from(token));
    }

    if tokens.is_empty() {
        return Err("No input".to_string());
    }

    // Read all expressions
    let mut reader = Reader::new(tokens);
    let mut values = Vec::new();
    while let Some(result) = reader.try_read(&mut symbols) {
        values.push(result?);
    }

    // If we have multiple expressions, wrap them in a begin
    let value = if values.len() == 1 {
        values.into_iter().next().unwrap()
    } else if values.is_empty() {
        return Err("No input".to_string());
    } else {
        // Wrap multiple expressions in a begin
        let mut begin_args = vec![Value::Symbol(symbols.intern("begin"))];
        begin_args.extend(values);
        list(begin_args)
    };

    let expr = value_to_expr(&value, &mut symbols)?;
    let bytecode = compile(&expr);
    vm.execute(&bytecode)
}

// ============================================================================
// SECTION: Basic let semantics
// ============================================================================

#[test]
fn test_let_basic() {
    // Basic let binding
    assert_eq!(eval("(let ((x 1)) x)").unwrap(), Value::Int(1));
    assert_eq!(eval("(let ((x 1) (y 2)) (+ x y))").unwrap(), Value::Int(3));
}

#[test]
fn test_let_bindings_dont_see_each_other() {
    // In let, bindings cannot reference each other
    // This should use the OUTER x (if defined), not the let-bound x
    let code = r#"
        (begin
          (define x 100)
          (let ((x 1) (y x))
            y))
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(100));
}

#[test]
fn test_let_parallel_binding() {
    // Classic parallel binding test - swap values
    let code = r#"
        (let ((x 1) (y 2))
          (let ((x y) (y x))
            (list x y)))
    "#;
    let result = eval(code).unwrap();
    // Should be (2 1), not (2 2)
    assert_eq!(result.to_string(), "(2 1)");
}

// ============================================================================
// SECTION: Basic let* semantics
// ============================================================================

#[test]
fn test_let_star_basic() {
    // Basic let* binding
    assert_eq!(eval("(let* ((x 1)) x)").unwrap(), Value::Int(1));
    assert_eq!(eval("(let* ((x 1) (y 2)) (+ x y))").unwrap(), Value::Int(3));
}

#[test]
fn test_let_star_sequential_binding() {
    // In let*, each binding can see previous bindings
    let code = "(let* ((x 1) (y (+ x 1))) y)";
    assert_eq!(eval(code).unwrap(), Value::Int(2));
}

#[test]
fn test_let_star_chain() {
    // Chain of dependencies
    let code = "(let* ((a 1) (b (+ a 1)) (c (+ b 1)) (d (+ c 1))) d)";
    assert_eq!(eval(code).unwrap(), Value::Int(4));
}

#[test]
fn test_let_star_shadows_previous() {
    // Later binding shadows earlier one
    let code = "(let* ((x 1) (x (+ x 1))) x)";
    assert_eq!(eval(code).unwrap(), Value::Int(2));
}

#[test]
fn test_let_star_complex_chain() {
    // More complex chain with multiple references
    let code = r#"
        (let* ((a 1)
               (b (* a 2))
               (c (+ a b))
               (d (* b c)))
          (list a b c d))
    "#;
    let result = eval(code).unwrap();
    // a=1, b=2, c=3, d=6
    assert_eq!(result.to_string(), "(1 2 3 6)");
}

// ============================================================================
// SECTION: Contrasting let vs let*
// ============================================================================

#[test]
fn test_let_vs_let_star_reference() {
    // Same code, different results

    // let* allows y to reference x
    let code_star = "(let* ((x 10) (y x)) y)";
    assert_eq!(eval(code_star).unwrap(), Value::Int(10));

    // let does NOT allow y to reference the let-bound x
    // y should reference outer x if it exists, or error if not
    let code_let = r#"
        (begin
          (define x 999)
          (let ((x 10) (y x)) y))
    "#;
    assert_eq!(eval(code_let).unwrap(), Value::Int(999));
}

#[test]
fn test_let_vs_let_star_computation() {
    // let* can build on previous computations
    let code_star = "(let* ((x 2) (y (* x x)) (z (* y y))) z)";
    assert_eq!(eval(code_star).unwrap(), Value::Int(16)); // 2^4

    // let cannot - each binding only sees outer scope
    // This would need outer definitions to work
}

// ============================================================================
// SECTION: Nested let/let*
// ============================================================================

#[test]
fn test_nested_let_star() {
    let code = r#"
        (let* ((x 1))
          (let* ((y (+ x 1)))
            (let* ((z (+ y 1)))
              z)))
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(3));
}

#[test]
fn test_let_star_with_inner_let() {
    let code = r#"
        (let* ((x 1) (y 2))
          (let ((a (+ x y)) (b (* x y)))
            (+ a b)))
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(5)); // (1+2) + (1*2) = 3 + 2
}

// ============================================================================
// SECTION: let* with closures
// ============================================================================

#[test]
fn test_let_star_closure_captures_sequential() {
    // Closure captures a let*-bound variable that depends on a previous binding
    let code = r#"
        (let* ((x 10)
               (y (+ x 5))
               (f (fn () y)))
          (f))
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(15));
}

#[test]
fn test_let_star_closure_escapes() {
    // Closure escapes the let* scope
    let code = r#"
        (begin
          (define make-adder
            (fn (n)
              (let* ((base n)
                     (doubled (* base 2)))
                (fn (x) (+ x doubled)))))
          (define add-20 (make-adder 10))
          (add-20 5))
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(25)); // 5 + 20
}

// ============================================================================
// SECTION: Edge cases
// ============================================================================

#[test]
fn test_let_star_empty() {
    // Empty bindings
    assert_eq!(eval("(let* () 42)").unwrap(), Value::Int(42));
}

#[test]
fn test_let_star_single_binding() {
    // Single binding (same as let)
    assert_eq!(eval("(let* ((x 1)) x)").unwrap(), Value::Int(1));
}

#[test]
fn test_let_star_with_function_calls() {
    // Binding expressions can be function calls
    let code = r#"
        (let* ((a (+ 1 2))
               (b (* a a))
               (c (- b a)))
          c)
    "#;
    assert_eq!(eval(code).unwrap(), Value::Int(6)); // a=3, b=9, c=6
}
