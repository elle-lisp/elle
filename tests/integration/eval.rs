// Integration tests for the `eval` special form
use crate::common::eval_source;
use elle::Value;

// === Basic eval ===

#[test]
fn test_eval_literal() {
    assert_eq!(eval_source("(eval '42)").unwrap(), Value::int(42));
}

#[test]
fn test_eval_string_literal() {
    assert_eq!(
        eval_source("(eval '\"hello\")").unwrap(),
        Value::string("hello")
    );
}

#[test]
fn test_eval_boolean() {
    assert_eq!(eval_source("(eval '#t)").unwrap(), Value::TRUE);
    assert_eq!(eval_source("(eval '#f)").unwrap(), Value::FALSE);
}

#[test]
fn test_eval_nil() {
    assert_eq!(eval_source("(eval 'nil)").unwrap(), Value::NIL);
}

#[test]
fn test_eval_quoted_expression() {
    assert_eq!(eval_source("(eval '(+ 1 2))").unwrap(), Value::int(3));
}

#[test]
fn test_eval_list_construction() {
    assert_eq!(eval_source("(eval (list '+ 1 2))").unwrap(), Value::int(3));
}

// === Env argument handling ===

#[test]
fn test_eval_with_struct_env() {
    assert_eq!(
        eval_source("(eval '(+ x y) {:x 10 :y 20})").unwrap(),
        Value::int(30)
    );
}

#[test]
fn test_eval_with_mutable_table_env() {
    // Mutable tables (created with `table`) should also work as env
    assert_eq!(
        eval_source("(eval '(+ x y) @{:x 10 :y 20})").unwrap(),
        Value::int(30)
    );
}

#[test]
fn test_eval_with_nil_env() {
    assert_eq!(eval_source("(eval '(+ 3 4) nil)").unwrap(), Value::int(7));
}

#[test]
fn test_eval_with_empty_table_env() {
    // Empty mutable table as env
    assert_eq!(
        eval_source("(eval '(+ 1 2) (table))").unwrap(),
        Value::int(3)
    );
}

#[test]
fn test_eval_env_invalid_type() {
    // env must be table/struct/nil — string should error
    let result = eval_source("(eval '42 \"bad\")");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("table or struct"),
        "Expected 'table or struct' in error, got: {}",
        err
    );
}

#[test]
fn test_eval_env_integer_invalid() {
    let result = eval_source("(eval '42 123)");
    assert!(result.is_err());
}

// === Prelude macros in eval'd code ===

#[test]
fn test_eval_with_when_macro() {
    assert_eq!(eval_source("(eval '(when #t 42))").unwrap(), Value::int(42));
}

#[test]
fn test_eval_with_unless_macro() {
    assert_eq!(
        eval_source("(eval '(unless #f 99))").unwrap(),
        Value::int(99)
    );
}

#[test]
fn test_eval_with_defn_macro() {
    // defn in eval'd code should work (defines and calls a function)
    assert_eq!(
        eval_source("(eval '(begin (defn f (x) (* x x)) (f 5)))").unwrap(),
        Value::int(25)
    );
}

#[test]
fn test_eval_with_let_star_macro() {
    assert_eq!(
        eval_source("(eval '(let* ((x 1) (y (+ x 1))) (+ x y)))").unwrap(),
        Value::int(3)
    );
}

#[test]
fn test_eval_with_thread_first() {
    assert_eq!(
        eval_source("(eval '(-> 5 (+ 3) (* 2)))").unwrap(),
        Value::int(16)
    );
}

// === Closures and scoping in eval'd code ===

#[test]
fn test_eval_with_closure() {
    assert_eq!(
        eval_source("(eval '(let ((x 1)) ((fn () x))))").unwrap(),
        Value::int(1)
    );
}

#[test]
fn test_eval_with_higher_order_function() {
    assert_eq!(
        eval_source("(eval '(let ((f (fn (x) (+ x 1)))) (f 41)))").unwrap(),
        Value::int(42)
    );
}

// === Eval in various contexts ===

#[test]
fn test_eval_inside_let() {
    assert_eq!(
        eval_source("(let ((x 10)) (eval '(+ 1 2)))").unwrap(),
        Value::int(3)
    );
}

#[test]
fn test_eval_inside_lambda() {
    assert_eq!(eval_source("((fn () (eval '42)))").unwrap(), Value::int(42));
}

#[test]
fn test_eval_result_in_computation() {
    assert_eq!(eval_source("(+ 1 (eval '2))").unwrap(), Value::int(3));
}

#[test]
fn test_eval_result_in_let_binding() {
    assert_eq!(
        eval_source("(let ((x (eval '42))) (+ x 1))").unwrap(),
        Value::int(43)
    );
}

#[test]
fn test_eval_in_conditional() {
    assert_eq!(eval_source("(if (eval '#t) 1 2)").unwrap(), Value::int(1));
}

// === Nested eval ===

#[test]
fn test_eval_nested() {
    assert_eq!(eval_source("(eval '(eval '42))").unwrap(), Value::int(42));
}

// === Error handling ===

#[test]
fn test_eval_compilation_error() {
    // (if) with no args should fail during analysis
    let result = eval_source("(eval '(if))");
    assert!(result.is_err());
}

#[test]
fn test_eval_runtime_error_in_evald_code() {
    // Division by zero in eval'd code
    let result = eval_source("(eval '(/ 1 0))");
    assert!(result.is_err());
}

#[test]
fn test_eval_undefined_variable() {
    // Referencing an undefined variable in eval'd code
    let result = eval_source("(eval 'undefined_var)");
    assert!(result.is_err());
}

// === Sequential evals (expander caching) ===

#[test]
fn test_eval_sequential() {
    // Multiple evals in sequence test expander caching
    assert_eq!(
        eval_source("(begin (eval '(+ 1 2)) (eval '(* 3 4)))").unwrap(),
        Value::int(12)
    );
}

// === Eval with begin/block ===

#[test]
fn test_eval_begin_sequence() {
    assert_eq!(eval_source("(eval '(begin 1 2 3))").unwrap(), Value::int(3));
}

// === Eval with match ===

#[test]
fn test_eval_with_match() {
    assert_eq!(
        eval_source("(eval '(match 42 (42 \"found\") (_ \"not found\")))").unwrap(),
        Value::string("found")
    );
}

// === Eval with list operations ===

#[test]
fn test_eval_list_operations() {
    assert_eq!(
        eval_source("(eval '(first (list 1 2 3)))").unwrap(),
        Value::int(1)
    );
}

#[test]
fn test_eval_returns_list() {
    let result = eval_source("(eval '(list 1 2 3))").unwrap();
    // Verify it's a proper list by extracting first element
    let first = result.as_cons().expect("should be a cons cell");
    assert_eq!(first.first, Value::int(1));
}

// === read + eval pattern (REPL pattern) ===

#[test]
fn test_read_eval_pattern() {
    // The classic REPL pattern: read a string, then eval the result
    assert_eq!(
        eval_source("(eval (read \"(+ 1 2)\"))").unwrap(),
        Value::int(3)
    );
}

#[test]
fn test_read_eval_literal() {
    assert_eq!(eval_source("(eval (read \"42\"))").unwrap(), Value::int(42));
}

// === Eval with cond ===

#[test]
fn test_eval_with_cond() {
    assert_eq!(
        eval_source("(eval '(cond ((= 1 2) \"no\") (#t \"yes\")))").unwrap(),
        Value::string("yes")
    );
}

// === Eval with while loop ===

#[test]
fn test_eval_with_while() {
    // while returns nil
    assert_eq!(
        eval_source("(eval '(begin (var i 0) (while (< i 3) (set! i (+ i 1))) i))").unwrap(),
        Value::int(3)
    );
}

// === Eval with recursion ===

#[test]
fn test_eval_with_recursion() {
    assert_eq!(
        eval_source("(eval '(begin (defn fact (n) (if (= n 0) 1 (* n (fact (- n 1))))) (fact 5)))")
            .unwrap(),
        Value::int(120)
    );
}

// === Eval with array operations ===

#[test]
fn test_eval_with_array() {
    assert_eq!(
        eval_source("(eval '(array-ref [10 20 30] 1))").unwrap(),
        Value::int(20)
    );
}

// === Eval with string operations ===

#[test]
fn test_eval_with_string_ops() {
    assert_eq!(
        eval_source("(eval '(length \"hello\"))").unwrap(),
        Value::int(5)
    );
}

// === Eval with multiple env bindings ===

#[test]
fn test_eval_env_many_bindings() {
    assert_eq!(
        eval_source("(eval '(+ a (+ b (+ c d))) {:a 1 :b 2 :c 3 :d 4})").unwrap(),
        Value::int(10)
    );
}

// === Eval with env binding shadowing primitives ===

#[test]
fn test_eval_env_shadows_nothing() {
    // env bindings should work alongside primitives
    assert_eq!(
        eval_source("(eval '(+ x 1) {:x 41})").unwrap(),
        Value::int(42)
    );
}

// === Eval returns keyword ===

#[test]
fn test_eval_returns_keyword() {
    let result = eval_source("(eval ':hello)").unwrap();
    assert_eq!(result.as_keyword_name().unwrap(), "hello");
}

// === Eval with try/catch in eval'd code ===

#[test]
fn test_eval_with_try_catch() {
    // try/catch is a prelude macro — should work in eval'd code
    assert_eq!(
        eval_source("(eval '(try (/ 1 0) (catch (e) 42)))").unwrap(),
        Value::int(42)
    );
}
