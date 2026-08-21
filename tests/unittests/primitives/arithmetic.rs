use super::*;

// Arithmetic tests (+ - * / are now stdlib functions, tested via eval_source)
#[test]
fn test_addition() {
    // No args
    eval_source("(+)", |r| assert_eq!(r.unwrap(), Value::int(0)));

    // Single arg
    eval_source("(+ 5)", |r| assert_eq!(r.unwrap(), Value::int(5)));

    // Multiple args
    eval_source("(+ 1 2 3)", |r| assert_eq!(r.unwrap(), Value::int(6)));

    // Mixed int/float
    eval_source("(+ 1 2.5)", |r| {
        if let Some(f) = r.unwrap().as_float() {
            assert!((f - 3.5).abs() < 1e-10)
        } else {
            panic!("Expected float");
        }
    });
}

#[test]
fn test_subtraction() {
    // Negate
    eval_source("(- 5)", |r| assert_eq!(r.unwrap(), Value::int(-5)));

    // Subtract
    eval_source("(- 10 3)", |r| assert_eq!(r.unwrap(), Value::int(7)));

    // Multiple args
    eval_source("(- 100 25 25)", |r| assert_eq!(r.unwrap(), Value::int(50)));
}

#[test]
fn test_multiplication() {
    // Identity
    eval_source("(*)", |r| assert_eq!(r.unwrap(), Value::int(1)));

    // Multiply
    eval_source("(* 2 3 4)", |r| assert_eq!(r.unwrap(), Value::int(24)));

    // Zero
    eval_source("(* 5 0)", |r| assert_eq!(r.unwrap(), Value::int(0)));
}

#[test]
fn test_division() {
    // Division
    eval_source("(/ 10 2)", |r| assert_eq!(r.unwrap(), Value::int(5)));

    // Integer division
    eval_source("(/ 7 2)", |r| assert_eq!(r.unwrap(), Value::int(3)));

    // Division by zero
    eval_source("(/ 10 0)", |r| assert!(r.is_err()));
}

// Comparison tests
#[test]
fn test_equality() {
    let (_vm, mut symbols, meta) = setup();
    let eq = get_primitive(&meta, &mut symbols, "=");

    assert_eq!(
        call_primitive(&eq, &[Value::int(5), Value::int(5)]).unwrap(),
        Value::bool(true)
    );

    assert_eq!(
        call_primitive(&eq, &[Value::int(5), Value::int(6)]).unwrap(),
        Value::bool(false)
    );

    // Float equality
    assert_eq!(
        call_primitive(
            &eq,
            &[
                Value::float(std::f64::consts::PI),
                Value::float(std::f64::consts::PI)
            ]
        )
        .unwrap(),
        Value::bool(true)
    );
}

#[test]
fn test_less_than() {
    eval_source("(< 3 5)", |r| assert_eq!(r.unwrap(), Value::bool(true)));
    eval_source("(< 5 5)", |r| assert_eq!(r.unwrap(), Value::bool(false)));
    eval_source("(< 7 5)", |r| assert_eq!(r.unwrap(), Value::bool(false)));
}

#[test]
fn test_greater_than() {
    eval_source("(> 7 5)", |r| assert_eq!(r.unwrap(), Value::bool(true)));
    eval_source("(> 5 5)", |r| assert_eq!(r.unwrap(), Value::bool(false)));
}

// List operation tests
#[test]
fn test_cons() {
    eval_source("(pair 1 2)", |r| {
        let result = r.unwrap();
        let cons_cell = result.as_pair().unwrap();
        assert_eq!(cons_cell.first, Value::int(1));
        assert_eq!(cons_cell.rest, Value::int(2));
    });
}

#[test]
fn test_first() {
    let (_vm, mut symbols, meta) = setup();
    let first = get_primitive(&meta, &mut symbols, "first");

    let h = elle::primitives::ctx::TestHeap::new();
    let l = h
        .ctx()
        .list(vec![Value::int(10), Value::int(20), Value::int(30)]);
    let result = call_primitive(&first, &[l]).unwrap();

    assert_eq!(result, Value::int(10));
}

#[test]
fn test_rest() {
    let (_vm, mut symbols, meta) = setup();
    let rest = get_primitive(&meta, &mut symbols, "rest");

    let h = elle::primitives::ctx::TestHeap::new();
    let l = h
        .ctx()
        .list(vec![Value::int(10), Value::int(20), Value::int(30)]);
    let result = call_primitive(&rest, &[l]).unwrap();

    assert!(result.is_list());
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 2);
    assert_eq!(vec[0], Value::int(20));
    assert_eq!(vec[1], Value::int(30));
}

#[test]
fn test_list() {
    let (_vm, mut symbols, meta) = setup();
    let list_fn = get_primitive(&meta, &mut symbols, "list");

    let result = call_primitive(&list_fn, &[Value::int(1), Value::int(2), Value::int(3)]).unwrap();

    assert!(result.is_list());
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 3);
}

// Logic tests (not is now a stdlib function)
#[test]
fn test_not() {
    eval_source("(not false)", |r| assert_eq!(r.unwrap(), Value::bool(true)));
    eval_source("(not true)", |r| assert_eq!(r.unwrap(), Value::bool(false)));
    eval_source("(not nil)", |r| assert_eq!(r.unwrap(), Value::bool(true))); // nil is falsy
                                                                             // Truthy values
    eval_source("(not 0)", |r| assert_eq!(r.unwrap(), Value::bool(false)));
}

// Error handling tests (+ < are now stdlib functions)
#[test]
fn test_arithmetic_type_errors() {
    // Adding non-numbers
    eval_source("(+ nil)", |r| assert!(r.is_err()));
    eval_source("(+ true)", |r| assert!(r.is_err()));
}

#[test]
fn test_comparison_type_errors() {
    // Comparing non-numbers
    eval_source("(< nil 5)", |r| assert!(r.is_err()));
}

#[test]
fn test_list_operation_errors() {
    let (_vm, mut symbols, meta) = setup();
    let first = get_primitive(&meta, &mut symbols, "first");

    // First of non-list
    assert!(call_primitive(&first, &[Value::int(42)]).is_err());
    assert!(call_primitive(&first, &[Value::NIL]).is_err());
}

// Arity checking — verified at the VM dispatch level
#[test]
fn test_arity_errors() {
    // first requires exactly 1 argument
    eval_source("(first)", |r| assert!(r.is_err()));
    eval_source("(first 1 2)", |r| assert!(r.is_err()));

    // = requires exactly 2 arguments
    eval_source("(= 1)", |r| assert!(r.is_err()));
}

#[test]
fn test_disbit_arity_error() {
    eval_source("(disassemble/bytecode)", |r| assert!(r.is_err()));
}

#[test]
fn test_disjit_arity_error() {
    eval_source("(disassemble/jit)", |r| assert!(r.is_err()));
}
