(elle/epoch 9)
# Core integration tests
#
# Migrated from tests/integration/core.rs
# Tests that require float precision checks (.as_float() with tolerance)
# or error message substring matching remain in the Rust file.


# ============================================================================
# Basic arithmetic
# ============================================================================

# test_simple_arithmetic
(assert (= (+ 1 2) 3) "simple add")
(assert (= (- 10 3) 7) "simple sub")
(assert (= (* 4 5) 20) "simple mul")
(assert (= (/ 20 4) 5) "simple div")

# test_nested_arithmetic
(assert (= (+ (* 2 3) (- 10 5)) 11) "nested arithmetic 1")
(assert (= (* (+ 1 2) (- 5 2)) 9) "nested arithmetic 2")

# test_deeply_nested
(assert (= (+ 1 (+ 2 (+ 3 (+ 4 5)))) 15) "deeply nested addition")

# ============================================================================
# Comparisons
# ============================================================================

# test_comparisons
(assert (= (= 5 5) true) "5 = 5")
(assert (= (= 5 6) false) "5 = 6")
(assert (= (< 3 5) true) "3 < 5")
(assert (= (< 5 3) false) "5 < 3")
(assert (= (> 7 5) true) "7 > 5")

# ============================================================================
# Conditionals
# ============================================================================

# test_if_true
(assert (= (if true 100 200) 100) "if true")

# test_if_false
(assert (= (if false 100 200) 200) "if false")

# test_if_with_condition
(assert (= (if (> 5 3) 100 200) 100) "if with true condition")
(assert (= (if (< 5 3) 100 200) 200) "if with false condition")

# test_nested_if
(assert (= (if (> 5 3) (if (< 2 4) 1 2) 3) 1) "nested if")

# test_if_nil_else
(assert (= (if false 100) nil) "if without else returns nil")

# ============================================================================
# Lists
# ============================================================================

# test_list_construction
(assert (list? (list 1 2 3)) "list construction is list")
(assert (= (length (list 1 2 3)) 3) "list construction length")

# test_cons
(def @cons-list (cons 1 (cons 2 (cons 3 nil))))
(assert (list? cons-list) "cons builds list")
(assert (= (first cons-list) 1) "cons first")
(assert (= (first (rest cons-list)) 2) "cons second")
(assert (= (first (rest (rest cons-list))) 3) "cons third")

# test_first_rest
(assert (= (first (list 10 20 30)) 10) "first of list")
(assert (= (first (rest (list 10 20 30))) 20) "second of list")
(assert (= (first (rest (rest (list 10 20 30)))) 30) "third of list")

# test_nested_lists
(def @nested (list (list 1 2) (list 3 4)))
(assert (= (length nested) 2) "nested lists length")
(assert (list? (first nested)) "nested list first is list")
(assert (list? (first (rest nested))) "nested list second is list")

# ============================================================================
# Quote
# ============================================================================

# test_quote_symbol
(assert (symbol? 'foo) "quoted symbol is symbol")

# test_quote_list
(assert (list? '(1 2 3)) "quoted list is list")

# ============================================================================
# Type predicates
# ============================================================================

# test_predicates
(assert (= (nil? nil) true) "nil? nil")
(assert (= (nil? 0) false) "nil? 0")
(assert (= (number? 42) true) "number? 42")
(assert (= (number? nil) false) "number? nil")
(assert (= (pair? (cons 1 2)) true) "pair? cons")
(assert (= (pair? nil) false) "pair? nil")

# ============================================================================
# Global definitions
# ============================================================================

# test_define_and_use
(def @x-def 42)
(assert (= (+ x-def 10) 52) "define and use")

# test_multiple_defines
(def @a-def 10)
(def @b-def 20)
(def @c-def 30)
(assert (= (+ a-def b-def c-def) 60) "multiple defines")

# ============================================================================
# Begin
# ============================================================================

# test_begin
(assert (= (begin
             1
             2
             3)
           3)
        "begin returns last")

# test_begin_with_side_effects
(assert (= (begin
             (def @x-begin 10)
             (def @y-begin 20)
             (+ x-begin y-begin))
           30)
        "begin with side effects")

# ============================================================================
# Complex expressions
# ============================================================================

# test_factorial_logic
(assert (= (if (<= 1 1) 1 (* 1 1)) 1) "factorial base case")
(assert (= (if (<= 5 1) 1 (* 5 120)) 600) "factorial recursive case")

# test_max_logic
(assert (= (if (> 10 5) 10 5) 10) "max 10 5")
(assert (= (if (> 3 7) 3 7) 7) "max 3 7")

# ============================================================================
# Error cases
# ============================================================================

# test_division_by_zero
(let [[ok? _] (protect ((fn [] (/ 10 0))))]
  (assert (not ok?) "division by zero errors"))

# test_type_error
(let [[ok? _] (protect ((fn [] (+ 1 nil))))]
  (assert (not ok?) "type error on + 1 nil"))

# test_undefined_variable
(let [[ok? _] (protect ((fn [] (eval 'undefined-var))))]
  (assert (not ok?) "undefined variable errors"))

# test_arity_error
# + accepts 0 args (identity)
(assert (= (+) 0) "plus zero args")
# first requires 1 arg — compile-time error, so use eval to catch it
(let [[ok? _] (protect ((fn [] (eval '(first)))))]
  (assert (not ok?) "first with no args errors"))

# ============================================================================
# Stress tests
# ============================================================================

# test_large_list — create list with 100 elements
(def @large-list
  (list 0
        1
        2
        3
        4
        5
        6
        7
        8
        9
        10
        11
        12
        13
        14
        15
        16
        17
        18
        19
        20
        21
        22
        23
        24
        25
        26
        27
        28
        29
        30
        31
        32
        33
        34
        35
        36
        37
        38
        39
        40
        41
        42
        43
        44
        45
        46
        47
        48
        49
        50
        51
        52
        53
        54
        55
        56
        57
        58
        59
        60
        61
        62
        63
        64
        65
        66
        67
        68
        69
        70
        71
        72
        73
        74
        75
        76
        77
        78
        79
        80
        81
        82
        83
        84
        85
        86
        87
        88
        89
        90
        91
        92
        93
        94
        95
        96
        97
        98
        99))
(assert (list? large-list) "large list is list")
(assert (= (length large-list) 100) "large list length")

# test_many_operations
(assert (= (+ 1 2 3 4 5 6 7 8 9 10) 55) "sum 1..10")
(assert (= (* 1 2 3 4 5) 120) "product 1..5")

# ============================================================================
# Logic combinations
# ============================================================================

# test_not
(assert (= (not true) false) "not true")
(assert (= (not false) true) "not false")
(assert (= (not nil) true) "not nil (falsy)")
(assert (= (not ()) false) "not empty list (truthy)")
(assert (= (not 0) false) "not 0 (truthy)")

# test_complex_conditionals
(assert (= (if (not (< 5 3)) 100 200) 100) "complex conditional not")
(assert (symbol? (if (= (+ 2 3) 5) 'yes 'no)) "conditional returns symbol")

# ============================================================================
# Standard library: length, append, reverse
# ============================================================================

# test_length
(assert (= (length (list 1 2 3 4 5)) 5) "length of 5-element list")
(assert (= (length nil) 0) "length of nil")

# test_append
(assert (= (append (append (list 1 2) (list 3 4)) (list 5)) (list 1 2 3 4 5))
        "append lists")

# test_reverse
(assert (= (reverse (list 1 2 3)) (list 3 2 1)) "reverse list")

# ============================================================================
# Math: min, max (integer parts only)
# ============================================================================

# test_min_max (integer parts)
(assert (= (min 5 3 7 2) 2) "min integers")
(assert (= (max 5 3 7 2) 7) "max integers")

# ============================================================================
# Math: abs (integer parts only)
# ============================================================================

# test_abs (integer parts)
(assert (= (abs -5) 5) "abs negative")
(assert (= (abs 5) 5) "abs positive")

# ============================================================================
# Math: type conversions (integer part only)
# ============================================================================

# test_type_conversions (integer part)
(assert (= (int 3.14) 3) "int truncates float")

# ============================================================================
# String operations
# ============================================================================

# test_string_length
(assert (= (length "hello") 5) "string length hello")
(assert (= (length "") 0) "string length empty")

# test_string_append
(assert (= (append (append "hello" " ") "world") "hello world") "string append")

# test_string_case
(assert (= (string-upcase "hello") "HELLO") "string upcase")
(assert (= (string-downcase "WORLD") "world") "string downcase")

# ============================================================================
# Indexed access
# ============================================================================

# test_get_indexed
(assert (= (get [10 20 30] 0) 10) "get array index 0")
(assert (= (get [10 20 30] 1) 20) "get array index 1")
(assert (= (get [10 20 30] 2) 30) "get array index 2")

# ============================================================================
# List utilities
# ============================================================================

# test_last
(assert (= (last (list 1 2 3 4 5)) 5) "last of list")

# test_take_drop
(assert (= (take 2 (list 1 2 3 4 5)) (list 1 2)) "take 2")
(assert (= (drop 2 (list 1 2 3 4 5)) (list 3 4 5)) "drop 2")

# ============================================================================
# type-of
# ============================================================================

# test_type
(assert (keyword? (type-of 42)) "type-of int is keyword")
(assert (keyword? (type-of 3.14)) "type-of float is keyword")
(assert (keyword? (type-of "hello")) "type-of string is keyword")

# test_type_of_list_consistency (Issue #308)
(def @type-empty (type-of ()))
(def @type-proper (type-of (list 1 2)))
(def @type-cons (type-of (cons 1 2)))
(assert (keyword? type-empty) "type-of empty list is keyword")
(assert (keyword? type-proper) "type-of proper list is keyword")
(assert (keyword? type-cons) "type-of cons cell is keyword")
(assert (= type-empty type-proper) "empty list and proper list same type")
(assert (= type-proper type-cons) "proper list and cons cell same type")
(assert (= (type-of ()) :list) "type-of () is :list")

# ============================================================================
# Math: floor, ceil, round
# ============================================================================

# test_floor_ceil_round
(assert (= (floor 3) 3) "floor int")
(assert (= (floor 3.7) 3) "floor float")
(assert (= (ceil 3) 3) "ceil int")
(assert (= (ceil 3.2) 4) "ceil float")
(assert (= (round 3) 3) "round int")
(assert (= (round 3.4) 3) "round down")
(assert (= (round 3.6) 4) "round up")

# ============================================================================
# String functions
# ============================================================================

# test_slice
(assert (= (slice "hello" 1 4) "ell") "slice middle")
(assert (= (slice "hello" 2 5) "llo") "slice to end")
(assert (= (slice "hello" 0 2) "he") "slice from start")

# test_string_index
(assert (= (string-index "hello" "l") 2) "string-index found")
(assert (= (string-index "hello" "x") nil) "string-index not found")

# test_get_string
(assert (= (get "hello" 0) "h") "get string 0")
(assert (= (get "hello" 1) "e") "get string 1")
(assert (= (get "hello" 4) "o") "get string 4")

# ============================================================================
# Array operations
# ============================================================================

# test_array_creation
(def @arr (@array 1 2 3))
(assert (= (length arr) 3) "array length 3")
(assert (= (get arr 0) 1) "array get 0")
(assert (= (get arr 1) 2) "array get 1")
(assert (= (get arr 2) 3) "array get 2")

# empty array
(assert (= (length (@array)) 0) "empty array length")

# test_array_length
(assert (= (length (@array 1 2 3)) 3) "array length")
(assert (= (length (@array)) 0) "array length empty")
(assert (= (length (@array 10 20 30 40 50)) 5) "array length 5")

# test_array_get
(assert (= (get (@array 10 20 30) 0) 10) "array get 0")
(assert (= (get (@array 10 20 30) 1) 20) "array get 1")
(assert (= (get (@array 10 20 30) 2) 30) "array get 2")

# test_array_put
(def @arr-put (put (@array 1 2 3) 1 99))
(assert (= (get arr-put 0) 1) "array put keeps 0")
(assert (= (get arr-put 1) 99) "array put sets 1")
(assert (= (get arr-put 2) 3) "array put keeps 2")

# put at beginning
(assert (= (get (put (@array 1 2 3) 0 100) 0) 100) "array put at beginning")

# put at end
(assert (= (get (put (@array 1 2 3) 2 200) 2) 200) "array put at end")

# ============================================================================
# Math: mod, rem, even?, odd?
# ============================================================================

# test_mod_and_remainder
(assert (= (mod 17 5) 2) "mod 17 5")
(assert (= (mod 20 4) 0) "mod 20 4")
(assert (= (mod -17 5) 3) "mod -17 5")
(assert (= (rem 17 5) 2) "rem 17 5")
(assert (= (rem 20 4) 0) "rem 20 4")

# test_even_odd
(assert (even? 2) "even? 2")
(assert (not (even? 3)) "even? 3")
(assert (not (odd? 2)) "odd? 2")
(assert (odd? 3) "odd? 3")
(assert (even? 0) "even? 0")

# ============================================================================
# Recursive functions
# ============================================================================

# test_recursive_lambda_fibonacci
(defn fib [n]
  (if (< n 2)
    n
    (+ (fib (- n 1)) (fib (- n 2)))))
(assert (= (fib 5) 5) "fibonacci 5")

# test_recursive_lambda_fibonacci_10
(assert (= (fib 10) 55) "fibonacci 10")

# test_tail_recursive_sum
(defn sum-to [n acc]
  (if (= n 0) acc (sum-to (- n 1) (+ acc n))))
(assert (= (sum-to 100 0) 5050) "tail recursive sum 1..100")

# test_recursive_countdown
(defn countdown [n]
  (if (<= n 0)
    0
    (+ n (countdown (- n 1)))))
(assert (= (countdown 5) 15) "recursive countdown")

# test_nested_recursive_functions
(defn outer [n]
  (defn inner [x]
    (if (< x 1)
      0
      (+ x (inner (- x 1)))))
  (inner n))
(assert (= (outer 5) 15) "nested recursive functions")

# ============================================================================
# Lambda basics
# ============================================================================

# test_simple_lambda_call
(defn identity-fn [x]
  x)
(assert (= (identity-fn 42) 42) "identity lambda")

# test_lambda_with_arithmetic
(defn double [x]
  (* x 2))
(assert (= (double 21) 42) "double lambda")

# test_lambda_with_comparison
(defn is-positive [x]
  (> x 0))
(assert (is-positive 5) "is-positive lambda")

# ============================================================================
# Closure scoping
# ============================================================================

# test_closure_captures_outer_variable
(def @x-outer 100)
(assert (= ((fn [y] (+ x-outer y)) 20) 120) "closure captures outer")

# test_closure_parameter_shadowing
(def @x-shadow 100)
(assert (= ((fn [x-shadow] (+ x-shadow 1)) 50) 51) "closure param shadowing")

# test_closure_captures_multiple_variables
(def @cx 10)
(def @cy 20)
(def @cz 30)
(assert (= ((fn [a b c] (+ a b c cx cy cz)) 1 2 3) 66)
        "closure captures multiple")

# test_closure_parameter_in_nested_expression
(assert (= ((fn [x] (if (> x 50) (* x 2) (+ x 100))) 25) 125)
        "closure param in nested expr")

# test_multiple_closures_independent_params
(def f1 (fn [x] (+ x 10)))
(def f2 (fn [x] (* x 2)))
(assert (= (+ (f1 5) (f2 5)) 25) "multiple closures independent")

# test_closure_captured_function_call
(def add-fn (fn [a b] (+ a b)))
(assert (= ((fn [x y] (add-fn x y)) 10 20) 30) "closure captured function")

# test_closure_with_list_operations
(def @numbers (list 1 2 3 4 5))
(assert (= ((fn [lst] (first lst)) numbers) 1) "closure with list ops")

# test_closure_parameter_in_conditional
(assert (= ((fn [n] (if (nil? n) "empty" "nonempty")) (list 1)) "nonempty")
        "closure param in conditional")

# test_closure_preserves_parameter_type
(assert ((fn [s] (string? s)) "hello") "closure preserves param type")

# ============================================================================
# Let bindings
# ============================================================================

# test_let_simple_binding
(assert (= (let [x 5]
             x)
           5)
        "let simple binding")

# test_let_with_arithmetic
(assert (= (let [x 5]
             (+ x 3))
           8)
        "let with arithmetic")

# test_let_multiple_bindings
(assert (= (let [x 5
                 y 3]
             (+ x y))
           8)
        "let multiple bindings")

# test_let_binding_with_expressions
(assert (= (let [x (+ 2 3)
                 y (* 4 5)]
             (+ x y))
           25)
        "let binding with expressions")

# test_let_shadowing_global
(def @x-let-shadow 10)
(assert (= (let [x-let-shadow 20]
             x-let-shadow)
           20)
        "let shadows global")

# test_let_does_not_modify_global
(def @x-let-global 10)
(let [x-let-global 20]
  x-let-global)
(assert (= x-let-global 10) "let does not modify global")

# test_let_with_lists
(assert (= (let [lst (list 1 2 3)]
             (first lst))
           1)
        "let with lists")

# test_let_with_string_operations
(assert (let [s "hello"]
          (string? s))
        "let with string ops")

# test_let_with_conditional
(assert (= (let [x 10]
             (if (> x 5) "big" "small"))
           "big")
        "let with conditional")

# test_let_empty_body_returns_nil
(assert (= (let [x 5]) nil) "let empty body returns nil")

# test_let_multiple_body_expressions
(assert (= (let [x 5]
             (+ x 1)
             (+ x 2)
             (+ x 3))
           8)
        "let multiple body returns last")

# test_let_with_global_reference
(def @y-let-ref 100)
(assert (= (let [x 50]
             (+ x y-let-ref))
           150)
        "let with global reference")

# test_let_binding_order
(assert (= (let [x 1
                 y 2
                 z 3]
             (+ x y z))
           6)
        "let binding order")

# test_let_with_list_literal
(assert (= (let [x '(1 2 3)]
             (rest x))
           (list 2 3))
        "let with quoted list")

# test_let_shadowing_with_calculation
(def @x-let-calc 10)
(assert (= (let [x-let-calc (* 2 x-let-calc)]
             x-let-calc)
           20)
        "let shadowing with calculation")

# test_let_with_builtin_functions
(assert (= (let [len (fn [x] 42)]
             (len nil))
           42)
        "let with builtin function override")

# ============================================================================
# let* (sequential bindings)
# ============================================================================

# test_let_star_empty
(assert (= (let* []
             42)
           42)
        "let* empty bindings")

# test_let_star_simple_binding
(assert (= (let* [x 5]
             x)
           5)
        "let* simple binding")

# test_let_star_with_multiple_bindings_no_dependencies
(assert (= (let* [x 1
                  y 2]
             (+ x y))
           3)
        "let* multiple bindings")

# ============================================================================
# cond
# ============================================================================

# NOTE: cond is a macro that triggers the match-in-closure bug when used
# directly inside assert-eq. Bind results to a var first.

# test_cond_single_true_clause
(def @cond-r1
  (cond
    true 42))
(assert (= cond-r1 42) "cond single true")

# test_cond_single_false_clause_with_else
(def @cond-r2
  (cond
    false 42
    100))
(assert (= cond-r2 100) "cond false with else")

# test_cond_single_false_clause_without_else
(def @cond-r3
  (cond
    false 42))
(assert (= cond-r3 nil) "cond false without else")

# test_cond_first_clause_matches
(def @cond-r4
  (cond
    (> 5 3) 100
    (> 4 2) 200))
(assert (= cond-r4 100) "cond first matches")

# test_cond_second_clause_matches
(def @cond-r5
  (cond
    (> 3 5) 100
    (> 4 2) 200))
(assert (= cond-r5 200) "cond second matches")

# test_cond_multiple_clauses_with_else
(def @cond-r6
  (cond
    (> 3 5) 100
    (> 2 4) 200
    300))
(assert (= cond-r6 300) "cond multiple with else")

# test_cond_with_expressions_as_conditions
(def @cond-r7
  (cond
    (= 1 2) "one-two"
    (= 2 2) "two-two"
    "other"))
(assert (= cond-r7 "two-two") "cond with expression conditions")

# test_cond_with_complex_bodies
(def @cond-r8
  (cond
    false (+ 1 1)
    true (+ 2 3)
    (+ 4 5)))
(assert (= cond-r8 5) "cond complex bodies")

# test_cond_with_multiple_body_expressions
(def @cond-r9
  (cond
    true (begin
           (+ 1 1)
           (+ 2 2)
           (+ 3 3))))
(assert (= cond-r9 6) "cond multiple body exprs")

# test_cond_nested
(def @cond-r10
  (cond
    true (cond
           true 42
           100)
    200))
(assert (= cond-r10 42) "cond nested")

# test_cond_with_variable_references
(def @x-cond 10)
(def @cond-r11
  (cond
    (< x-cond 5) "small"
    (< x-cond 15) "medium"
    "large"))
(assert (= cond-r11 "medium") "cond with variable references")

# test_cond_respects_clause_order
(def @cond-r12
  (cond
    (>= 10 5) "first"
    (>= 10 3) "second"
    "third"))
(assert (= cond-r12 "first") "cond respects clause order")

# test_cond_with_else_body_multiple_expressions
(def @cond-r13
  (cond
    false 100
    (begin
      (+ 1 1)
      (+ 2 2)
      (* 3 3))))
(assert (= cond-r13 9) "cond else multiple body exprs")

# ============================================================================
# Nested lambdas with closure capture
# ============================================================================

# test_nested_lambda_single_capture
(def make-const (fn [x] (fn [y] x)))
(def @f-const (make-const 42))
(assert (= (f-const 100) 42) "nested lambda single capture")

# test_nested_lambda_parameter_only
(def make-id (fn [x] (fn [y] y)))
(def @f-id (make-id 100))
(assert (= (f-id 42) 42) "nested lambda parameter only")

# ============================================================================
# Threading operators
# ============================================================================

# test_thread_first_simple
(assert (= (-> 5
               (+ 10)
               (* 2))
           30)
        "thread-first simple")

# test_thread_first_with_multiple_args
(assert (= (-> 5
               (+ 10 2)
               (* 3))
           51)
        "thread-first multiple args")

# test_thread_last_simple
(assert (= (->> 5
                (+ 10)
                (* 2))
           30)
        "thread-last simple")

# test_thread_last_with_multiple_args
(assert (= (->> 2
                (+ 10)
                (* 3))
           36)
        "thread-last multiple args")

# test_thread_first_chain
(assert (= (-> 1
               (+ 1)
               (+ 1)
               (+ 1))
           4)
        "thread-first chain")

# test_thread_last_chain
(assert (= (->> 1
                (+ 1)
                (+ 1)
                (+ 1))
           4)
        "thread-last chain")

# test_thread_first_with_list_ops
(assert (= (-> (list 1 2 3)
               (length))
           3)
        "thread-first list ops")

# test_thread_last_with_list_ops
(assert (= (->> (list 1 2 3)
                (length))
           3)
        "thread-last list ops")

# test_thread_first_nested
(assert (= (-> 10
               (- 3)
               (+ 5))
           12)
        "thread-first nested")

# test_thread_last_nested
(assert (= (->> 10
                (- 3)
                (+ 5))
           -2)
        "thread-last nested")

# ============================================================================
# Threading operator variants: as->, some->, some->>
# ============================================================================

# test_as_thread_zero_forms
(assert (= (as-> 42 x) 42) "as-> zero forms returns value")

# test_as_thread_single_form
(assert (= (as-> 5 x (+ x 10)) 15) "as-> single form")

# test_as_thread_simple
(assert (= (as-> 5 x (+ x 1) (* x 2)) 12) "as-> simple chain")

# test_as_thread_mixed_position
# x appears in different argument positions across steps
(assert (= (as-> 10 x (- x 3) (- 100 x)) 93) "as-> mixed argument position")

# test_as_thread_complex_expr
(assert (= (as-> (list 1 2 3) v (length v) (* v v)) 9) "as-> complex expr")

# test_some_thread_first_zero_forms
(assert (= (some-> 42) 42) "some-> zero forms returns value")

# test_some_thread_first_nil_zero_forms
(assert (= (some-> nil) nil) "some-> nil input zero forms")

# test_some_thread_first_simple
(assert (= (some-> 5
                   (+ 1)
                   (* 2))
           12)
        "some-> simple chain")

# test_some_thread_first_nil_input
(assert (= (some-> nil
                   (+ 1))
           nil)
        "some-> short-circuits on nil input")

# test_some_thread_first_nil_midchain
(def some-test-fn (fn (x) nil))
(assert (= (some-> 5
                   some-test-fn
                   (+ 1))
           nil)
        "some-> short-circuits mid-chain")

# test_some_thread_first_false_passes_through
# false is falsy but not nil; some-> must not short-circuit on false
(assert (= (some-> false
                   not)
           true)
        "some-> false is not nil, passes through")

# test_some_thread_last_zero_forms
(assert (= (some->> 42) 42) "some->> zero forms returns value")

# test_some_thread_last_simple
(assert (= (some->> 5
                    (+ 1)
                    (* 2))
           12)
        "some->> simple chain")

# test_some_thread_last_nil_input
(assert (= (some->> nil
                    (+ 1))
           nil)
        "some->> short-circuits on nil input")

# test_some_thread_last_nil_midchain
(assert (= (some->> 5
                    some-test-fn
                    (+ 1))
           nil)
        "some->> short-circuits mid-chain")

# test_some_thread_last_position
# (->> 2 (- 10) (* 3)) => (* 3 (- 10 2)) => (* 3 8) => 24
(assert (= (some->> 2
                    (- 10)
                    (* 3))
           24)
        "some->> inserts value as last argument")

# ============================================================================
# Closure with local define and param arithmetic
# ============================================================================

# test_closure_with_local_define_and_param_arithmetic
(let [outer-fn (fn [x]
                 (begin
                   (def @local (* x 2))
                   (fn [y] (+ local y))))]
  (assert (= ((outer-fn 1) 1) 3) "closure with local define and param"))

# ============================================================================
# Bug fix: let inside lambda with append
# ============================================================================

# test_let_inside_lambda_with_append
(defn f-append [x]
  (if (= x 0)
    (list)
    (let [y x]
      (append (list y) (f-append (- x 1))))))
(assert (= (f-append 3) (list 3 2 1)) "let inside lambda with append")

# test_let_inside_lambda_values_correct
(defn f-let-val [x]
  (let [y x]
    y))
(assert (= (f-let-val 42) 42) "let inside lambda values correct")

# test_multiple_let_bindings_in_lambda
(defn f-multi-let [x]
  (let [y x
        z (+ x 1)]
    (+ y z)))
(assert (= (f-multi-let 10) 21) "multiple let bindings in lambda")

# ============================================================================
# Bug fix: defn
# ============================================================================

# test_define_shorthand
(defn f-short [x]
  (+ x 1))
(assert (= (f-short 42) 43) "defn shorthand")

# test_define_shorthand_multiple_params
(defn add-short [a b]
  (+ a b))
(assert (= (add-short 3 4) 7) "defn multiple params")

# test_define_shorthand_with_body
(defn fact [n]
  (if (= n 0)
    1
    (* n (fact (- n 1)))))
(assert (= (fact 5) 120) "defn factorial")

# ============================================================================
# Bug fix: List display (no dot)
# ============================================================================

# test_list_display_no_dot
(assert (= (string (list 1 2 3)) "(1 2 3)") "list display no dot")

# test_single_element_list_display
(assert (= (string (list 1)) "(1)") "single element list display")

# test_empty_list_display
(assert (= (string (list)) "()") "empty list display")

# NOTE: halt tests stay in Rust — halt terminates the entire program,
# so it cannot be tested in a script that runs multiple assertions.
# NOTE: error message content test (issue #300) stays in Rust — requires
# checking that error strings contain specific substrings.

# ============================================================================
# Float precision tests (migrated from integration/core.rs)
# ============================================================================

# test_int_float_mixing
(assert (< (abs (- (+ 1 2.5) 3.5)) 0.0000000001) "int+float mixing: 1+2.5=3.5")
(assert (< (abs (- (* 2 3.5) 7.0)) 0.0000000001) "int*float mixing: 2*3.5=7.0")

# test_min_max_float
(assert (< (abs (- (min 1.5 2 0.5) 0.5)) 0.0000000001)
        "min float: min(1.5,2,0.5)=0.5")

# test_abs_float
(assert (< (abs (- (abs -3.5) 3.5)) 0.0000000001) "abs float: abs(-3.5)=3.5")

# test_type_conversions_float
(assert (< (abs (- (float 5) 5.0)) 0.0000000001)
        "float conversion: (float 5)=5.0")

# test_sqrt
(assert (= (sqrt 4) 2.0) "sqrt 4 = 2.0")
(assert (= (sqrt 9) 3.0) "sqrt 9 = 3.0")
(assert (< (abs (- (sqrt 16.0) 4.0)) 0.0001) "sqrt 16.0 ≈ 4.0")

# test_trigonometric
(assert (< (abs (sin 0)) 0.0001) "sin(0) ≈ 0")
(assert (< (abs (- (cos 0) 1.0)) 0.0001) "cos(0) ≈ 1")
(assert (< (abs (tan 0)) 0.0001) "tan(0) ≈ 0")

# test_log_functions
(assert (< (abs (log 1)) 0.0001) "log(1) ≈ 0")
(assert (< (abs (- (log 8 2) 3.0)) 0.0001) "log(8,2) ≈ 3")

# test_exp
(assert (< (abs (- (exp 0) 1.0)) 0.0001) "exp(0) ≈ 1")
(assert (< (abs (- (exp 1) 2.718281828)) 0.0001) "exp(1) ≈ e")

# test_pow
(assert (= (pow 2 3) 8) "pow 2 3 = 8")
(assert (< (abs (- (pow 2 -1) 0.5)) 0.0001) "pow(2,-1) ≈ 0.5")
(assert (< (abs (- (pow 2.0 3.0) 8.0)) 0.0001) "pow(2.0,3.0) ≈ 8.0")

# test_math_constants
(assert (< (abs (- (pi) 3.14159265)) 0.0001) "pi ≈ 3.14159")
(assert (< (abs (- (e) 2.71828182)) 0.0001) "e ≈ 2.71828")

# ============================================================================
# Stress tests (migrated from integration/core.rs)
# ============================================================================

# ============================================================================
# nonempty? predicate
# ============================================================================

(assert (nonempty? (list 1)) "nonempty? true for non-empty list")
(assert (not (nonempty? (list))) "nonempty? false for empty list")
(assert (nonempty? [1]) "nonempty? true for non-empty array")
(assert (not (nonempty? [])) "nonempty? false for empty array")
(assert (nonempty? @[1]) "nonempty? true for non-empty @array")
(assert (not (nonempty? @[])) "nonempty? false for empty @array")
(assert (nonempty? "hello") "nonempty? true for non-empty string")
(assert (not (nonempty? "")) "nonempty? false for empty string")
(assert (nonempty? {:x 1}) "nonempty? true for non-empty struct")
(assert (not (nonempty? {})) "nonempty? false for empty struct")
(assert (nonempty? |1|) "nonempty? true for non-empty set")
(assert (not (nonempty? ||)) "nonempty? false for empty set")
(assert (nonempty? (bytes 1 2)) "nonempty? true for non-empty bytes")
(assert (not (nonempty? (bytes))) "nonempty? false for empty bytes")
(assert (= :type-error (try
                         (nonempty? nil)
                         (catch e e:error)))
        "nonempty? errors on nil")
(assert (= :type-error (try
                         (nonempty? 42)
                         (catch e e:error)))
        "nonempty? errors on non-container")

# test_deep_arithmetic — 50 nested additions
(let [a (+ (+ (+ (+ (+ (+ (+ (+ (+ (+ 1 1) 1) 1) 1) 1) 1) 1) 1) 1) 1)
      b (+ (+ (+ (+ (+ (+ (+ (+ (+ (+ a 1) 1) 1) 1) 1) 1) 1) 1) 1) 1)
      c (+ (+ (+ (+ (+ (+ (+ (+ (+ (+ b 1) 1) 1) 1) 1) 1) 1) 1) 1) 1)
      d (+ (+ (+ (+ (+ (+ (+ (+ (+ (+ c 1) 1) 1) 1) 1) 1) 1) 1) 1) 1)
      e (+ (+ (+ (+ (+ (+ (+ (+ (+ (+ d 1) 1) 1) 1) 1) 1) 1) 1) 1) 1)]
  (assert (= e 51) "deep arithmetic: 50 nested additions"))
