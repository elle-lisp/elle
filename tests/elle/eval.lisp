# Integration tests for the eval special form
#
# Migrated from tests/integration/eval.rs (46 tests)


# Helper: assert that an expression errors (uses protect to capture VM-level signals)
(defn assert-err [thunk msg]
  "Assert that (thunk) signals an error"
  (let (([ok? _] (protect (thunk))))
    (assert (not ok?) msg)))

# Helper: assert that an expression errors and the message contains a substring
(defn assert-err-contains [thunk substring msg]
  "Assert that (thunk) signals an error containing substring"
  (let (([ok? result] (protect (thunk))))
    (assert (not ok?) (append msg " — expected error"))
    (assert (string/contains? (string result) substring) (-> msg (append " — expected '") (append substring) (append "' in error")))))

# ============================================================
# Basic eval
# ============================================================

# test_eval_literal
(assert (= (eval '42) 42) "eval literal integer")

# test_eval_string_literal
(assert (= (eval '"hello") "hello") "eval string literal")

# test_eval_boolean
(assert (= (eval 'true) true) "eval true")
(assert (= (eval 'false) false) "eval false")

# test_eval_nil
(assert (= (eval 'nil) nil) "eval nil")

# test_eval_quoted_expression
(assert (= (eval '(+ 1 2)) 3) "eval quoted addition")

# test_eval_list_construction
(assert (= (eval (list '+ 1 2)) 3) "eval list construction")

# ============================================================
# Env argument handling (REMOVED)
# ============================================================
# Environment argument support was intentionally removed from eval.
# Tests that relied on (eval expr env) have been removed.
# Lexical scoping via closures is the recommended pattern.

# ============================================================
# Prelude macros in eval'd code
# ============================================================

# test_eval_with_when_macro
(assert (= (eval '(when true 42)) 42) "eval with when macro")

# test_eval_with_unless_macro
(assert (= (eval '(unless false 99)) 99) "eval with unless macro")

# test_eval_with_defn_macro
(assert (= (eval '(begin (defn f (x) (* x x)) (f 5))) 25) "eval with defn macro")

# test_eval_with_let_star_macro
(assert (= (eval '(let* ((x 1) (y (+ x 1))) (+ x y))) 3) "eval with let* macro")

# test_eval_with_thread_first
(assert (= (eval '(-> 5 (+ 3) (* 2))) 16) "eval with thread-first")

# ============================================================
# Closures and scoping in eval'd code
# ============================================================

# test_eval_with_closure
(assert (= (eval '(let ((x 1)) ((fn () x)))) 1) "eval with closure")

# test_eval_with_higher_order_function
(assert (= (eval '(let ((f (fn (x) (+ x 1)))) (f 41))) 42) "eval with higher-order function")

# ============================================================
# Eval in various contexts
# ============================================================

# test_eval_inside_let
(assert (= (let ((x 10)) (eval '(+ 1 2))) 3) "eval inside let")

# test_eval_inside_lambda
(assert (= ((fn () (eval '42))) 42) "eval inside lambda")

# test_eval_result_in_computation
(assert (= (+ 1 (eval '2)) 3) "eval result in computation")

# test_eval_result_in_let_binding
(assert (= (let ((x (eval '42))) (+ x 1)) 43) "eval result in let binding")

# test_eval_in_conditional
(assert (= (if (eval 'true) 1 2) 1) "eval in conditional")

# ============================================================
# Nested eval
# ============================================================

# test_eval_nested
(assert (= (eval '(eval '42)) 42) "nested eval")

# ============================================================
# Error handling
# ============================================================

# test_eval_compilation_error
(let (([ok? _] (protect ((fn () (eval '(if))))))) (assert (not ok?) "eval compilation error (if with no args)"))

# test_eval_runtime_error_in_evald_code
(let (([ok? _] (protect ((fn () (eval '(/ 1 0))))))) (assert (not ok?) "eval runtime error (division by zero)"))

# test_eval_undefined_variable
(let (([ok? _] (protect ((fn () (eval 'undefined_var)))))) (assert (not ok?) "eval undefined variable"))

# ============================================================
# Sequential evals (expander caching)
# ============================================================

# test_eval_sequential
(assert (= (begin (eval '(+ 1 2)) (eval '(* 3 4))) 12) "sequential evals")

# ============================================================
# Eval with begin/block
# ============================================================

# test_eval_begin_sequence
(assert (= (eval '(begin 1 2 3)) 3) "eval begin sequence")

# ============================================================
# Eval with match
# ============================================================

# test_eval_with_match — bind match result to var first (known bug workaround)
(var match-result (eval '(match 42 (42 "found") (_ "not found"))))
(assert (= match-result "found") "eval with match")

# ============================================================
# Eval with list operations
# ============================================================

# test_eval_list_operations
(assert (= (eval '(first (list 1 2 3))) 1) "eval list operations (first)")

# test_eval_returns_list
(var eval-list (eval '(list 1 2 3)))
(assert (= (first eval-list) 1) "eval returns list (first element)")

# ============================================================
# read + eval pattern (REPL pattern)
# ============================================================

# test_read_eval_pattern
(assert (= (eval (read "(+ 1 2)")) 3) "read-eval pattern")

# test_read_eval_literal
(assert (= (eval (read "42")) 42) "read-eval literal")

# ============================================================
# Eval with cond
# ============================================================

# test_eval_with_cond
(assert (= (eval '(cond ((= 1 2) "no") (true "yes"))) "yes") "eval with cond")

# ============================================================
# Eval with while loop
# ============================================================

# test_eval_with_while
(assert (= (eval '(begin (var i 0) (while (< i 3) (assign i (+ i 1))) i)) 3) "eval with while loop")

# ============================================================
# Eval with recursion
# ============================================================

# test_eval_with_recursion
(assert (= (eval '(begin (defn fact (n) (if (= n 0) 1 (* n (fact (- n 1))))) (fact 5))) 120) "eval with recursion (factorial)")

# ============================================================
# Eval with array operations
# ============================================================

# test_eval_with_array
(assert (= (eval '(get @[10 20 30] 1)) 20) "eval with array get")

# ============================================================
# Eval with string operations
# ============================================================

# test_eval_with_string_ops
(assert (= (eval '(length "hello")) 5) "eval with string length")

# ============================================================
# Eval with multiple env bindings (REMOVED)
# ============================================================
# This test relied on environment argument support, which was removed.

# ============================================================
# Eval with env binding shadowing primitives (REMOVED)
# ============================================================
# This test relied on environment argument support, which was removed.

# ============================================================
# Eval returns keyword
# ============================================================

# test_eval_returns_keyword
(assert (= (eval ':hello) :hello) "eval returns keyword")

# ============================================================
# Eval with try/catch in eval'd code
# ============================================================

# test_eval_with_try_catch
(assert (= (eval '(try (/ 1 0) (catch e 42))) 42) "eval with try/catch")

# ============================================================
# import (prelude function)
# ============================================================

# test_import_returns_last_value
# Write a temp file, import it, check the returned struct
(var import-test-path "/tmp/elle-test-import.lisp")
(spit import-test-path "(def internal 42)\n{:answer internal :double (* internal 2)}")
(var import-result (import-file import-test-path))
(assert (= (get import-result :answer) 42) "import returns last value (:answer)")
(assert (= (get import-result :double) 84) "import returns last value (:double)")

# test_import_destructuring
(var import-destr-path "/tmp/elle-test-import-destr.lisp")
(spit import-destr-path "(def internal 42)\n{:answer internal :double (* internal 2)}")
(let (({:answer a} (import-file import-destr-path)))
  (assert (= a 42) "import destructuring"))
