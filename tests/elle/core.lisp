# Core integration tests
#
# Migrated from tests/integration/core.rs
# Tests that require float precision checks (.as_float() with tolerance)
# or error message substring matching remain in the Rust file.

(import-file "./examples/assertions.lisp")

# Helper: assert that a thunk raises an error
(defn assert-err [thunk msg]
  "Assert that (thunk) raises an error"
  (let ([result (try (begin (thunk) :no-error)
                  (catch (e) :got-error))])
    (assert-eq result :got-error msg)))

# ============================================================================
# Basic arithmetic
# ============================================================================

# test_simple_arithmetic
(assert-eq (+ 1 2) 3 "simple add")
(assert-eq (- 10 3) 7 "simple sub")
(assert-eq (* 4 5) 20 "simple mul")
(assert-eq (/ 20 4) 5 "simple div")

# test_nested_arithmetic
(assert-eq (+ (* 2 3) (- 10 5)) 11 "nested arithmetic 1")
(assert-eq (* (+ 1 2) (- 5 2)) 9 "nested arithmetic 2")

# test_deeply_nested
(assert-eq (+ 1 (+ 2 (+ 3 (+ 4 5)))) 15 "deeply nested addition")

# ============================================================================
# Comparisons
# ============================================================================

# test_comparisons
(assert-eq (= 5 5) true "5 = 5")
(assert-eq (= 5 6) false "5 = 6")
(assert-eq (< 3 5) true "3 < 5")
(assert-eq (< 5 3) false "5 < 3")
(assert-eq (> 7 5) true "7 > 5")

# ============================================================================
# Conditionals
# ============================================================================

# test_if_true
(assert-eq (if true 100 200) 100 "if true")

# test_if_false
(assert-eq (if false 100 200) 200 "if false")

# test_if_with_condition
(assert-eq (if (> 5 3) 100 200) 100 "if with true condition")
(assert-eq (if (< 5 3) 100 200) 200 "if with false condition")

# test_nested_if
(assert-eq (if (> 5 3) (if (< 2 4) 1 2) 3) 1 "nested if")

# test_if_nil_else
(assert-eq (if false 100) nil "if without else returns nil")

# ============================================================================
# Lists
# ============================================================================

# test_list_construction
(assert-true (list? (list 1 2 3)) "list construction is list")
(assert-eq (length (list 1 2 3)) 3 "list construction length")

# test_cons
(var cons-list (cons 1 (cons 2 (cons 3 nil))))
(assert-true (list? cons-list) "cons builds list")
(assert-eq (first cons-list) 1 "cons first")
(assert-eq (first (rest cons-list)) 2 "cons second")
(assert-eq (first (rest (rest cons-list))) 3 "cons third")

# test_first_rest
(assert-eq (first (list 10 20 30)) 10 "first of list")
(assert-eq (first (rest (list 10 20 30))) 20 "second of list")
(assert-eq (first (rest (rest (list 10 20 30)))) 30 "third of list")

# test_nested_lists
(var nested (list (list 1 2) (list 3 4)))
(assert-eq (length nested) 2 "nested lists length")
(assert-true (list? (first nested)) "nested list first is list")
(assert-true (list? (first (rest nested))) "nested list second is list")

# ============================================================================
# Quote
# ============================================================================

# test_quote_symbol
(assert-true (symbol? 'foo) "quoted symbol is symbol")

# test_quote_list
(assert-true (list? '(1 2 3)) "quoted list is list")

# ============================================================================
# Type predicates
# ============================================================================

# test_predicates
(assert-eq (nil? nil) true "nil? nil")
(assert-eq (nil? 0) false "nil? 0")
(assert-eq (number? 42) true "number? 42")
(assert-eq (number? nil) false "number? nil")
(assert-eq (pair? (cons 1 2)) true "pair? cons")
(assert-eq (pair? nil) false "pair? nil")

# ============================================================================
# Global definitions
# ============================================================================

# test_define_and_use
(var x-def 42)
(assert-eq (+ x-def 10) 52 "define and use")

# test_multiple_defines
(var a-def 10)
(var b-def 20)
(var c-def 30)
(assert-eq (+ a-def b-def c-def) 60 "multiple defines")

# ============================================================================
# Begin
# ============================================================================

# test_begin
(assert-eq (begin 1 2 3) 3 "begin returns last")

# test_begin_with_side_effects
(assert-eq (begin (var x-begin 10) (var y-begin 20) (+ x-begin y-begin)) 30
           "begin with side effects")

# ============================================================================
# Complex expressions
# ============================================================================

# test_factorial_logic
(assert-eq (if (<= 1 1) 1 (* 1 1)) 1 "factorial base case")
(assert-eq (if (<= 5 1) 1 (* 5 120)) 600 "factorial recursive case")

# test_max_logic
(assert-eq (if (> 10 5) 10 5) 10 "max 10 5")
(assert-eq (if (> 3 7) 3 7) 7 "max 3 7")

# ============================================================================
# Error cases
# ============================================================================

# test_division_by_zero
(assert-err (fn [] (/ 10 0)) "division by zero errors")

# test_type_error
(assert-err (fn [] (+ 1 nil)) "type error on + 1 nil")

# test_undefined_variable
(assert-err (fn [] undefined-var) "undefined variable errors")

# test_arity_error
# + accepts 0 args (identity)
(assert-eq (+) 0 "plus zero args")
# first requires 1 arg — compile-time error, so use eval to catch it
(assert-err (fn [] (eval '(first))) "first with no args errors")

# ============================================================================
# Stress tests
# ============================================================================

# test_large_list — create list with 100 elements
(var large-list (list 0 1 2 3 4 5 6 7 8 9
                      10 11 12 13 14 15 16 17 18 19
                      20 21 22 23 24 25 26 27 28 29
                      30 31 32 33 34 35 36 37 38 39
                      40 41 42 43 44 45 46 47 48 49
                      50 51 52 53 54 55 56 57 58 59
                      60 61 62 63 64 65 66 67 68 69
                      70 71 72 73 74 75 76 77 78 79
                      80 81 82 83 84 85 86 87 88 89
                      90 91 92 93 94 95 96 97 98 99))
(assert-true (list? large-list) "large list is list")
(assert-eq (length large-list) 100 "large list length")

# test_many_operations
(assert-eq (+ 1 2 3 4 5 6 7 8 9 10) 55 "sum 1..10")
(assert-eq (* 1 2 3 4 5) 120 "product 1..5")

# ============================================================================
# Logic combinations
# ============================================================================

# test_not
(assert-eq (not true) false "not true")
(assert-eq (not false) true "not false")
(assert-eq (not nil) true "not nil (falsy)")
(assert-eq (not ()) false "not empty list (truthy)")
(assert-eq (not 0) false "not 0 (truthy)")

# test_complex_conditionals
(assert-eq (if (not (< 5 3)) 100 200) 100 "complex conditional not")
(assert-true (symbol? (if (= (+ 2 3) 5) 'yes 'no)) "conditional returns symbol")

# ============================================================================
# Standard library: length, append, reverse
# ============================================================================

# test_length
(assert-eq (length (list 1 2 3 4 5)) 5 "length of 5-element list")
(assert-eq (length nil) 0 "length of nil")

# test_append
(assert-list-eq (append (append (list 1 2) (list 3 4)) (list 5))
                (list 1 2 3 4 5)
                "append lists")

# test_reverse
(assert-list-eq (reverse (list 1 2 3)) (list 3 2 1) "reverse list")

# ============================================================================
# Math: min, max (integer parts only)
# ============================================================================

# test_min_max (integer parts)
(assert-eq (min 5 3 7 2) 2 "min integers")
(assert-eq (max 5 3 7 2) 7 "max integers")

# ============================================================================
# Math: abs (integer parts only)
# ============================================================================

# test_abs (integer parts)
(assert-eq (abs -5) 5 "abs negative")
(assert-eq (abs 5) 5 "abs positive")

# ============================================================================
# Math: type conversions (integer part only)
# ============================================================================

# test_type_conversions (integer part)
(assert-eq (int 3.14) 3 "int truncates float")

# ============================================================================
# String operations
# ============================================================================

# test_string_length
(assert-eq (length "hello") 5 "string length hello")
(assert-eq (length "") 0 "string length empty")

# test_string_append
(assert-string-eq (append (append "hello" " ") "world") "hello world"
                  "string append")

# test_string_case
(assert-string-eq (string-upcase "hello") "HELLO" "string upcase")
(assert-string-eq (string-downcase "WORLD") "world" "string downcase")

# ============================================================================
# Indexed access
# ============================================================================

# test_get_indexed
(assert-eq (get [10 20 30] 0) 10 "get tuple index 0")
(assert-eq (get [10 20 30] 1) 20 "get tuple index 1")
(assert-eq (get [10 20 30] 2) 30 "get tuple index 2")

# ============================================================================
# List utilities
# ============================================================================

# test_last
(assert-eq (last (list 1 2 3 4 5)) 5 "last of list")

# test_take_drop
(assert-list-eq (take 2 (list 1 2 3 4 5)) (list 1 2) "take 2")
(assert-list-eq (drop 2 (list 1 2 3 4 5)) (list 3 4 5) "drop 2")

# ============================================================================
# type-of
# ============================================================================

# test_type
(assert-true (keyword? (type-of 42)) "type-of int is keyword")
(assert-true (keyword? (type-of 3.14)) "type-of float is keyword")
(assert-true (keyword? (type-of "hello")) "type-of string is keyword")

# test_type_of_list_consistency (Issue #308)
(var type-empty (type-of ()))
(var type-proper (type-of (list 1 2)))
(var type-cons (type-of (cons 1 2)))
(assert-true (keyword? type-empty) "type-of empty list is keyword")
(assert-true (keyword? type-proper) "type-of proper list is keyword")
(assert-true (keyword? type-cons) "type-of cons cell is keyword")
(assert-eq type-empty type-proper "empty list and proper list same type")
(assert-eq type-proper type-cons "proper list and cons cell same type")
(assert-eq (type-of ()) :list "type-of () is :list")

# ============================================================================
# Math: floor, ceil, round
# ============================================================================

# test_floor_ceil_round
(assert-eq (floor 3) 3 "floor int")
(assert-eq (floor 3.7) 3 "floor float")
(assert-eq (ceil 3) 3 "ceil int")
(assert-eq (ceil 3.2) 4 "ceil float")
(assert-eq (round 3) 3 "round int")
(assert-eq (round 3.4) 3 "round down")
(assert-eq (round 3.6) 4 "round up")

# ============================================================================
# String functions
# ============================================================================

# test_substring
(assert-string-eq (substring "hello" 1 4) "ell" "substring middle")
(assert-string-eq (substring "hello" 2) "llo" "substring to end")
(assert-string-eq (substring "hello" 0 2) "he" "substring from start")

# test_string_index
(assert-eq (string-index "hello" "l") 2 "string-index found")
(assert-eq (string-index "hello" "x") nil "string-index not found")

# test_char_at
(assert-string-eq (char-at "hello" 0) "h" "char-at 0")
(assert-string-eq (char-at "hello" 1) "e" "char-at 1")
(assert-string-eq (char-at "hello" 4) "o" "char-at 4")

# ============================================================================
# Array operations
# ============================================================================

# test_array_creation
(var arr (array 1 2 3))
(assert-eq (length arr) 3 "array length 3")
(assert-eq (get arr 0) 1 "array get 0")
(assert-eq (get arr 1) 2 "array get 1")
(assert-eq (get arr 2) 3 "array get 2")

# empty array
(assert-eq (length (array)) 0 "empty array length")

# test_array_length
(assert-eq (length (array 1 2 3)) 3 "array length")
(assert-eq (length (array)) 0 "array length empty")
(assert-eq (length (array 10 20 30 40 50)) 5 "array length 5")

# test_array_get
(assert-eq (get (array 10 20 30) 0) 10 "array get 0")
(assert-eq (get (array 10 20 30) 1) 20 "array get 1")
(assert-eq (get (array 10 20 30) 2) 30 "array get 2")

# test_array_put
(var arr-put (put (array 1 2 3) 1 99))
(assert-eq (get arr-put 0) 1 "array put keeps 0")
(assert-eq (get arr-put 1) 99 "array put sets 1")
(assert-eq (get arr-put 2) 3 "array put keeps 2")

# put at beginning
(assert-eq (get (put (array 1 2 3) 0 100) 0) 100 "array put at beginning")

# put at end
(assert-eq (get (put (array 1 2 3) 2 200) 2) 200 "array put at end")

# ============================================================================
# Math: mod, rem, even?, odd?
# ============================================================================

# test_mod_and_remainder
(assert-eq (mod 17 5) 2 "mod 17 5")
(assert-eq (mod 20 4) 0 "mod 20 4")
(assert-eq (mod -17 5) 3 "mod -17 5")
(assert-eq (rem 17 5) 2 "rem 17 5")
(assert-eq (rem 20 4) 0 "rem 20 4")

# test_even_odd
(assert-true (even? 2) "even? 2")
(assert-false (even? 3) "even? 3")
(assert-false (odd? 2) "odd? 2")
(assert-true (odd? 3) "odd? 3")
(assert-true (even? 0) "even? 0")

# ============================================================================
# Recursive functions
# ============================================================================

# test_recursive_lambda_fibonacci
(defn fib [n]
  (if (< n 2) n (+ (fib (- n 1)) (fib (- n 2)))))
(assert-eq (fib 5) 5 "fibonacci 5")

# test_recursive_lambda_fibonacci_10
(assert-eq (fib 10) 55 "fibonacci 10")

# test_tail_recursive_sum
(defn sum-to [n acc]
  (if (= n 0) acc (sum-to (- n 1) (+ acc n))))
(assert-eq (sum-to 100 0) 5050 "tail recursive sum 1..100")

# test_recursive_countdown
(defn countdown [n]
  (if (<= n 0) 0 (+ n (countdown (- n 1)))))
(assert-eq (countdown 5) 15 "recursive countdown")

# test_nested_recursive_functions
(defn outer [n]
  (defn inner [x]
    (if (< x 1) 0 (+ x (inner (- x 1)))))
  (inner n))
(assert-eq (outer 5) 15 "nested recursive functions")

# ============================================================================
# Lambda basics
# ============================================================================

# test_simple_lambda_call
(defn identity-fn [x] x)
(assert-eq (identity-fn 42) 42 "identity lambda")

# test_lambda_with_arithmetic
(defn double [x] (* x 2))
(assert-eq (double 21) 42 "double lambda")

# test_lambda_with_comparison
(defn is-positive [x] (> x 0))
(assert-true (is-positive 5) "is-positive lambda")

# ============================================================================
# Closure scoping
# ============================================================================

# test_closure_captures_outer_variable
(var x-outer 100)
(assert-eq ((fn [y] (+ x-outer y)) 20) 120 "closure captures outer")

# test_closure_parameter_shadowing
(var x-shadow 100)
(assert-eq ((fn [x-shadow] (+ x-shadow 1)) 50) 51 "closure param shadowing")

# test_closure_captures_multiple_variables
(var cx 10)
(var cy 20)
(var cz 30)
(assert-eq ((fn [a b c] (+ a b c cx cy cz)) 1 2 3) 66
           "closure captures multiple")

# test_closure_parameter_in_nested_expression
(assert-eq ((fn [x] (if (> x 50) (* x 2) (+ x 100))) 25) 125
           "closure param in nested expr")

# test_multiple_closures_independent_params
(def f1 (fn [x] (+ x 10)))
(def f2 (fn [x] (* x 2)))
(assert-eq (+ (f1 5) (f2 5)) 25 "multiple closures independent")

# test_closure_captured_function_call
(def add-fn (fn [a b] (+ a b)))
(assert-eq ((fn [x y] (add-fn x y)) 10 20) 30 "closure captured function")

# test_closure_with_list_operations
(var numbers (list 1 2 3 4 5))
(assert-eq ((fn [lst] (first lst)) numbers) 1 "closure with list ops")

# test_closure_parameter_in_conditional
(assert-string-eq ((fn [n] (if (nil? n) "empty" "nonempty")) (list 1))
                  "nonempty"
                  "closure param in conditional")

# test_closure_preserves_parameter_type
(assert-true ((fn [s] (string? s)) "hello") "closure preserves param type")

# ============================================================================
# Let bindings
# ============================================================================

# test_let_simple_binding
(assert-eq (let ([x 5]) x) 5 "let simple binding")

# test_let_with_arithmetic
(assert-eq (let ([x 5]) (+ x 3)) 8 "let with arithmetic")

# test_let_multiple_bindings
(assert-eq (let ([x 5] [y 3]) (+ x y)) 8 "let multiple bindings")

# test_let_binding_with_expressions
(assert-eq (let ([x (+ 2 3)] [y (* 4 5)]) (+ x y)) 25
           "let binding with expressions")

# test_let_shadowing_global
(var x-let-shadow 10)
(assert-eq (let ([x-let-shadow 20]) x-let-shadow) 20 "let shadows global")

# test_let_does_not_modify_global
(var x-let-global 10)
(let ([x-let-global 20]) x-let-global)
(assert-eq x-let-global 10 "let does not modify global")

# test_let_with_lists
(assert-eq (let ([lst (list 1 2 3)]) (first lst)) 1 "let with lists")

# test_let_with_string_operations
(assert-true (let ([s "hello"]) (string? s)) "let with string ops")

# test_let_with_conditional
(assert-string-eq (let ([x 10]) (if (> x 5) "big" "small"))
                  "big"
                  "let with conditional")

# test_let_empty_body_returns_nil
(assert-eq (let ([x 5])) nil "let empty body returns nil")

# test_let_multiple_body_expressions
(assert-eq (let ([x 5]) (+ x 1) (+ x 2) (+ x 3)) 8
           "let multiple body returns last")

# test_let_with_global_reference
(var y-let-ref 100)
(assert-eq (let ([x 50]) (+ x y-let-ref)) 150 "let with global reference")

# test_let_binding_order
(assert-eq (let ([x 1] [y 2] [z 3]) (+ x y z)) 6 "let binding order")

# test_let_with_list_literal
(assert-list-eq (let ([x '(1 2 3)]) (rest x)) (list 2 3)
                "let with quoted list")

# test_let_shadowing_with_calculation
(var x-let-calc 10)
(assert-eq (let ([x-let-calc (* 2 x-let-calc)]) x-let-calc) 20
           "let shadowing with calculation")

# test_let_with_builtin_functions
(assert-eq (let ([len (fn [x] 42)]) (len nil)) 42
           "let with builtin function override")

# ============================================================================
# let* (sequential bindings)
# ============================================================================

# test_let_star_empty
(assert-eq (let* () 42) 42 "let* empty bindings")

# test_let_star_simple_binding
(assert-eq (let* ([x 5]) x) 5 "let* simple binding")

# test_let_star_with_multiple_bindings_no_dependencies
(assert-eq (let* ([x 1] [y 2]) (+ x y)) 3 "let* multiple bindings")

# ============================================================================
# cond
# ============================================================================

# NOTE: cond is a macro that triggers the match-in-closure bug when used
# directly inside assert-eq. Bind results to a var first.

# test_cond_single_true_clause
(var cond-r1 (cond (true 42)))
(assert-eq cond-r1 42 "cond single true")

# test_cond_single_false_clause_with_else
(var cond-r2 (cond (false 42) (else 100)))
(assert-eq cond-r2 100 "cond false with else")

# test_cond_single_false_clause_without_else
(var cond-r3 (cond (false 42)))
(assert-eq cond-r3 nil "cond false without else")

# test_cond_first_clause_matches
(var cond-r4 (cond ((> 5 3) 100) ((> 4 2) 200)))
(assert-eq cond-r4 100 "cond first matches")

# test_cond_second_clause_matches
(var cond-r5 (cond ((> 3 5) 100) ((> 4 2) 200)))
(assert-eq cond-r5 200 "cond second matches")

# test_cond_multiple_clauses_with_else
(var cond-r6 (cond ((> 3 5) 100) ((> 2 4) 200) (else 300)))
(assert-eq cond-r6 300 "cond multiple with else")

# test_cond_with_expressions_as_conditions
(var cond-r7 (cond
  ((= 1 2) "one-two")
  ((= 2 2) "two-two")
  (else "other")))
(assert-string-eq cond-r7 "two-two" "cond with expression conditions")

# test_cond_with_complex_bodies
(var cond-r8 (cond (false (+ 1 1)) (true (+ 2 3)) (else (+ 4 5))))
(assert-eq cond-r8 5 "cond complex bodies")

# test_cond_with_multiple_body_expressions
(var cond-r9 (cond (true (+ 1 1) (+ 2 2) (+ 3 3))))
(assert-eq cond-r9 6 "cond multiple body exprs")

# test_cond_nested
(var cond-r10 (cond (true (cond (true 42) (else 100))) (else 200)))
(assert-eq cond-r10 42 "cond nested")

# test_cond_with_variable_references
(var x-cond 10)
(var cond-r11 (cond
  ((< x-cond 5) "small")
  ((< x-cond 15) "medium")
  (else "large")))
(assert-string-eq cond-r11 "medium" "cond with variable references")

# test_cond_respects_clause_order
(var cond-r12 (cond
  ((>= 10 5) "first")
  ((>= 10 3) "second")
  (else "third")))
(assert-string-eq cond-r12 "first" "cond respects clause order")

# test_cond_with_else_body_multiple_expressions
(var cond-r13 (cond (false 100) (else (+ 1 1) (+ 2 2) (* 3 3))))
(assert-eq cond-r13 9 "cond else multiple body exprs")

# ============================================================================
# Nested lambdas with closure capture
# ============================================================================

# test_nested_lambda_single_capture
(def make-const (fn [x] (fn [y] x)))
(var f-const (make-const 42))
(assert-eq (f-const 100) 42 "nested lambda single capture")

# test_nested_lambda_parameter_only
(def make-id (fn [x] (fn [y] y)))
(var f-id (make-id 100))
(assert-eq (f-id 42) 42 "nested lambda parameter only")

# ============================================================================
# Threading operators
# ============================================================================

# test_thread_first_simple
(assert-eq (-> 5 (+ 10) (* 2)) 30 "thread-first simple")

# test_thread_first_with_multiple_args
(assert-eq (-> 5 (+ 10 2) (* 3)) 51 "thread-first multiple args")

# test_thread_last_simple
(assert-eq (->> 5 (+ 10) (* 2)) 30 "thread-last simple")

# test_thread_last_with_multiple_args
(assert-eq (->> 2 (+ 10) (* 3)) 36 "thread-last multiple args")

# test_thread_first_chain
(assert-eq (-> 1 (+ 1) (+ 1) (+ 1)) 4 "thread-first chain")

# test_thread_last_chain
(assert-eq (->> 1 (+ 1) (+ 1) (+ 1)) 4 "thread-last chain")

# test_thread_first_with_list_ops
(assert-eq (-> (list 1 2 3) (length)) 3 "thread-first list ops")

# test_thread_last_with_list_ops
(assert-eq (->> (list 1 2 3) (length)) 3 "thread-last list ops")

# test_thread_first_nested
(assert-eq (-> 10 (- 3) (+ 5)) 12 "thread-first nested")

# test_thread_last_nested
(assert-eq (->> 10 (- 3) (+ 5)) -2 "thread-last nested")

# ============================================================================
# Closure with local define and param arithmetic
# ============================================================================

# test_closure_with_local_define_and_param_arithmetic
(let ([outer-fn (fn [x]
                  (begin
                    (var local (* x 2))
                    (fn [y] (+ local y))))])
  (assert-eq ((outer-fn 1) 1) 3 "closure with local define and param"))

# ============================================================================
# Bug fix: let inside lambda with append
# ============================================================================

# test_let_inside_lambda_with_append
(defn f-append [x]
  (if (= x 0) (list) (let ([y x]) (append (list y) (f-append (- x 1))))))
(assert-list-eq (f-append 3) (list 3 2 1) "let inside lambda with append")

# test_let_inside_lambda_values_correct
(defn f-let-val [x] (let ([y x]) y))
(assert-eq (f-let-val 42) 42 "let inside lambda values correct")

# test_multiple_let_bindings_in_lambda
(defn f-multi-let [x] (let ([y x] [z (+ x 1)]) (+ y z)))
(assert-eq (f-multi-let 10) 21 "multiple let bindings in lambda")

# ============================================================================
# Bug fix: defn
# ============================================================================

# test_define_shorthand
(defn f-short [x] (+ x 1))
(assert-eq (f-short 42) 43 "defn shorthand")

# test_define_shorthand_multiple_params
(defn add-short [a b] (+ a b))
(assert-eq (add-short 3 4) 7 "defn multiple params")

# test_define_shorthand_with_body
(defn fact [n] (if (= n 0) 1 (* n (fact (- n 1)))))
(assert-eq (fact 5) 120 "defn factorial")

# ============================================================================
# Bug fix: List display (no dot)
# ============================================================================

# test_list_display_no_dot
(assert-string-eq (string (list 1 2 3)) "(1 2 3)" "list display no dot")

# test_single_element_list_display
(assert-string-eq (string (list 1)) "(1)" "single element list display")

# test_empty_list_display
(assert-string-eq (string (list)) "()" "empty list display")

# NOTE: halt tests stay in Rust — halt terminates the entire program,
# so it cannot be tested in a script that runs multiple assertions.
