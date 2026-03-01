// Integration tests for destructuring patterns in def, var, let, let*, and fn
use crate::common::eval_source;
use elle::Value;

// === def: list destructuring ===

#[test]
fn test_def_list_basic() {
    assert_eq!(
        eval_source("(begin (def (a b c) (list 1 2 3)) a)").unwrap(),
        Value::int(1)
    );
    assert_eq!(
        eval_source("(begin (def (a b c) (list 1 2 3)) b)").unwrap(),
        Value::int(2)
    );
    assert_eq!(
        eval_source("(begin (def (a b c) (list 1 2 3)) c)").unwrap(),
        Value::int(3)
    );
}

#[test]
fn test_def_list_short_source() {
    // Missing elements become nil
    assert_eq!(
        eval_source("(begin (def (a b c) (list 1)) a)").unwrap(),
        Value::int(1)
    );
    assert_eq!(
        eval_source("(begin (def (a b c) (list 1)) b)").unwrap(),
        Value::NIL
    );
    assert_eq!(
        eval_source("(begin (def (a b c) (list 1)) c)").unwrap(),
        Value::NIL
    );
}

#[test]
fn test_def_list_empty_source() {
    assert_eq!(
        eval_source("(begin (def (a b) (list)) a)").unwrap(),
        Value::NIL
    );
    assert_eq!(
        eval_source("(begin (def (a b) (list)) b)").unwrap(),
        Value::NIL
    );
}

#[test]
fn test_def_list_extra_elements_ignored() {
    // More elements than bindings — extras are silently dropped
    assert_eq!(
        eval_source("(begin (def (a b) (list 1 2 3 4)) a)").unwrap(),
        Value::int(1)
    );
    assert_eq!(
        eval_source("(begin (def (a b) (list 1 2 3 4)) b)").unwrap(),
        Value::int(2)
    );
}

#[test]
fn test_def_list_wrong_type_gives_nil() {
    // Destructuring a non-list gives nil for all bindings
    assert_eq!(eval_source("(begin (def (a b) 42) a)").unwrap(), Value::NIL);
    assert_eq!(eval_source("(begin (def (a b) 42) b)").unwrap(), Value::NIL);
}

// === def: array destructuring ===

#[test]
fn test_def_array_basic() {
    assert_eq!(
        eval_source("(begin (def [x y] [10 20]) x)").unwrap(),
        Value::int(10)
    );
    assert_eq!(
        eval_source("(begin (def [x y] [10 20]) y)").unwrap(),
        Value::int(20)
    );
}

#[test]
fn test_def_array_short_source() {
    assert_eq!(
        eval_source("(begin (def [x y z] [10]) x)").unwrap(),
        Value::int(10)
    );
    assert_eq!(
        eval_source("(begin (def [x y z] [10]) y)").unwrap(),
        Value::NIL
    );
    assert_eq!(
        eval_source("(begin (def [x y z] [10]) z)").unwrap(),
        Value::NIL
    );
}

#[test]
fn test_def_array_wrong_type_gives_nil() {
    assert_eq!(eval_source("(begin (def [a b] 42) a)").unwrap(), Value::NIL);
}

// === def: nested destructuring ===

#[test]
fn test_def_nested_list() {
    assert_eq!(
        eval_source("(begin (def ((a b) c) (list (list 1 2) 3)) a)").unwrap(),
        Value::int(1)
    );
    assert_eq!(
        eval_source("(begin (def ((a b) c) (list (list 1 2) 3)) b)").unwrap(),
        Value::int(2)
    );
    assert_eq!(
        eval_source("(begin (def ((a b) c) (list (list 1 2) 3)) c)").unwrap(),
        Value::int(3)
    );
}

#[test]
fn test_def_nested_array_in_list() {
    assert_eq!(
        eval_source("(begin (def ([x y] z) (list [10 20] 30)) x)").unwrap(),
        Value::int(10)
    );
    assert_eq!(
        eval_source("(begin (def ([x y] z) (list [10 20] 30)) y)").unwrap(),
        Value::int(20)
    );
    assert_eq!(
        eval_source("(begin (def ([x y] z) (list [10 20] 30)) z)").unwrap(),
        Value::int(30)
    );
}

// === def: immutability ===

#[test]
fn test_def_destructured_bindings_are_immutable() {
    let result = eval_source("(begin (def (a b) (list 1 2)) (set a 10))");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("immutable"));
}

// === var: mutable destructuring ===

#[test]
fn test_var_list_basic() {
    assert_eq!(
        eval_source("(begin (var (a b) (list 1 2)) a)").unwrap(),
        Value::int(1)
    );
    assert_eq!(
        eval_source("(begin (var (a b) (list 1 2)) b)").unwrap(),
        Value::int(2)
    );
}

#[test]
fn test_var_destructured_bindings_are_mutable() {
    assert_eq!(
        eval_source("(begin (var (a b) (list 1 2)) (set a 10) a)").unwrap(),
        Value::int(10)
    );
}

// === let: destructuring in bindings ===

#[test]
fn test_let_list_destructure() {
    assert_eq!(
        eval_source("(let (((a b) (list 10 20))) (+ a b))").unwrap(),
        Value::int(30)
    );
}

#[test]
fn test_let_array_destructure() {
    assert_eq!(
        eval_source("(let (([x y] [3 4])) (+ x y))").unwrap(),
        Value::int(7)
    );
}

#[test]
fn test_let_mixed_bindings() {
    // Mix of simple and destructured bindings
    assert_eq!(
        eval_source("(let ((a 1) ((b c) (list 2 3))) (+ a b c))").unwrap(),
        Value::int(6)
    );
}

#[test]
fn test_let_nested_destructure() {
    assert_eq!(
        eval_source("(let ((((a b) c) (list (list 1 2) 3))) (+ a b c))").unwrap(),
        Value::int(6)
    );
}

// === let*: sequential destructuring ===

#[test]
fn test_let_star_destructure_basic() {
    assert_eq!(
        eval_source("(let* (((a b) (list 1 2)) (c (+ a b))) c)").unwrap(),
        Value::int(3)
    );
}

#[test]
fn test_let_star_destructure_sequential_reference() {
    // Second destructure references first
    assert_eq!(
        eval_source("(let* (((a b) (list 1 2)) ((c d) (list a b))) (+ c d))").unwrap(),
        Value::int(3)
    );
}

#[test]
fn test_let_star_mixed_simple_and_destructure() {
    assert_eq!(
        eval_source("(let* ((x 10) ((a b) (list x 20))) (+ a b))").unwrap(),
        Value::int(30)
    );
}

#[test]
fn test_let_star_shadowing_with_destructure() {
    // Rebind via destructuring
    assert_eq!(
        eval_source("(let* ((a 1) ((a b) (list 10 20))) a)").unwrap(),
        Value::int(10)
    );
}

// === fn: parameter destructuring ===

#[test]
fn test_fn_list_param() {
    assert_eq!(
        eval_source("((fn ((a b)) (+ a b)) (list 3 4))").unwrap(),
        Value::int(7)
    );
}

#[test]
fn test_fn_array_param() {
    assert_eq!(
        eval_source("((fn ([x y]) (+ x y)) [5 6])").unwrap(),
        Value::int(11)
    );
}

#[test]
fn test_fn_mixed_params() {
    assert_eq!(
        eval_source("((fn (x (a b)) (+ x a b)) 10 (list 20 30))").unwrap(),
        Value::int(60)
    );
}

#[test]
fn test_fn_nested_param() {
    assert_eq!(
        eval_source("((fn (((a b) c)) (+ a b c)) (list (list 1 2) 3))").unwrap(),
        Value::int(6)
    );
}

// === defn: destructuring in named function params ===

#[test]
fn test_defn_with_destructured_param() {
    assert_eq!(
        eval_source("(begin (defn f ((a b)) (+ a b)) (f (list 3 4)))").unwrap(),
        Value::int(7)
    );
}

#[test]
fn test_defn_mixed_params() {
    assert_eq!(
        eval_source("(begin (defn f (x (a b)) (+ x a b)) (f 10 (list 20 30)))").unwrap(),
        Value::int(60)
    );
}

// === Edge cases ===

#[test]
fn test_destructure_single_element_list() {
    assert_eq!(
        eval_source("(begin (def (a) (list 42)) a)").unwrap(),
        Value::int(42)
    );
}

#[test]
fn test_destructure_single_element_array() {
    assert_eq!(
        eval_source("(begin (def [a] [42]) a)").unwrap(),
        Value::int(42)
    );
}

#[test]
fn test_destructure_string_values() {
    assert_eq!(
        eval_source(r#"(begin (def (a b) (list "hello" "world")) a)"#).unwrap(),
        Value::string("hello")
    );
}

#[test]
fn test_destructure_boolean_values() {
    assert_eq!(
        eval_source("(begin (def (a b) (list true false)) a)").unwrap(),
        Value::bool(true)
    );
    assert_eq!(
        eval_source("(begin (def (a b) (list true false)) b)").unwrap(),
        Value::bool(false)
    );
}

#[test]
fn test_destructure_nil_in_list() {
    assert_eq!(
        eval_source("(begin (def (a b) (list nil 2)) a)").unwrap(),
        Value::NIL
    );
    assert_eq!(
        eval_source("(begin (def (a b) (list nil 2)) b)").unwrap(),
        Value::int(2)
    );
}

#[test]
fn test_destructure_in_closure_capture() {
    assert_eq!(
        eval_source("(begin (def (a b) (list 1 2)) (def f (fn () (+ a b))) (f))").unwrap(),
        Value::int(3)
    );
}

#[test]
fn test_let_destructure_in_closure() {
    assert_eq!(
        eval_source("(let (((a b) (list 10 20))) ((fn () (+ a b))))").unwrap(),
        Value::int(30)
    );
}

// === Wildcard _ ===

#[test]
fn test_wildcard_list_basic() {
    // Skip first element
    assert_eq!(
        eval_source("(begin (def (_ b) (list 1 2)) b)").unwrap(),
        Value::int(2)
    );
}

#[test]
fn test_wildcard_list_middle() {
    // Skip middle element
    assert_eq!(
        eval_source("(begin (def (a _ c) (list 1 2 3)) (+ a c))").unwrap(),
        Value::int(4)
    );
}

#[test]
fn test_wildcard_array_basic() {
    assert_eq!(
        eval_source("(begin (def [_ y] [10 20]) y)").unwrap(),
        Value::int(20)
    );
}

#[test]
fn test_wildcard_multiple() {
    // Multiple wildcards
    assert_eq!(
        eval_source("(begin (def (_ _ c) (list 1 2 3)) c)").unwrap(),
        Value::int(3)
    );
}

#[test]
fn test_wildcard_in_let() {
    assert_eq!(
        eval_source("(let (((_ b) (list 10 20))) b)").unwrap(),
        Value::int(20)
    );
}

#[test]
fn test_wildcard_in_fn_param() {
    assert_eq!(
        eval_source("((fn ((_ b)) b) (list 10 20))").unwrap(),
        Value::int(20)
    );
}

#[test]
fn test_wildcard_nested() {
    // Wildcard in nested destructuring
    assert_eq!(
        eval_source("(begin (def ((_ b) c) (list (list 1 2) 3)) (+ b c))").unwrap(),
        Value::int(5)
    );
}

// === & rest: list destructuring ===

#[test]
fn test_rest_list_basic() {
    // Collect remaining elements
    assert_eq!(
        eval_source("(begin (def (a & r) (list 1 2 3)) a)").unwrap(),
        Value::int(1)
    );
    // r should be (2 3)
    assert_eq!(
        eval_source("(begin (def (a & r) (list 1 2 3)) (first r))").unwrap(),
        Value::int(2)
    );
    assert_eq!(
        eval_source("(begin (def (a & r) (list 1 2 3)) (first (rest r)))").unwrap(),
        Value::int(3)
    );
}

#[test]
fn test_rest_list_empty_rest() {
    // When all elements are consumed, rest is empty list (cdr of last cons)
    assert_eq!(
        eval_source("(begin (def (a b & r) (list 1 2)) r)").unwrap(),
        Value::EMPTY_LIST
    );
}

#[test]
fn test_rest_list_single_rest() {
    assert_eq!(
        eval_source("(begin (def (a & r) (list 1)) r)").unwrap(),
        Value::EMPTY_LIST
    );
}

#[test]
fn test_rest_list_all_rest() {
    // No fixed elements, just rest
    assert_eq!(
        eval_source("(begin (def (& r) (list 1 2 3)) (first r))").unwrap(),
        Value::int(1)
    );
}

#[test]
fn test_rest_list_in_let() {
    assert_eq!(
        eval_source("(let (((a & r) (list 10 20 30))) (+ a (first r)))").unwrap(),
        Value::int(30)
    );
}

#[test]
fn test_rest_list_in_fn_param() {
    assert_eq!(
        eval_source("((fn ((a & r)) (+ a (first r))) (list 10 20))").unwrap(),
        Value::int(30)
    );
}

// === & rest: array destructuring ===

#[test]
fn test_rest_array_basic() {
    assert_eq!(
        eval_source("(begin (def [a & r] [1 2 3]) a)").unwrap(),
        Value::int(1)
    );
    // r should be [2 3]
    assert_eq!(
        eval_source("(begin (def [a & r] [1 2 3]) (get r 0))").unwrap(),
        Value::int(2)
    );
    assert_eq!(
        eval_source("(begin (def [a & r] [1 2 3]) (get r 1))").unwrap(),
        Value::int(3)
    );
}

#[test]
fn test_rest_array_empty_rest() {
    assert_eq!(
        eval_source("(begin (def [a b & r] [1 2]) (length r))").unwrap(),
        Value::int(0)
    );
}

#[test]
fn test_rest_array_in_let() {
    assert_eq!(
        eval_source("(let (([a & r] [10 20 30])) (+ a (get r 0)))").unwrap(),
        Value::int(30)
    );
}

// === Wildcard + rest combined ===

#[test]
fn test_wildcard_with_rest() {
    // Skip first, collect rest
    assert_eq!(
        eval_source("(begin (def (_ & r) (list 1 2 3)) (first r))").unwrap(),
        Value::int(2)
    );
}

#[test]
fn test_wildcard_and_rest_array() {
    assert_eq!(
        eval_source("(begin (def [_ & r] [10 20 30]) (get r 0))").unwrap(),
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
        eval_source("((fn (& args) args) 1 2 3)").unwrap(),
        eval_source("(list 1 2 3)").unwrap()
    );
}

#[test]
fn test_variadic_fn_rest_empty() {
    // No extra args → rest is empty list
    assert_eq!(
        eval_source("((fn (& args) args))").unwrap(),
        Value::EMPTY_LIST
    );
}

#[test]
fn test_variadic_fn_fixed_and_rest() {
    // (fn (a b & rest) ...) — first two are fixed, rest collected
    assert_eq!(
        eval_source("((fn (a b & rest) (+ a b)) 10 20 30 40)").unwrap(),
        Value::int(30)
    );
}

#[test]
fn test_variadic_fn_rest_value() {
    // Check the rest parameter value
    assert_eq!(
        eval_source("((fn (a & rest) rest) 1 2 3)").unwrap(),
        eval_source("(list 2 3)").unwrap()
    );
}

#[test]
fn test_variadic_fn_rest_single_extra() {
    assert_eq!(
        eval_source("((fn (a & rest) rest) 1 2)").unwrap(),
        eval_source("(list 2)").unwrap()
    );
}

#[test]
fn test_variadic_fn_rest_no_extra() {
    // No extra args beyond fixed → rest is empty list
    assert_eq!(
        eval_source("((fn (a & rest) rest) 1)").unwrap(),
        Value::EMPTY_LIST
    );
}

#[test]
fn test_variadic_defn() {
    // defn with variadic params
    assert_eq!(
        eval_source("(begin (defn my-list (& items) items) (my-list 1 2 3))").unwrap(),
        eval_source("(list 1 2 3)").unwrap()
    );
}

#[test]
fn test_variadic_defn_fixed_and_rest() {
    assert_eq!(
        eval_source("(begin (defn f (x & rest) (cons x rest)) (f 1 2 3))").unwrap(),
        eval_source("(list 1 2 3)").unwrap()
    );
}

#[test]
fn test_variadic_let_binding() {
    // let binding with variadic lambda
    assert_eq!(
        eval_source("(let ((f (fn (& args) args))) (f 10 20))").unwrap(),
        eval_source("(list 10 20)").unwrap()
    );
}

#[test]
fn test_variadic_arity_check_too_few() {
    // (fn (a b & rest) ...) requires at least 2 args
    assert!(eval_source("((fn (a b & rest) a) 1)").is_err());
}

#[test]
fn test_variadic_recursive() {
    // Recursive variadic function
    assert_eq!(
        eval_source(
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
        eval_source(
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
        eval_source(
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
        eval_source(
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
    assert!(eval_source("(begin (defn f (x & rest) x) (f 1))").is_ok());
    // This should fail at compile time (0 args, needs at least 1)
    assert!(eval_source("(begin (defn f (x & rest) x) (f))").is_err());
}

// === Table/struct destructuring (edge cases) ===

#[test]
fn test_table_missing_key_is_nil() {
    assert_eq!(
        eval_source("(begin (def {:missing m} {:other 42}) m)").unwrap(),
        Value::NIL
    );
}

#[test]
fn test_table_wrong_type_is_nil() {
    assert_eq!(
        eval_source("(begin (def {:x x} 42) x)").unwrap(),
        Value::NIL
    );
}

#[test]
fn test_table_empty_pattern() {
    assert_eq!(
        eval_source("(begin (def {} {:x 1}) :ok)").unwrap(),
        Value::keyword("ok")
    );
}

#[test]
fn test_table_match_fallback() {
    assert_eq!(
        eval_source("(match 42 ({:x x} x) (_ :no-match))").unwrap(),
        Value::keyword("no-match")
    );
}

// === Table/struct destructuring ===

#[test]
fn test_def_table_basic() {
    assert_eq!(
        eval_source("(begin (def {:name n :age a} {:name \"Alice\" :age 30}) n)").unwrap(),
        Value::string("Alice")
    );
    assert_eq!(
        eval_source("(begin (def {:name n :age a} {:name \"Alice\" :age 30}) a)").unwrap(),
        Value::int(30)
    );
}

#[test]
fn test_def_table_missing_key() {
    // Missing key → nil (silent nil semantics)
    assert_eq!(
        eval_source("(begin (def {:missing m} {:other 42}) m)").unwrap(),
        Value::NIL
    );
}

#[test]
fn test_def_table_wrong_type() {
    // Non-table/struct value → nil for all bindings
    assert_eq!(
        eval_source("(begin (def {:x x} 42) x)").unwrap(),
        Value::NIL
    );
}

#[test]
fn test_var_table() {
    assert_eq!(
        eval_source("(begin (var {:x x} {:x 99}) x)").unwrap(),
        Value::int(99)
    );
}

#[test]
fn test_let_table() {
    assert_eq!(
        eval_source("(let (({:x x :y y} {:x 10 :y 20})) (+ x y))").unwrap(),
        Value::int(30)
    );
}

#[test]
fn test_let_star_table() {
    assert_eq!(
        eval_source("(let* (({:x x} {:x 5}) ({:y y} {:y x})) (+ x y))").unwrap(),
        Value::int(10)
    );
}

#[test]
fn test_fn_param_table() {
    assert_eq!(
        eval_source("(begin (defn f ({:x x :y y}) (+ x y)) (f {:x 3 :y 4}))").unwrap(),
        Value::int(7)
    );
}

#[test]
fn test_table_nested() {
    assert_eq!(
        eval_source("(begin (def {:point {:x px :y py}} {:point {:x 3 :y 4}}) (+ px py))").unwrap(),
        Value::int(7)
    );
}

#[test]
fn test_table_with_mutable_table() {
    // Destructuring works on mutable tables too
    assert_eq!(
        eval_source("(begin (def {:a a} @{:a 99}) a)").unwrap(),
        Value::int(99)
    );
}

#[test]
fn test_table_in_match() {
    assert_eq!(
        eval_source(
            "(match {:type :circle :radius 5}
               ({:type :circle :radius r} r)
               ({:type :square :side s} s)
               (_ 0))"
        )
        .unwrap(),
        Value::int(5)
    );
}

#[test]
fn test_table_match_fallthrough() {
    assert_eq!(
        eval_source(
            "(match {:type :square :side 7}
               ({:type :circle :radius r} r)
               ({:type :square :side s} s)
               (_ 0))"
        )
        .unwrap(),
        Value::int(7)
    );
}

#[test]
fn test_table_match_wildcard_fallback() {
    assert_eq!(
        eval_source(
            "(match 42
               ({:x x} x)
               (_ :no-match))"
        )
        .unwrap(),
        Value::keyword("no-match")
    );
}

#[test]
fn test_table_expression_position() {
    // {:a 1 :b 2} in expression position is a struct literal
    assert_eq!(eval_source("(get {:a 1 :b 2} :a)").unwrap(), Value::int(1));
}

#[test]
fn test_table_empty() {
    // Empty table destructuring
    assert_eq!(
        eval_source("(begin (def {} {:x 1}) :ok)").unwrap(),
        Value::keyword("ok")
    );
}

#[test]
fn test_table_mixed_with_list() {
    // Table inside list destructuring
    assert_eq!(
        eval_source("(begin (def (a {:x x}) (list 1 {:x 2})) (+ a x))").unwrap(),
        Value::int(3)
    );
}

#[test]
fn test_table_wildcard_value() {
    // Use _ for value to ignore it
    assert_eq!(
        eval_source("(begin (def {:x _ :y y} {:x 10 :y 20}) y)").unwrap(),
        Value::int(20)
    );
}

// === Tuple destructuring (bracket patterns on tuples) ===
// Tuples are immutable sequences. Currently the only way to get a tuple in
// user code is through error values (which are tuples [:kind "msg"]).
// The try/catch tests in prelude.rs cover the primary use case.
// These tests use a helper that captures an error value as a tuple.

fn make_error_tuple() -> &'static str {
    // Trigger a division-by-zero error and capture the error tuple
    "(let ((f (fiber/new (fn () (/ 1 0)) 1)))
       (fiber/resume f nil)
       (fiber/value f))"
}

#[test]
fn test_let_destructure_tuple() {
    // Error values are tuples — bracket destructuring should extract elements
    let src = format!("(let (([a b] {})) b)", make_error_tuple());
    let result = eval_source(&src);
    assert_eq!(result.unwrap(), Value::string("division by zero"));
}

#[test]
fn test_let_destructure_tuple_first() {
    let src = format!("(let (([a b] {})) a)", make_error_tuple());
    let result = eval_source(&src);
    assert_eq!(result.unwrap(), Value::keyword("division-by-zero"));
}

#[test]
fn test_match_tuple_pattern_matches_tuple() {
    // match [a b] should match tuples (immutable)
    let result = eval_source("(match [1 2] ([a b] (+ a b)) (_ :no-match))").unwrap();
    assert_eq!(result.as_int(), Some(3));
}

#[test]
fn test_match_tuple_pattern_does_not_match_array() {
    // match [a b] should NOT match arrays (mutable)
    let result = eval_source("(match @[1 2] ([a b] (+ a b)) (_ :no-match))").unwrap();
    assert_eq!(result, Value::keyword("no-match"));
}

#[test]
fn test_match_array_pattern_matches_array() {
    // match @[a b] should match arrays (mutable)
    let result = eval_source("(match @[1 2] (@[a b] (+ a b)) (_ :no-match))").unwrap();
    assert_eq!(result.as_int(), Some(3));
}

#[test]
fn test_match_array_pattern_does_not_match_tuple() {
    // match @[a b] should NOT match tuples (immutable)
    let result = eval_source("(match [1 2] (@[a b] (+ a b)) (_ :no-match))").unwrap();
    assert_eq!(result, Value::keyword("no-match"));
}

#[test]
fn test_match_struct_pattern_matches_struct() {
    // match {:a x} should match structs (immutable)
    let result = eval_source("(match {:a 1} ({:a x} x) (_ :no-match))").unwrap();
    assert_eq!(result.as_int(), Some(1));
}

#[test]
fn test_match_struct_pattern_does_not_match_table() {
    // match {:a x} should NOT match tables (mutable)
    let result = eval_source("(match @{:a 1} ({:a x} x) (_ :no-match))").unwrap();
    assert_eq!(result, Value::keyword("no-match"));
}

#[test]
fn test_match_table_pattern_matches_table() {
    // match @{:a x} should match tables (mutable)
    let result = eval_source("(match @{:a 1} (@{:a x} x) (_ :no-match))").unwrap();
    assert_eq!(result.as_int(), Some(1));
}

#[test]
fn test_match_table_pattern_does_not_match_struct() {
    // match @{:a x} should NOT match structs (immutable)
    let result = eval_source("(match {:a 1} (@{:a x} x) (_ :no-match))").unwrap();
    assert_eq!(result, Value::keyword("no-match"));
}

#[test]
fn test_destructure_non_sequential_gives_nil() {
    // Destructuring a non-array, non-tuple value gives nil (silent nil semantics)
    assert_eq!(eval_source("(let (([a b] 42)) a)").unwrap(), Value::NIL);
    assert_eq!(
        eval_source(r#"(let (([a b] "hello")) a)"#).unwrap(),
        Value::NIL
    );
}

#[test]
fn test_def_tuple_basic() {
    // Destructure an error tuple via def
    let src = format!("(begin (def [a b] {}) a)", make_error_tuple());
    assert_eq!(
        eval_source(&src).unwrap(),
        Value::keyword("division-by-zero")
    );
    let src = format!("(begin (def [a b] {}) b)", make_error_tuple());
    assert_eq!(
        eval_source(&src).unwrap(),
        Value::string("division by zero")
    );
}

// ============================================================
// &opt optional parameters
// ============================================================

#[test]
fn test_opt_basic_provided() {
    // Optional param receives the argument when provided
    assert_eq!(
        eval_source("((fn (a &opt b) b) 1 2)").unwrap(),
        Value::int(2)
    );
}

#[test]
fn test_opt_basic_missing() {
    // Optional param defaults to nil when not provided
    assert_eq!(eval_source("((fn (a &opt b) b) 1)").unwrap(), Value::NIL);
}

#[test]
fn test_opt_multiple() {
    assert_eq!(
        eval_source("((fn (a &opt b c) (list a b c)) 1)").unwrap(),
        eval_source("(list 1 nil nil)").unwrap()
    );
    assert_eq!(
        eval_source("((fn (a &opt b c) (list a b c)) 1 2)").unwrap(),
        eval_source("(list 1 2 nil)").unwrap()
    );
    assert_eq!(
        eval_source("((fn (a &opt b c) (list a b c)) 1 2 3)").unwrap(),
        eval_source("(list 1 2 3)").unwrap()
    );
}

#[test]
fn test_opt_too_many_args() {
    // More args than required + optional → arity error
    assert!(eval_source("((fn (a &opt b) a) 1 2 3)").is_err());
}

#[test]
fn test_opt_too_few_args() {
    // Fewer args than required → arity error
    assert!(eval_source("((fn (a &opt b c) a))").is_err());
}

#[test]
fn test_opt_with_rest() {
    // &opt before & rest
    assert_eq!(
        eval_source("((fn (a &opt b & rest) (list a b rest)) 1)").unwrap(),
        eval_source("(list 1 nil ())").unwrap()
    );
    assert_eq!(
        eval_source("((fn (a &opt b & rest) (list a b rest)) 1 2)").unwrap(),
        eval_source("(list 1 2 ())").unwrap()
    );
    assert_eq!(
        eval_source("((fn (a &opt b & rest) (list a b rest)) 1 2 3 4)").unwrap(),
        eval_source("(list 1 2 (list 3 4))").unwrap()
    );
}

#[test]
fn test_opt_defn() {
    assert_eq!(
        eval_source("(begin (defn f (a &opt b) (list a b)) (f 1))").unwrap(),
        eval_source("(list 1 nil)").unwrap()
    );
    assert_eq!(
        eval_source("(begin (defn f (a &opt b) (list a b)) (f 1 2))").unwrap(),
        eval_source("(list 1 2)").unwrap()
    );
}

#[test]
fn test_opt_compile_time_arity() {
    // Compile-time arity check for known callees
    assert!(eval_source("(begin (defn f (a &opt b) a) (f))").is_err());
    assert!(eval_source("(begin (defn f (a &opt b) a) (f 1 2 3))").is_err());
}

#[test]
fn test_opt_no_params_after() {
    // &opt must be followed by at least one parameter
    assert!(eval_source("(fn (&opt) 1)").is_err());
}

#[test]
fn test_opt_after_rest_error() {
    // &opt after & is an error
    assert!(eval_source("(fn (a & rest &opt b) 1)").is_err());
}

#[test]
fn test_opt_only() {
    // All params optional, no required
    assert_eq!(
        eval_source("((fn (&opt a b) (list a b)))").unwrap(),
        eval_source("(list nil nil)").unwrap()
    );
    assert_eq!(
        eval_source("((fn (&opt a b) (list a b)) 1)").unwrap(),
        eval_source("(list 1 nil)").unwrap()
    );
    assert_eq!(
        eval_source("((fn (&opt a b) (list a b)) 1 2)").unwrap(),
        eval_source("(list 1 2)").unwrap()
    );
}

// ============================================================
// &keys keyword arguments
// ============================================================

#[test]
fn test_keys_basic() {
    assert_eq!(
        eval_source("((fn (a &keys opts) opts) 1 :x 10 :y 20)").unwrap(),
        eval_source("{:x 10 :y 20}").unwrap()
    );
}

#[test]
fn test_keys_empty() {
    assert_eq!(
        eval_source("((fn (a &keys opts) opts) 1)").unwrap(),
        eval_source("{}").unwrap()
    );
}

#[test]
fn test_keys_destructure() {
    assert_eq!(
        eval_source("((fn (a &keys {:x x :y y}) (+ x y)) 1 :x 10 :y 20)").unwrap(),
        Value::int(30)
    );
}

#[test]
fn test_keys_missing_key_destructure() {
    // Missing key in destructure → nil (silent nil semantics)
    assert_eq!(
        eval_source("((fn (a &keys {:x x :y y}) y) 1 :x 10)").unwrap(),
        Value::NIL
    );
}

#[test]
fn test_keys_with_opt() {
    assert_eq!(
        eval_source("((fn (a &opt b &keys opts) (list a b opts)) 1)").unwrap(),
        eval_source("(list 1 nil {})").unwrap()
    );
    assert_eq!(
        eval_source("((fn (a &opt b &keys opts) (list a b opts)) 1 2)").unwrap(),
        eval_source("(list 1 2 {})").unwrap()
    );
    assert_eq!(
        eval_source("((fn (a &opt b &keys opts) (list a b opts)) 1 2 :x 10)").unwrap(),
        eval_source("(list 1 2 {:x 10})").unwrap()
    );
}

#[test]
fn test_keys_odd_args_error() {
    assert!(eval_source("((fn (a &keys opts) opts) 1 :x 10 :y)").is_err());
}

#[test]
fn test_keys_non_keyword_key_error() {
    assert!(eval_source("((fn (a &keys opts) opts) 1 42 10)").is_err());
}

#[test]
fn test_keys_and_rest_exclusive() {
    assert!(eval_source("(fn (a &keys opts & rest) 1)").is_err());
    assert!(eval_source("(fn (a & rest &keys opts) 1)").is_err());
}

#[test]
fn test_keys_defn() {
    assert_eq!(
        eval_source("(begin (defn f (a &keys opts) opts) (f 1 :host \"db\" :port 3306))").unwrap(),
        eval_source("{:host \"db\" :port 3306}").unwrap()
    );
}

// ============================================================
// &named strict named parameters
// ============================================================

#[test]
fn test_named_basic() {
    assert_eq!(
        eval_source("((fn (a &named host port) (list host port)) 1 :host \"db\" :port 3306)")
            .unwrap(),
        eval_source("(list \"db\" 3306)").unwrap()
    );
}

#[test]
fn test_named_missing_key() {
    // Missing named param → nil (from struct destructuring)
    assert_eq!(
        eval_source("((fn (a &named host port) port) 1 :host \"db\")").unwrap(),
        Value::NIL
    );
}

#[test]
fn test_named_unknown_key_error() {
    // Unknown key → runtime error (strict validation)
    assert!(eval_source("((fn (a &named host) host) 1 :host \"db\" :port 3306)").is_err());
}

#[test]
fn test_named_with_opt() {
    assert_eq!(
        eval_source("((fn (a &opt b &named host) (list a b host)) 1 :host \"db\")").unwrap(),
        eval_source("(list 1 nil \"db\")").unwrap()
    );
}

#[test]
fn test_named_defn() {
    assert_eq!(
        eval_source(
            "(begin (defn connect (host &named port) (list host port)) \
             (connect \"db\" :port 3306))"
        )
        .unwrap(),
        eval_source("(list \"db\" 3306)").unwrap()
    );
}

#[test]
fn test_named_odd_args_error() {
    assert!(eval_source("((fn (a &named host) host) 1 :host)").is_err());
}

#[test]
fn test_named_and_keys_exclusive() {
    assert!(eval_source("(fn (a &keys opts &named host) 1)").is_err());
    assert!(eval_source("(fn (a &named host &keys opts) 1)").is_err());
}

#[test]
fn test_named_no_params_error() {
    assert!(eval_source("(fn (a &named) 1)").is_err());
}

#[test]
fn test_named_non_symbol_error() {
    assert!(eval_source("(fn (a &named [x]) 1)").is_err());
}

// ============================================================
// Edge case tests
// ============================================================

#[test]
fn test_opt_destructuring_pattern() {
    // &opt with destructuring pattern as optional param
    assert_eq!(
        eval_source("((fn (a &opt (b c)) (list a b c)) (list 1 2))").unwrap(),
        eval_source("(list (list 1 2) nil nil)").unwrap()
    );
    assert_eq!(
        eval_source("((fn (a &opt (b c)) (list a b c)) 1 (list 2 3))").unwrap(),
        eval_source("(list 1 2 3)").unwrap()
    );
}

#[test]
fn test_keys_mutable_capture() {
    // &keys param that is set! inside body and captured by nested closure
    assert_eq!(
        eval_source(
            "((fn (&keys opts)
               (let ((f (fn () opts)))
                 (f)))
             :x 10)"
        )
        .unwrap(),
        eval_source("{:x 10}").unwrap()
    );
}

#[test]
fn test_keys_tail_call_error() {
    // Tail call with bad keyword args should produce error, not crash
    assert!(eval_source(
        "(begin
           (defn f (a &keys opts) opts)
           (defn g () (f 1 :x))
           (g))"
    )
    .is_err());
}

#[test]
fn test_opt_fiber_resume() {
    // Coroutine with &opt closure — first resume with no args, second with a value
    // coro/resume passes the value as the result of yield, not the param
    assert_eq!(
        eval_source(
            "(let ((co (coro/new (fn (&opt a) (+ (or a 0) (yield a))))))
               (coro/resume co)
               (coro/resume co 42))"
        )
        .unwrap(),
        Value::int(42)
    );
    // With an initial arg, the param is bound
    assert_eq!(
        eval_source(
            "(let ((co (coro/new (fn (&opt a) (yield a) a))))
               (coro/resume co 10)
               (coro/resume co))"
        )
        .unwrap(),
        Value::int(10)
    );
}

#[test]
fn test_keys_duplicate_keys() {
    // Duplicate keyword keys → runtime error
    assert!(eval_source("((fn (a &keys opts) (get opts :x)) 1 :x 10 :x 20)").is_err());
}
