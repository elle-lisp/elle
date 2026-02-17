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

#[test]
fn test_mutual_recursion_even_odd() {
    let code = r#"(begin
      (define is-even (lambda (n) (if (= n 0) #t (is-odd (- n 1)))))
      (define is-odd (lambda (n) (if (= n 0) #f (is-even (- n 1)))))
      (is-even 4))"#;

    let result = eval(code).unwrap();
    assert_eq!(result, Value::bool(true));
}

#[test]
fn test_mutual_recursion_simple_alternation() {
    let code = r#"(begin
      (define func-a (lambda (n) (if (= n 0) 0 (func-b (- n 1)))))
      (define func-b (lambda (n) (if (= n 0) 1 (func-a (- n 1)))))
      (list (func-a 0) (func-a 1) (func-a 2) (func-a 5)))"#;

    let result = eval(code).unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 4);
    assert_eq!(vec[0], Value::int(0));
    assert_eq!(vec[1], Value::int(1));
    assert_eq!(vec[2], Value::int(0));
    assert_eq!(vec[3], Value::int(1));
}

#[test]
fn test_mutual_recursion_three_way() {
    let code = r#"(begin
      (define func-x (lambda (n) (if (= n 0) "x" (func-y (- n 1)))))
      (define func-y (lambda (n) (if (= n 0) "y" (func-z (- n 1)))))
      (define func-z (lambda (n) (if (= n 0) "z" (func-x (- n 1)))))
      (list (func-x 0) (func-x 1) (func-x 2) (func-x 3)))"#;

    let result = eval(code).unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 4);
    assert_eq!(vec[0], Value::string("x"));
    assert_eq!(vec[1], Value::string("y"));
    assert_eq!(vec[2], Value::string("z"));
    assert_eq!(vec[3], Value::string("x"));
}

#[test]
fn test_mutual_recursion_accumulator() {
    let code = r#"(begin
      (define count-up (lambda (n max acc)
        (if (> n max) acc (count-down (+ n 1) max (+ acc 1)))))
      (define count-down (lambda (n max acc)
        (if (> n max) acc (count-up (+ n 1) max (+ acc 1)))))
      (list (count-up 1 1 0) (count-up 1 2 0) (count-up 1 5 0)))"#;

    let result = eval(code).unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 3);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(2));
    assert_eq!(vec[2], Value::int(5));
}

#[test]
fn test_mutual_recursion_with_conditions() {
    let code = r#"(begin
      (define is-positive (lambda (n)
        (if (> n 0) #t
          (if (= n 0) #f (is-negative (+ n 1))))))
      (define is-negative (lambda (n)
        (if (< n 0) #t
          (if (= n 0) #f (is-positive (- n 1))))))
      (is-positive 5))"#;

    let result = eval(code).unwrap();
    assert_eq!(result, Value::bool(true));
}

#[test]
fn test_mutual_recursion_mutual_calls() {
    let code = r#"(begin
      (define f (lambda (x) (if (= x 0) 100 (g (- x 1)))))
      (define g (lambda (x) (if (= x 0) 200 (f (- x 1)))))
      (list (f 0) (f 1) (f 2) (g 0) (g 1)))"#;

    let result = eval(code).unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 5);
    assert_eq!(vec[0], Value::int(100));
    assert_eq!(vec[1], Value::int(200));
    assert_eq!(vec[2], Value::int(100));
    assert_eq!(vec[3], Value::int(200));
    assert_eq!(vec[4], Value::int(100));
}

#[test]
fn test_mutual_recursion_string_building() {
    let code = r#"(begin
      (define build-a (lambda (n)
        (if (= n 0) "a"
          (string-append "A" (build-b (- n 1))))))
      (define build-b (lambda (n)
        (if (= n 0) "b"
          (string-append "B" (build-a (- n 1))))))
      (build-a 0))"#;

    let result = eval(code).unwrap();
    assert_eq!(result, Value::string("a"));
}

#[test]
fn test_mutual_recursion_forward_references() {
    let code = r#"(begin
      (define uses-later (lambda () (later-function)))
      (define later-function (lambda () 42))
      (uses-later))"#;

    let result = eval(code).unwrap();
    assert_eq!(result, Value::int(42));
}

#[test]
fn test_mutual_recursion_with_multiple_calls() {
    let code = r#"(begin
      (define mult-a (lambda (n)
        (if (= n 0) 1
          (+ (mult-b (- n 1)) (mult-b (- n 1))))))
      (define mult-b (lambda (n)
        (if (= n 0) 1
          (+ (mult-a (- n 1)) (mult-a (- n 1))))))
      (list (mult-a 0) (mult-a 1) (mult-a 2)))"#;

    let result = eval(code).unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 3);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(2));
    assert_eq!(vec[2], Value::int(4));
}
