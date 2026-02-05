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
fn test_mutual_recursion_even_odd_integration() {
    let code = r#"(begin
      (define is-even (lambda (n) (if (= n 0) #t (is-odd (- n 1)))))
      (define is-odd (lambda (n) (if (= n 0) #f (is-even (- n 1)))))
      (list (is-even 0) (is-even 100) (is-odd 99)))"#;

    let result = eval(code);
    assert!(result.is_ok());
    let vec = result.unwrap().list_to_vec().unwrap();
    assert_eq!(vec[0], Value::Bool(true));
    assert_eq!(vec[1], Value::Bool(true));
    assert_eq!(vec[2], Value::Bool(true));
}

#[test]
fn test_mutual_recursion_fibonacci_pattern() {
    let code = r#"(begin
      (define fib (lambda (n)
        (if (= n 0) 0
          (if (= n 1) 1
            (+ (fib (- n 1)) (fib (- n 2)))))))
      (define fib-pair (lambda (n)
        (list (fib n) (fib (- n 1)))))
      (fib-pair 5))"#;

    let result = eval(code);
    assert!(result.is_ok());
}

#[test]
fn test_mutual_recursion_three_way_integration() {
    let code = r#"(begin
      (define step-a (lambda (n) (if (= n 0) "A" (step-b (- n 1)))))
      (define step-b (lambda (n) (if (= n 0) "B" (step-c (- n 1)))))
      (define step-c (lambda (n) (if (= n 0) "C" (step-a (- n 1)))))
      (step-a 2))"#;

    let result = eval(code);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::String("C".into()));
}

#[test]
fn test_mutual_recursion_deeply_nested() {
    let code = r#"(begin
      (define is-even-deep (lambda (n)
        (if (= n 0) #t
          (if (< n 0) #f
            (is-odd-deep (- n 1))))))
      (define is-odd-deep (lambda (n)
        (if (= n 0) #f
          (if (< n 0) #t
            (is-even-deep (- n 1))))))
      (list (is-even-deep 100) (is-odd-deep 99)))"#;

    let result = eval(code);
    assert!(result.is_ok());
    let vec = result.unwrap().list_to_vec().unwrap();
    assert_eq!(vec[0], Value::Bool(true));
    assert_eq!(vec[1], Value::Bool(true));
}

#[test]
fn test_mutual_recursion_with_accumulation() {
    let code = r#"(begin
      (define collect-up (lambda (n max acc)
        (if (> n max) acc
          (collect-down (+ n 1) max (append acc (list n))))))
      (define collect-down (lambda (n max acc)
        (if (> n max) acc
          (collect-up (+ n 1) max (append acc (list n))))))
      (collect-up 1 5 (list)))"#;

    let result = eval(code);
    assert!(result.is_ok());
    let vec = result.unwrap().list_to_vec().unwrap();
    assert_eq!(vec.len(), 5);
}

#[test]
fn test_mutual_recursion_string_manipulation() {
    let code = r#"(begin
      (define build-pattern-a (lambda (n)
        (if (= n 0) "a"
          (string-append "A" (build-pattern-b (- n 1))))))
      (define build-pattern-b (lambda (n)
        (if (= n 0) "b"
          (string-append "B" (build-pattern-a (- n 1))))))
      (list (build-pattern-a 4) (build-pattern-b 3)))"#;

    let result = eval(code);
    assert!(result.is_ok());
}

#[test]
fn test_mutual_recursion_multiple_functions() {
    let code = r#"(begin
      (define step-a (lambda (n) (if (= n 0) "A" (step-b (- n 1)))))
      (define step-b (lambda (n) (if (= n 0) "B" (step-c (- n 1)))))
      (define step-c (lambda (n) (if (= n 0) "C" (step-a (- n 1)))))
      (list (step-a 5) (step-b 5) (step-c 5)))"#;

    let result = eval(code);
    assert!(result.is_ok());
}

#[test]
fn test_mutual_recursion_conditional_dispatch() {
    let code = r#"(begin
      (define process (lambda (item)
        (if (= (mod item 2) 0)
          (process-even item)
          (process-odd item))))
      (define process-even (lambda (n)
        (if (= n 0) "done-even"
          (process (- n 2)))))
      (define process-odd (lambda (n)
        (if (= n 1) "done-odd"
          (process (- n 2)))))
      (list (process 10) (process 11)))"#;

    let result = eval(code);
    assert!(result.is_ok());
    let vec = result.unwrap().list_to_vec().unwrap();
    assert_eq!(vec[0], Value::String("done-even".into()));
    assert_eq!(vec[1], Value::String("done-odd".into()));
}

#[test]
fn test_mutual_recursion_with_boolean_logic() {
    let code = r#"(begin
      (define is-valid-a (lambda (n)
        (if (< n 0) #f
          (if (= n 0) #t
            (is-valid-b (- n 1))))))
      (define is-valid-b (lambda (n)
        (if (< n 0) #t
          (if (= n 0) #f
            (is-valid-a (- n 1))))))
      (list (is-valid-a 0) (is-valid-a 5) (is-valid-b 4)))"#;

    let result = eval(code);
    assert!(result.is_ok());
    let vec = result.unwrap().list_to_vec().unwrap();
    assert_eq!(vec[0], Value::Bool(true));
    assert_eq!(vec[1], Value::Bool(false));
    assert_eq!(vec[2], Value::Bool(false));
}
