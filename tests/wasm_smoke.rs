//! Smoke test: compile Elle source through LIR → WASM → Wasmtime.

fn eval(source: &str) -> String {
    match elle::wasm::eval_wasm(source, "<test>") {
        Ok(result) => format!("{}", result),
        Err(e) => panic!("eval_wasm('{}') failed: {}", source, e),
    }
}

// --- Phase 0: arithmetic ---

#[test]
fn test_integer_literal() {
    assert_eq!(eval("42"), "42");
}

#[test]
fn test_add_integers() {
    assert_eq!(eval("(+ 1 2)"), "3");
}

#[test]
fn test_nested_arithmetic() {
    assert_eq!(eval("(+ (* 3 4) (- 10 5))"), "17");
}

#[test]
fn test_negative() {
    assert_eq!(eval("(- 0 5)"), "-5");
}

#[test]
fn test_boolean_literals() {
    assert_eq!(eval("true"), "true");
    assert_eq!(eval("false"), "false");
}

#[test]
fn test_nil() {
    assert_eq!(eval("nil"), "nil");
}

// --- Phase 1: control flow ---

#[test]
fn test_if_true() {
    assert_eq!(eval("(if true 1 2)"), "1");
}

#[test]
fn test_if_false() {
    assert_eq!(eval("(if false 1 2)"), "2");
}

#[test]
fn test_if_nil() {
    assert_eq!(eval("(if nil 1 2)"), "2");
}

#[test]
fn test_if_integer_truthy() {
    assert_eq!(eval("(if 0 1 2)"), "1");
}

#[test]
fn test_let_binding() {
    assert_eq!(eval("(let* [[x 10]] (+ x 1))"), "11");
}

#[test]
fn test_let_two_bindings() {
    assert_eq!(eval("(let* [[x 10] [y 20]] (+ x y))"), "30");
}

#[test]
fn test_if_with_comparison() {
    assert_eq!(eval("(if (> 5 3) (+ 1 2) (- 10 5))"), "3");
}

#[test]
fn test_nested_if() {
    assert_eq!(eval("(if true (if false 10 20) 30)"), "20");
}

// --- Phase 1: function calls (primitives) ---

#[test]
fn test_call_length() {
    assert_eq!(eval("(length [1 2 3])"), "3");
}

#[test]
fn test_call_cons() {
    assert_eq!(eval("(cons 1 (list 2 3))"), "(1 2 3)");
}

#[test]
fn test_call_not() {
    // `not` is a UnaryOp intrinsic, but explicit call still works
    assert_eq!(eval("(not false)"), "true");
}

#[test]
fn test_call_type() {
    assert_eq!(eval("(type 42)"), ":integer");
}

#[test]
fn test_call_empty() {
    assert_eq!(eval("(empty? ())"), "true");
    assert_eq!(eval("(empty? (list 1))"), "false");
}

// --- Phase 1: data operations ---

#[test]
fn test_array_literal() {
    assert_eq!(eval("[1 2 3]"), "[1 2 3]");
}

#[test]
fn test_first_rest() {
    assert_eq!(eval("(first (cons 1 2))"), "1");
    assert_eq!(eval("(rest (cons 1 2))"), "2");
}

#[test]
fn test_struct_access() {
    assert_eq!(eval("(get {:x 1 :y 2} :x)"), "1");
}

// --- Phase 1: closures ---

#[test]
fn test_lambda_call() {
    assert_eq!(eval("((fn [x] (+ x 1)) 42)"), "43");
}

#[test]
fn test_let_lambda() {
    assert_eq!(eval("(let* [[f (fn [x] (+ x 1))]] (f 42))"), "43");
}

#[test]
fn test_closure_capture() {
    // Closure captures a value from outer scope
    assert_eq!(eval("(let* [[x 10]] ((fn [y] (+ x y)) 5))"), "15");
}

#[test]
fn test_higher_order() {
    // Pass a primitive as an argument
    assert_eq!(eval("((fn [f x y] (f x y)) + 3 4)"), "7");
}

#[test]
fn test_multi_arg_lambda() {
    assert_eq!(eval("((fn [a b c] (+ a (+ b c))) 1 2 3)"), "6");
}

// --- Phase 1: strings + error handling ---

#[test]
fn test_string_literal() {
    assert_eq!(eval("\"hello\""), "hello");
}

#[test]
fn test_string_concat() {
    assert_eq!(eval("(string \"hello\" \" \" \"world\")"), "hello world");
}

#[test]
fn test_string_size() {
    assert_eq!(eval("(string/size-of \"hello\")"), "5");
}

// --- Phase 1: recursion ---

#[test]
fn test_recursive_factorial() {
    // defn at top-level uses letrec, enabling recursion
    assert_eq!(
        eval("(defn fact [n] (if (<= n 1) 1 (* n (fact (- n 1)))))\n(fact 5)"),
        "120"
    );
}

#[test]
fn test_recursive_sum() {
    assert_eq!(
        eval("(defn sum [n] (if (<= n 0) 0 (+ n (sum (- n 1)))))\n(sum 10)"),
        "55"
    );
}

#[test]
fn test_let_with_if() {
    assert_eq!(eval("(let* [[x (if true 10 20)]] (+ x 5))"), "15");
}
