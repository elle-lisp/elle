// Deep tail recursion tests for issue #168
// These tests verify that tail call optimization can handle deep recursion
// without stack overflow or excessive memory usage
use elle::compiler::value_to_expr;
use elle::ffi_primitives;
use elle::{compile, read_str, register_primitives, SymbolTable, Value, VM};

fn eval(input: &str) -> Result<Value, String> {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    register_primitives(&mut vm, &mut symbols);
    ffi_primitives::set_vm_context(&mut vm as *mut VM);

    let value = read_str(input, &mut symbols)?;
    let expr = value_to_expr(&value, &mut symbols)?;
    let bytecode = compile(&expr);
    let result = vm.execute(&bytecode);

    ffi_primitives::clear_vm_context();
    result
}

/// Test basic tail recursion with moderate depth (1000)
#[test]
fn test_tail_recursion_moderate_depth() {
    let code = r#"(begin
      (define count-down
        (fn (n)
          (if (<= n 0)
            0
            (count-down (- n 1)))))
      (count-down 1000))"#;

    let result = eval(code);
    assert!(
        result.is_ok(),
        "Moderate depth recursion should succeed: {:?}",
        result
    );
    assert_eq!(result.unwrap(), Value::Int(0));
}

/// Test tail recursion with deeper depth (10000)
#[test]
fn test_tail_recursion_deep() {
    let code = r#"(begin
      (define count-down
        (fn (n)
          (if (<= n 0)
            0
            (count-down (- n 1)))))
      (count-down 10000))"#;

    let result = eval(code);
    assert!(
        result.is_ok(),
        "Deep recursion should succeed: {:?}",
        result
    );
    assert_eq!(result.unwrap(), Value::Int(0));
}

/// Test tail recursion with very deep depth (50000) - the original issue
#[test]
fn test_tail_recursion_very_deep() {
    let code = r#"(begin
      (define count-down
        (fn (n)
          (if (<= n 0)
            0
            (count-down (- n 1)))))
      (count-down 50000))"#;

    let result = eval(code);
    assert!(
        result.is_ok(),
        "Very deep recursion (50k) should succeed: {:?}",
        result
    );
    assert_eq!(result.unwrap(), Value::Int(0));
}

/// Test tail recursion with accumulator pattern
#[test]
fn test_tail_recursion_with_accumulator() {
    let code = r#"(begin
      (define sum-down
        (fn (n acc)
          (if (<= n 0)
            acc
            (sum-down (- n 1) (+ acc n)))))
      (sum-down 1000 0))"#;

    let result = eval(code);
    assert!(
        result.is_ok(),
        "Tail recursion with accumulator should succeed: {:?}",
        result
    );
    // Sum of 1 to 1000 = 1000 * 1001 / 2 = 500500
    assert_eq!(result.unwrap(), Value::Int(500500));
}

/// Test tail recursion with accumulator at deep depth
#[test]
fn test_tail_recursion_accumulator_deep() {
    let code = r#"(begin
      (define sum-down
        (fn (n acc)
          (if (<= n 0)
            acc
            (sum-down (- n 1) (+ acc n)))))
      (sum-down 10000 0))"#;

    let result = eval(code);
    assert!(
        result.is_ok(),
        "Deep tail recursion with accumulator should succeed: {:?}",
        result
    );
    // Sum of 1 to 10000 = 10000 * 10001 / 2 = 50005000
    assert_eq!(result.unwrap(), Value::Int(50005000));
}

/// Test tail recursion with captured variables
#[test]
fn test_tail_recursion_with_captures() {
    let code = r#"(begin
      (define make-countdown
        (fn (limit)
          (fn (n)
            (if (<= n 0)
              limit
              ((make-countdown limit) (- n 1))))))
      (define countdown (make-countdown 42))
      (countdown 100))"#;

    let result = eval(code);
    // This is a complex nested case, just verify it doesn't crash
    assert!(
        result.is_ok(),
        "Tail recursion with captures should succeed: {:?}",
        result
    );
    assert_eq!(result.unwrap(), Value::Int(42));
}

/// Test tail recursion with local variable definitions
#[test]
fn test_tail_recursion_with_locals() {
    let code = r#"(begin
      (define process
        (fn (n)
          (if (<= n 0)
            0
            (let ((x (+ n 1)))
              (process (- n 1))))))
      (process 1000))"#;

    let result = eval(code);
    assert!(
        result.is_ok(),
        "Tail recursion with local variables should succeed: {:?}",
        result
    );
    assert_eq!(result.unwrap(), Value::Int(0));
}

/// Test tail recursion with local variables at moderate depth
#[test]
fn test_tail_recursion_locals_deep() {
    let code = r#"(begin
       (define process
         (fn (n)
           (if (<= n 0)
             0
             (let ((x (+ n 1)))
               (process (- n 1))))))
       (process 1000))"#;

    let result = eval(code);
    assert!(
        result.is_ok(),
        "Deep tail recursion with local variables should succeed: {:?}",
        result
    );
    assert_eq!(result.unwrap(), Value::Int(0));
}

/// Test tail recursion with multiple parameters
#[test]
fn test_tail_recursion_multiple_params() {
    let code = r#"(begin
      (define countdown-pair
        (fn (a b)
          (if (and (<= a 0) (<= b 0))
            (list a b)
            (if (<= a 0)
              (countdown-pair a (- b 1))
              (countdown-pair (- a 1) b)))))
      (countdown-pair 100 100))"#;

    let result = eval(code);
    assert!(
        result.is_ok(),
        "Tail recursion with multiple params should succeed: {:?}",
        result
    );
    let vec = result.unwrap().list_to_vec().unwrap();
    assert_eq!(vec[0], Value::Int(0));
    assert_eq!(vec[1], Value::Int(0));
}

/// Test tail recursion with multiple parameters at deep depth
#[test]
fn test_tail_recursion_multiple_params_deep() {
    let code = r#"(begin
      (define countdown-pair
        (fn (a b)
          (if (and (<= a 0) (<= b 0))
            (list a b)
            (if (<= a 0)
              (countdown-pair a (- b 1))
              (countdown-pair (- a 1) b)))))
      (countdown-pair 5000 5000))"#;

    let result = eval(code);
    assert!(
        result.is_ok(),
        "Deep tail recursion with multiple params should succeed: {:?}",
        result
    );
    let vec = result.unwrap().list_to_vec().unwrap();
    assert_eq!(vec[0], Value::Int(0));
    assert_eq!(vec[1], Value::Int(0));
}

/// Test tail recursion with conditional branching
#[test]
fn test_tail_recursion_conditional() {
    let code = r#"(begin
      (define process-conditional
        (fn (n)
          (if (<= n 0)
            "done"
            (if (= (mod n 2) 0)
              (process-conditional (- n 1))
              (process-conditional (- n 1))))))
      (process-conditional 1000))"#;

    let result = eval(code);
    assert!(
        result.is_ok(),
        "Tail recursion with conditionals should succeed: {:?}",
        result
    );
    assert_eq!(result.unwrap(), Value::String("done".into()));
}

/// Test tail recursion with conditional branching at deep depth
#[test]
fn test_tail_recursion_conditional_deep() {
    let code = r#"(begin
      (define process-conditional
        (fn (n)
          (if (<= n 0)
            "done"
            (if (= (mod n 2) 0)
              (process-conditional (- n 1))
              (process-conditional (- n 1))))))
      (process-conditional 10000))"#;

    let result = eval(code);
    assert!(
        result.is_ok(),
        "Deep tail recursion with conditionals should succeed: {:?}",
        result
    );
    assert_eq!(result.unwrap(), Value::String("done".into()));
}

/// Test tail recursion returning accumulated list
#[test]
fn test_tail_recursion_list_accumulation() {
    let code = r#"(begin
      (define build-list
        (fn (n acc)
          (if (<= n 0)
            acc
            (build-list (- n 1) (cons n acc)))))
      (length (build-list 100 (list))))"#;

    let result = eval(code);
    assert!(
        result.is_ok(),
        "Tail recursion with list accumulation should succeed: {:?}",
        result
    );
    assert_eq!(result.unwrap(), Value::Int(100));
}

/// Test tail recursion with very deep list accumulation
#[test]
fn test_tail_recursion_list_accumulation_deep() {
    let code = r#"(begin
      (define build-list
        (fn (n acc)
          (if (<= n 0)
            acc
            (build-list (- n 1) (cons n acc)))))
      (length (build-list 5000 (list))))"#;

    let result = eval(code);
    assert!(
        result.is_ok(),
        "Deep tail recursion with list accumulation should succeed: {:?}",
        result
    );
    assert_eq!(result.unwrap(), Value::Int(5000));
}

/// Test mutual tail recursion at moderate depth
#[test]
fn test_mutual_tail_recursion_moderate() {
    let code = r#"(begin
      (define is-even (fn (n) (if (= n 0) #t (is-odd (- n 1)))))
      (define is-odd (fn (n) (if (= n 0) #f (is-even (- n 1)))))
      (list (is-even 1000) (is-odd 999)))"#;

    let result = eval(code);
    assert!(
        result.is_ok(),
        "Mutual tail recursion should succeed: {:?}",
        result
    );
    let vec = result.unwrap().list_to_vec().unwrap();
    assert_eq!(vec[0], Value::Bool(true));
    assert_eq!(vec[1], Value::Bool(true));
}

/// Test mutual tail recursion at deep depth
#[test]
fn test_mutual_tail_recursion_deep() {
    let code = r#"(begin
      (define is-even (fn (n) (if (= n 0) #t (is-odd (- n 1)))))
      (define is-odd (fn (n) (if (= n 0) #f (is-even (- n 1)))))
      (list (is-even 10000) (is-odd 9999)))"#;

    let result = eval(code);
    assert!(
        result.is_ok(),
        "Deep mutual tail recursion should succeed: {:?}",
        result
    );
    let vec = result.unwrap().list_to_vec().unwrap();
    assert_eq!(vec[0], Value::Bool(true));
    assert_eq!(vec[1], Value::Bool(true));
}
