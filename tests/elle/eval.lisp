# Integration tests for the eval special form
#
# Migrated from tests/integration/eval.rs (46 tests)

(import-file "./examples/assertions.lisp")

# Helper: assert that an expression errors (wraps in try/catch)
(defn assert-err [thunk msg]
  "Assert that (thunk) raises an error"
  (let ([result (try (begin (thunk) :no-error)
                  (catch (e) :got-error))])
    (assert-eq result :got-error msg)))

# Helper: assert that an expression errors and the message contains a substring
(defn assert-err-contains [thunk substring msg]
  "Assert that (thunk) raises an error containing substring"
  (let ([result (try (begin (thunk) nil)
                  (catch (e) e))])
    (assert-true (string? (string/repr result))
                 (string/concat msg " — expected error"))
    (assert-true (string/contains? (string/repr result) substring)
                 (string/concat msg " — expected '" substring "' in error"))))

# ============================================================
# Basic eval
# ============================================================

# test_eval_literal
(assert-eq (eval '42) 42 "eval literal integer")

# test_eval_string_literal
(assert-eq (eval '"hello") "hello" "eval string literal")

# test_eval_boolean
(assert-eq (eval 'true) true "eval true")
(assert-eq (eval 'false) false "eval false")

# test_eval_nil
(assert-eq (eval 'nil) nil "eval nil")

# test_eval_quoted_expression
(assert-eq (eval '(+ 1 2)) 3 "eval quoted addition")

# test_eval_list_construction
(assert-eq (eval (list '+ 1 2)) 3 "eval list construction")

# ============================================================
# Env argument handling
# ============================================================

# test_eval_with_struct_env
(assert-eq (eval '(+ x y) {:x 10 :y 20}) 30 "eval with struct env")

# test_eval_with_mutable_table_env
(assert-eq (eval '(+ x y) @{:x 10 :y 20}) 30 "eval with mutable table env")

# test_eval_with_nil_env
(assert-eq (eval '(+ 3 4) nil) 7 "eval with nil env")

# test_eval_with_empty_table_env
(assert-eq (eval '(+ 1 2) (table)) 3 "eval with empty table env")

# test_eval_env_invalid_type
(assert-err (fn () (eval '42 "bad")) "eval env invalid type (string)")

# test_eval_env_integer_invalid
(assert-err (fn () (eval '42 123)) "eval env invalid type (integer)")

# ============================================================
# Prelude macros in eval'd code
# ============================================================

# test_eval_with_when_macro
(assert-eq (eval '(when true 42)) 42 "eval with when macro")

# test_eval_with_unless_macro
(assert-eq (eval '(unless false 99)) 99 "eval with unless macro")

# test_eval_with_defn_macro
(assert-eq (eval '(begin (defn f (x) (* x x)) (f 5))) 25
           "eval with defn macro")

# test_eval_with_let_star_macro
(assert-eq (eval '(let* ((x 1) (y (+ x 1))) (+ x y))) 3
           "eval with let* macro")

# test_eval_with_thread_first
(assert-eq (eval '(-> 5 (+ 3) (* 2))) 16 "eval with thread-first")

# ============================================================
# Closures and scoping in eval'd code
# ============================================================

# test_eval_with_closure
(assert-eq (eval '(let ((x 1)) ((fn () x)))) 1 "eval with closure")

# test_eval_with_higher_order_function
(assert-eq (eval '(let ((f (fn (x) (+ x 1)))) (f 41))) 42
           "eval with higher-order function")

# ============================================================
# Eval in various contexts
# ============================================================

# test_eval_inside_let
(assert-eq (let ((x 10)) (eval '(+ 1 2))) 3 "eval inside let")

# test_eval_inside_lambda
(assert-eq ((fn () (eval '42))) 42 "eval inside lambda")

# test_eval_result_in_computation
(assert-eq (+ 1 (eval '2)) 3 "eval result in computation")

# test_eval_result_in_let_binding
(assert-eq (let ((x (eval '42))) (+ x 1)) 43 "eval result in let binding")

# test_eval_in_conditional
(assert-eq (if (eval 'true) 1 2) 1 "eval in conditional")

# ============================================================
# Nested eval
# ============================================================

# test_eval_nested
(assert-eq (eval '(eval '42)) 42 "nested eval")

# ============================================================
# Error handling
# ============================================================

# test_eval_compilation_error
(assert-err (fn () (eval '(if))) "eval compilation error (if with no args)")

# test_eval_runtime_error_in_evald_code
(assert-err (fn () (eval '(/ 1 0))) "eval runtime error (division by zero)")

# test_eval_undefined_variable
(assert-err (fn () (eval 'undefined_var)) "eval undefined variable")

# ============================================================
# Sequential evals (expander caching)
# ============================================================

# test_eval_sequential
(assert-eq (begin (eval '(+ 1 2)) (eval '(* 3 4))) 12
           "sequential evals")

# ============================================================
# Eval with begin/block
# ============================================================

# test_eval_begin_sequence
(assert-eq (eval '(begin 1 2 3)) 3 "eval begin sequence")

# ============================================================
# Eval with match
# ============================================================

# test_eval_with_match — bind match result to var first (known bug workaround)
(var match-result (eval '(match 42 (42 "found") (_ "not found"))))
(assert-eq match-result "found" "eval with match")

# ============================================================
# Eval with list operations
# ============================================================

# test_eval_list_operations
(assert-eq (eval '(first (list 1 2 3))) 1 "eval list operations (first)")

# test_eval_returns_list
(var eval-list (eval '(list 1 2 3)))
(assert-eq (first eval-list) 1 "eval returns list (first element)")

# ============================================================
# read + eval pattern (REPL pattern)
# ============================================================

# test_read_eval_pattern
(assert-eq (eval (read "(+ 1 2)")) 3 "read-eval pattern")

# test_read_eval_literal
(assert-eq (eval (read "42")) 42 "read-eval literal")

# ============================================================
# Eval with cond
# ============================================================

# test_eval_with_cond
(assert-eq (eval '(cond ((= 1 2) "no") (true "yes"))) "yes"
           "eval with cond")

# ============================================================
# Eval with while loop
# ============================================================

# test_eval_with_while
(assert-eq (eval '(begin (var i 0) (while (< i 3) (set i (+ i 1))) i)) 3
           "eval with while loop")

# ============================================================
# Eval with recursion
# ============================================================

# test_eval_with_recursion
(assert-eq (eval '(begin (defn fact (n) (if (= n 0) 1 (* n (fact (- n 1))))) (fact 5)))
           120 "eval with recursion (factorial)")

# ============================================================
# Eval with array operations
# ============================================================

# test_eval_with_array
(assert-eq (eval '(get @[10 20 30] 1)) 20 "eval with array get")

# ============================================================
# Eval with string operations
# ============================================================

# test_eval_with_string_ops
(assert-eq (eval '(length "hello")) 5 "eval with string length")

# ============================================================
# Eval with multiple env bindings
# ============================================================

# test_eval_env_many_bindings
(assert-eq (eval '(+ a (+ b (+ c d))) {:a 1 :b 2 :c 3 :d 4}) 10
           "eval env many bindings")

# ============================================================
# Eval with env binding shadowing primitives
# ============================================================

# test_eval_env_shadows_nothing
(assert-eq (eval '(+ x 1) {:x 41}) 42
           "eval env alongside primitives")

# ============================================================
# Eval returns keyword
# ============================================================

# test_eval_returns_keyword
(assert-eq (eval ':hello) :hello "eval returns keyword")

# ============================================================
# Eval with try/catch in eval'd code
# ============================================================

# test_eval_with_try_catch
(assert-eq (eval '(try (/ 1 0) (catch (e) 42))) 42
           "eval with try/catch")

# ============================================================
# import (prelude function)
# ============================================================

# test_import_returns_last_value
# Write a temp file, import it, check the returned struct
(var import-test-path "/run/user/1000/elle-test-import.lisp")
(spit import-test-path "(def internal 42)\n{:answer internal :double (* internal 2)}")
(var import-result (import-file import-test-path))
(assert-eq (get import-result :answer) 42 "import returns last value (:answer)")
(assert-eq (get import-result :double) 84 "import returns last value (:double)")

# test_import_destructuring
(var import-destr-path "/run/user/1000/elle-test-import-destr.lisp")
(spit import-destr-path "(def internal 42)\n{:answer internal :double (* internal 2)}")
(let (({:answer a} (import-file import-destr-path)))
  (assert-eq a 42 "import destructuring"))
