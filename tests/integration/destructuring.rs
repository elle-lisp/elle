// Integration tests for destructuring patterns in def, var, let, let*, and fn
use elle::ffi::primitives::context::set_symbol_table;
use elle::pipeline::{compile, compile_all};
use elle::primitives::register_primitives;
use elle::{SymbolTable, Value, VM};

fn eval(input: &str) -> Result<Value, String> {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    let _effects = register_primitives(&mut vm, &mut symbols);
    set_symbol_table(&mut symbols as *mut SymbolTable);

    match compile(input, &mut symbols) {
        Ok(result) => vm.execute(&result.bytecode).map_err(|e| e.to_string()),
        Err(_) => {
            let wrapped = format!("(begin {})", input);
            match compile(&wrapped, &mut symbols) {
                Ok(result) => vm.execute(&result.bytecode).map_err(|e| e.to_string()),
                Err(_) => {
                    let results = compile_all(input, &mut symbols)?;
                    let mut last_result = Value::NIL;
                    for result in results {
                        last_result = vm.execute(&result.bytecode).map_err(|e| e.to_string())?;
                    }
                    Ok(last_result)
                }
            }
        }
    }
}

// === def: list destructuring ===

#[test]
fn test_def_list_basic() {
    assert_eq!(
        eval("(begin (def (a b c) (list 1 2 3)) a)").unwrap(),
        Value::int(1)
    );
    assert_eq!(
        eval("(begin (def (a b c) (list 1 2 3)) b)").unwrap(),
        Value::int(2)
    );
    assert_eq!(
        eval("(begin (def (a b c) (list 1 2 3)) c)").unwrap(),
        Value::int(3)
    );
}

#[test]
fn test_def_list_short_source() {
    // Missing elements become nil
    assert_eq!(
        eval("(begin (def (a b c) (list 1)) a)").unwrap(),
        Value::int(1)
    );
    assert_eq!(
        eval("(begin (def (a b c) (list 1)) b)").unwrap(),
        Value::NIL
    );
    assert_eq!(
        eval("(begin (def (a b c) (list 1)) c)").unwrap(),
        Value::NIL
    );
}

#[test]
fn test_def_list_empty_source() {
    assert_eq!(eval("(begin (def (a b) (list)) a)").unwrap(), Value::NIL);
    assert_eq!(eval("(begin (def (a b) (list)) b)").unwrap(), Value::NIL);
}

#[test]
fn test_def_list_extra_elements_ignored() {
    // More elements than bindings — extras are silently dropped
    assert_eq!(
        eval("(begin (def (a b) (list 1 2 3 4)) a)").unwrap(),
        Value::int(1)
    );
    assert_eq!(
        eval("(begin (def (a b) (list 1 2 3 4)) b)").unwrap(),
        Value::int(2)
    );
}

#[test]
fn test_def_list_wrong_type_gives_nil() {
    // Destructuring a non-list gives nil for all bindings
    assert_eq!(eval("(begin (def (a b) 42) a)").unwrap(), Value::NIL);
    assert_eq!(eval("(begin (def (a b) 42) b)").unwrap(), Value::NIL);
}

// === def: array destructuring ===

#[test]
fn test_def_array_basic() {
    assert_eq!(
        eval("(begin (def [x y] [10 20]) x)").unwrap(),
        Value::int(10)
    );
    assert_eq!(
        eval("(begin (def [x y] [10 20]) y)").unwrap(),
        Value::int(20)
    );
}

#[test]
fn test_def_array_short_source() {
    assert_eq!(
        eval("(begin (def [x y z] [10]) x)").unwrap(),
        Value::int(10)
    );
    assert_eq!(eval("(begin (def [x y z] [10]) y)").unwrap(), Value::NIL);
    assert_eq!(eval("(begin (def [x y z] [10]) z)").unwrap(), Value::NIL);
}

#[test]
fn test_def_array_wrong_type_gives_nil() {
    assert_eq!(eval("(begin (def [a b] 42) a)").unwrap(), Value::NIL);
}

// === def: nested destructuring ===

#[test]
fn test_def_nested_list() {
    assert_eq!(
        eval("(begin (def ((a b) c) (list (list 1 2) 3)) a)").unwrap(),
        Value::int(1)
    );
    assert_eq!(
        eval("(begin (def ((a b) c) (list (list 1 2) 3)) b)").unwrap(),
        Value::int(2)
    );
    assert_eq!(
        eval("(begin (def ((a b) c) (list (list 1 2) 3)) c)").unwrap(),
        Value::int(3)
    );
}

#[test]
fn test_def_nested_array_in_list() {
    assert_eq!(
        eval("(begin (def ([x y] z) (list [10 20] 30)) x)").unwrap(),
        Value::int(10)
    );
    assert_eq!(
        eval("(begin (def ([x y] z) (list [10 20] 30)) y)").unwrap(),
        Value::int(20)
    );
    assert_eq!(
        eval("(begin (def ([x y] z) (list [10 20] 30)) z)").unwrap(),
        Value::int(30)
    );
}

// === def: immutability ===

#[test]
fn test_def_destructured_bindings_are_immutable() {
    let result = eval("(begin (def (a b) (list 1 2)) (set! a 10))");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("immutable"));
}

// === var: mutable destructuring ===

#[test]
fn test_var_list_basic() {
    assert_eq!(
        eval("(begin (var (a b) (list 1 2)) a)").unwrap(),
        Value::int(1)
    );
    assert_eq!(
        eval("(begin (var (a b) (list 1 2)) b)").unwrap(),
        Value::int(2)
    );
}

#[test]
fn test_var_destructured_bindings_are_mutable() {
    assert_eq!(
        eval("(begin (var (a b) (list 1 2)) (set! a 10) a)").unwrap(),
        Value::int(10)
    );
}

// === let: destructuring in bindings ===

#[test]
fn test_let_list_destructure() {
    assert_eq!(
        eval("(let (((a b) (list 10 20))) (+ a b))").unwrap(),
        Value::int(30)
    );
}

#[test]
fn test_let_array_destructure() {
    assert_eq!(
        eval("(let (([x y] [3 4])) (+ x y))").unwrap(),
        Value::int(7)
    );
}

#[test]
fn test_let_mixed_bindings() {
    // Mix of simple and destructured bindings
    assert_eq!(
        eval("(let ((a 1) ((b c) (list 2 3))) (+ a b c))").unwrap(),
        Value::int(6)
    );
}

#[test]
fn test_let_nested_destructure() {
    assert_eq!(
        eval("(let ((((a b) c) (list (list 1 2) 3))) (+ a b c))").unwrap(),
        Value::int(6)
    );
}

// === let*: sequential destructuring ===

#[test]
fn test_let_star_destructure_basic() {
    assert_eq!(
        eval("(let* (((a b) (list 1 2)) (c (+ a b))) c)").unwrap(),
        Value::int(3)
    );
}

#[test]
fn test_let_star_destructure_sequential_reference() {
    // Second destructure references first
    assert_eq!(
        eval("(let* (((a b) (list 1 2)) ((c d) (list a b))) (+ c d))").unwrap(),
        Value::int(3)
    );
}

#[test]
fn test_let_star_mixed_simple_and_destructure() {
    assert_eq!(
        eval("(let* ((x 10) ((a b) (list x 20))) (+ a b))").unwrap(),
        Value::int(30)
    );
}

#[test]
fn test_let_star_shadowing_with_destructure() {
    // Rebind via destructuring
    assert_eq!(
        eval("(let* ((a 1) ((a b) (list 10 20))) a)").unwrap(),
        Value::int(10)
    );
}

// === fn: parameter destructuring ===

#[test]
fn test_fn_list_param() {
    assert_eq!(
        eval("((fn ((a b)) (+ a b)) (list 3 4))").unwrap(),
        Value::int(7)
    );
}

#[test]
fn test_fn_array_param() {
    assert_eq!(
        eval("((fn ([x y]) (+ x y)) [5 6])").unwrap(),
        Value::int(11)
    );
}

#[test]
fn test_fn_mixed_params() {
    assert_eq!(
        eval("((fn (x (a b)) (+ x a b)) 10 (list 20 30))").unwrap(),
        Value::int(60)
    );
}

#[test]
fn test_fn_nested_param() {
    assert_eq!(
        eval("((fn (((a b) c)) (+ a b c)) (list (list 1 2) 3))").unwrap(),
        Value::int(6)
    );
}

// === defn: destructuring in named function params ===

#[test]
fn test_defn_with_destructured_param() {
    assert_eq!(
        eval("(begin (defn f ((a b)) (+ a b)) (f (list 3 4)))").unwrap(),
        Value::int(7)
    );
}

#[test]
fn test_defn_mixed_params() {
    assert_eq!(
        eval("(begin (defn f (x (a b)) (+ x a b)) (f 10 (list 20 30)))").unwrap(),
        Value::int(60)
    );
}

// === Edge cases ===

#[test]
fn test_destructure_single_element_list() {
    assert_eq!(
        eval("(begin (def (a) (list 42)) a)").unwrap(),
        Value::int(42)
    );
}

#[test]
fn test_destructure_single_element_array() {
    assert_eq!(eval("(begin (def [a] [42]) a)").unwrap(), Value::int(42));
}

#[test]
fn test_destructure_string_values() {
    assert_eq!(
        eval(r#"(begin (def (a b) (list "hello" "world")) a)"#).unwrap(),
        Value::string("hello")
    );
}

#[test]
fn test_destructure_boolean_values() {
    assert_eq!(
        eval("(begin (def (a b) (list #t #f)) a)").unwrap(),
        Value::bool(true)
    );
    assert_eq!(
        eval("(begin (def (a b) (list #t #f)) b)").unwrap(),
        Value::bool(false)
    );
}

#[test]
fn test_destructure_nil_in_list() {
    assert_eq!(
        eval("(begin (def (a b) (list nil 2)) a)").unwrap(),
        Value::NIL
    );
    assert_eq!(
        eval("(begin (def (a b) (list nil 2)) b)").unwrap(),
        Value::int(2)
    );
}

#[test]
fn test_destructure_in_closure_capture() {
    assert_eq!(
        eval("(begin (def (a b) (list 1 2)) (def f (fn () (+ a b))) (f))").unwrap(),
        Value::int(3)
    );
}

#[test]
fn test_let_destructure_in_closure() {
    assert_eq!(
        eval("(let (((a b) (list 10 20))) ((fn () (+ a b))))").unwrap(),
        Value::int(30)
    );
}

// === Wildcard _ ===

#[test]
fn test_wildcard_list_basic() {
    // Skip first element
    assert_eq!(
        eval("(begin (def (_ b) (list 1 2)) b)").unwrap(),
        Value::int(2)
    );
}

#[test]
fn test_wildcard_list_middle() {
    // Skip middle element
    assert_eq!(
        eval("(begin (def (a _ c) (list 1 2 3)) (+ a c))").unwrap(),
        Value::int(4)
    );
}

#[test]
fn test_wildcard_array_basic() {
    assert_eq!(
        eval("(begin (def [_ y] [10 20]) y)").unwrap(),
        Value::int(20)
    );
}

#[test]
fn test_wildcard_multiple() {
    // Multiple wildcards
    assert_eq!(
        eval("(begin (def (_ _ c) (list 1 2 3)) c)").unwrap(),
        Value::int(3)
    );
}

#[test]
fn test_wildcard_in_let() {
    assert_eq!(
        eval("(let (((_ b) (list 10 20))) b)").unwrap(),
        Value::int(20)
    );
}

#[test]
fn test_wildcard_in_fn_param() {
    assert_eq!(
        eval("((fn ((_ b)) b) (list 10 20))").unwrap(),
        Value::int(20)
    );
}

#[test]
fn test_wildcard_nested() {
    // Wildcard in nested destructuring
    assert_eq!(
        eval("(begin (def ((_ b) c) (list (list 1 2) 3)) (+ b c))").unwrap(),
        Value::int(5)
    );
}

// === & rest: list destructuring ===

#[test]
fn test_rest_list_basic() {
    // Collect remaining elements
    assert_eq!(
        eval("(begin (def (a & r) (list 1 2 3)) a)").unwrap(),
        Value::int(1)
    );
    // r should be (2 3)
    assert_eq!(
        eval("(begin (def (a & r) (list 1 2 3)) (first r))").unwrap(),
        Value::int(2)
    );
    assert_eq!(
        eval("(begin (def (a & r) (list 1 2 3)) (first (rest r)))").unwrap(),
        Value::int(3)
    );
}

#[test]
fn test_rest_list_empty_rest() {
    // When all elements are consumed, rest is empty list (cdr of last cons)
    assert_eq!(
        eval("(begin (def (a b & r) (list 1 2)) r)").unwrap(),
        Value::EMPTY_LIST
    );
}

#[test]
fn test_rest_list_single_rest() {
    assert_eq!(
        eval("(begin (def (a & r) (list 1)) r)").unwrap(),
        Value::EMPTY_LIST
    );
}

#[test]
fn test_rest_list_all_rest() {
    // No fixed elements, just rest
    assert_eq!(
        eval("(begin (def (& r) (list 1 2 3)) (first r))").unwrap(),
        Value::int(1)
    );
}

#[test]
fn test_rest_list_in_let() {
    assert_eq!(
        eval("(let (((a & r) (list 10 20 30))) (+ a (first r)))").unwrap(),
        Value::int(30)
    );
}

#[test]
fn test_rest_list_in_fn_param() {
    assert_eq!(
        eval("((fn ((a & r)) (+ a (first r))) (list 10 20))").unwrap(),
        Value::int(30)
    );
}

// === & rest: array destructuring ===

#[test]
fn test_rest_array_basic() {
    assert_eq!(
        eval("(begin (def [a & r] [1 2 3]) a)").unwrap(),
        Value::int(1)
    );
    // r should be [2 3]
    assert_eq!(
        eval("(begin (def [a & r] [1 2 3]) (array-ref r 0))").unwrap(),
        Value::int(2)
    );
    assert_eq!(
        eval("(begin (def [a & r] [1 2 3]) (array-ref r 1))").unwrap(),
        Value::int(3)
    );
}

#[test]
fn test_rest_array_empty_rest() {
    assert_eq!(
        eval("(begin (def [a b & r] [1 2]) (length r))").unwrap(),
        Value::int(0)
    );
}

#[test]
fn test_rest_array_in_let() {
    assert_eq!(
        eval("(let (([a & r] [10 20 30])) (+ a (array-ref r 0)))").unwrap(),
        Value::int(30)
    );
}

// === Wildcard + rest combined ===

#[test]
fn test_wildcard_with_rest() {
    // Skip first, collect rest
    assert_eq!(
        eval("(begin (def (_ & r) (list 1 2 3)) (first r))").unwrap(),
        Value::int(2)
    );
}

#[test]
fn test_wildcard_and_rest_array() {
    assert_eq!(
        eval("(begin (def [_ & r] [10 20 30]) (array-ref r 0))").unwrap(),
        Value::int(20)
    );
}

// ============================================================
// Variadic & rest in fn/lambda parameters
// ============================================================

#[test]
fn test_variadic_fn_rest_only() {
    // (fn (& args) args) — all args collected into a list
    assert_eq!(
        eval("((fn (& args) args) 1 2 3)").unwrap(),
        eval("(list 1 2 3)").unwrap()
    );
}

#[test]
fn test_variadic_fn_rest_empty() {
    // No extra args → rest is empty list
    assert_eq!(eval("((fn (& args) args))").unwrap(), Value::EMPTY_LIST);
}

#[test]
fn test_variadic_fn_fixed_and_rest() {
    // (fn (a b & rest) ...) — first two are fixed, rest collected
    assert_eq!(
        eval("((fn (a b & rest) (+ a b)) 10 20 30 40)").unwrap(),
        Value::int(30)
    );
}

#[test]
fn test_variadic_fn_rest_value() {
    // Check the rest parameter value
    assert_eq!(
        eval("((fn (a & rest) rest) 1 2 3)").unwrap(),
        eval("(list 2 3)").unwrap()
    );
}

#[test]
fn test_variadic_fn_rest_single_extra() {
    assert_eq!(
        eval("((fn (a & rest) rest) 1 2)").unwrap(),
        eval("(list 2)").unwrap()
    );
}

#[test]
fn test_variadic_fn_rest_no_extra() {
    // No extra args beyond fixed → rest is empty list
    assert_eq!(eval("((fn (a & rest) rest) 1)").unwrap(), Value::EMPTY_LIST);
}

#[test]
fn test_variadic_defn() {
    // defn with variadic params
    assert_eq!(
        eval("(begin (defn my-list (& items) items) (my-list 1 2 3))").unwrap(),
        eval("(list 1 2 3)").unwrap()
    );
}

#[test]
fn test_variadic_defn_fixed_and_rest() {
    assert_eq!(
        eval("(begin (defn f (x & rest) (cons x rest)) (f 1 2 3))").unwrap(),
        eval("(list 1 2 3)").unwrap()
    );
}

#[test]
fn test_variadic_let_binding() {
    // let binding with variadic lambda
    assert_eq!(
        eval("(let ((f (fn (& args) args))) (f 10 20))").unwrap(),
        eval("(list 10 20)").unwrap()
    );
}

#[test]
fn test_variadic_arity_check_too_few() {
    // (fn (a b & rest) ...) requires at least 2 args
    assert!(eval("((fn (a b & rest) a) 1)").is_err());
}

#[test]
fn test_variadic_recursive() {
    // Recursive variadic function
    assert_eq!(
        eval(
            "(begin
            (defn my-len (& args)
                (def lst (first args))
                (if (empty? lst) 0
                    (+ 1 (my-len (rest lst)))))
            (my-len (list 1 2 3)))"
        )
        .unwrap(),
        Value::int(3)
    );
}

#[test]
fn test_variadic_tail_call() {
    // Tail-recursive variadic function (non-variadic self-call for clean recursion)
    assert_eq!(
        eval(
            "(begin
            (defn sum-list (acc lst)
                (if (empty? lst) acc
                    (sum-list (+ acc (first lst)) (rest lst))))
            (defn sum-all (& nums)
                (sum-list 0 nums))
            (sum-all 1 2 3 4 5))"
        )
        .unwrap(),
        Value::int(15)
    );
}

#[test]
fn test_variadic_closure_capture() {
    // Variadic function that captures a variable
    assert_eq!(
        eval(
            "(begin
            (def x 100)
            (defn add-to-x (& nums)
                (+ x (first nums)))
            (add-to-x 42))"
        )
        .unwrap(),
        Value::int(142)
    );
}

#[test]
fn test_variadic_higher_order() {
    // Pass variadic function as argument
    assert_eq!(
        eval(
            "(begin
            (defn apply-fn (f & args)
                (f (first args)))
            (apply-fn (fn (x) (+ x 1)) 10))"
        )
        .unwrap(),
        Value::int(11)
    );
}

#[test]
fn test_variadic_compile_time_arity_check() {
    // Compile-time arity check should work for variadic functions
    // This should succeed (at least 1 arg)
    assert!(eval("(begin (defn f (x & rest) x) (f 1))").is_ok());
    // This should fail at compile time (0 args, needs at least 1)
    assert!(eval("(begin (defn f (x & rest) x) (f))").is_err());
}
