# Integration tests for destructuring patterns in def, var, let, let*, fn, defn
#
# Migrated from tests/integration/destructuring.rs
# Tests that check error messages stay in Rust (3 tests).

(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false :assert-list-eq assert-list-eq :assert-equal assert-equal :assert-not-nil assert-not-nil :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind} ((import-file "tests/elle/assert.lisp")))

# Helper: assert that (thunk) signals an error
# Uses protect (not try/catch) to correctly capture VM-level signals
# from destructuring instructions.
(defn assert-err [thunk msg]
  "Assert that (thunk) signals an error"
  (let (([ok? _] (protect (thunk))))
    (assert (not ok?) msg)))

# ============================================================
# def: list destructuring
# ============================================================

# test_def_list_basic
(begin
  (def (a b c) (list 1 2 3))
  (assert-eq a 1 "def list basic: a")
  (assert-eq b 2 "def list basic: b")
  (assert-eq c 3 "def list basic: c"))

# test_def_list_short_source — missing elements signal error
(begin
  (def (a-ok) (list 1))
  (assert-eq a-ok 1 "def list short: present element ok"))
(assert-err (fn () (def (a b c) (list 1)))
  "def list short: error on missing element")

# test_def_list_empty_source — empty source signals error
(assert-err (fn () (def (a b) (list)))
  "def list empty: error on empty list")

# test_def_list_extra_elements_ignored
(begin
  (def (a b) (list 1 2 3 4))
  (assert-eq a 1 "def list extra: a")
  (assert-eq b 2 "def list extra: b"))

# test_def_list_wrong_type — wrong type signals error
(assert-err (fn () (def (a b) 42))
  "def list wrong type: error")

# ============================================================
# def: @array/array destructuring
# ============================================================

# test_def_array_basic — [x y] destructures arrays
(begin
  (def [x y] [10 20])
  (assert-eq x 10 "def array basic: x")
  (assert-eq y 20 "def array basic: y"))

# test_def_array_short_source — missing elements signal error
(begin
  (def [x-ok] [10])
  (assert-eq x-ok 10 "def array short: present element ok"))
(assert-err (fn () (def [x y z] [10]))
  "def array short: error on missing element")

# test_def_array_wrong_type — wrong type signals error
(assert-err (fn () (def [a b] 42))
  "def array wrong type: error")

# ============================================================
# def: nested destructuring
# ============================================================

# test_def_nested_list
(begin
  (def ((a b) c) (list (list 1 2) 3))
  (assert-eq a 1 "def nested list: a")
  (assert-eq b 2 "def nested list: b")
  (assert-eq c 3 "def nested list: c"))

# test_def_nested_array_in_list
(begin
  (def ([x y] z) (list [10 20] 30))
  (assert-eq x 10 "def nested array in list: x")
  (assert-eq y 20 "def nested array in list: y")
  (assert-eq z 30 "def nested array in list: z"))

# ============================================================
# var: mutable destructuring
# ============================================================

# test_var_list_basic
(begin
  (var (a b) (list 1 2))
  (assert-eq a 1 "var list basic: a")
  (assert-eq b 2 "var list basic: b"))

# test_var_destructured_bindings_are_mutable
(block
  (var (a b) (list 1 2))
  (assign a 10)
  (assert-eq a 10 "var destructured mutable: a after set"))

# ============================================================
# let: destructuring in bindings
# ============================================================

# test_let_list_destructure
(assert-eq (let (((a b) (list 10 20))) (+ a b)) 30
  "let list destructure")

# test_let_array_destructure
(assert-eq (let (([x y] [3 4])) (+ x y)) 7
  "let array destructure")

# test_let_mixed_bindings
(assert-eq (let ((a 1) ((b c) (list 2 3))) (+ a b c)) 6
  "let mixed bindings")

# test_let_nested_destructure
(assert-eq (let ((((a b) c) (list (list 1 2) 3))) (+ a b c)) 6
  "let nested destructure")

# ============================================================
# let*: sequential destructuring
# ============================================================

# test_let_star_destructure_basic
(assert-eq (let* (((a b) (list 1 2)) (c (+ a b))) c) 3
  "let* destructure basic")

# test_let_star_destructure_sequential_reference
(assert-eq (let* (((a b) (list 1 2)) ((c d) (list a b))) (+ c d)) 3
  "let* destructure sequential reference")

# test_let_star_mixed_simple_and_destructure
(assert-eq (let* ((x 10) ((a b) (list x 20))) (+ a b)) 30
  "let* mixed simple and destructure")

# test_let_star_shadowing_with_destructure
(assert-eq (let* ((a 1) ((a b) (list 10 20))) a) 10
  "let* shadowing with destructure")

# ============================================================
# fn: parameter destructuring
# ============================================================

# test_fn_list_param
(assert-eq ((fn ((a b)) (+ a b)) (list 3 4)) 7
  "fn list param")

# test_fn_array_param
(assert-eq ((fn ([x y]) (+ x y)) [5 6]) 11
  "fn array param")

# test_fn_mixed_params
(assert-eq ((fn (x (a b)) (+ x a b)) 10 (list 20 30)) 60
  "fn mixed params")

# test_fn_nested_param
(assert-eq ((fn (((a b) c)) (+ a b c)) (list (list 1 2) 3)) 6
  "fn nested param")

# ============================================================
# defn: destructuring in named function params
# ============================================================

# test_defn_with_destructured_param
(begin
  (defn f ((a b)) (+ a b))
  (assert-eq (f (list 3 4)) 7 "defn with destructured param"))

# test_defn_mixed_params
(begin
  (defn f2 (x (a b)) (+ x a b))
  (assert-eq (f2 10 (list 20 30)) 60 "defn mixed params"))

# ============================================================
# Edge cases
# ============================================================

# test_destructure_single_element_list
(begin
  (def (a) (list 42))
  (assert-eq a 42 "destructure single element list"))

# test_destructure_single_element_array
(begin
  (def [a] [42])
  (assert-eq a 42 "destructure single element array"))

# test_destructure_string_values
(begin
  (def (a b) (list "hello" "world"))
  (assert-string-eq a "hello" "destructure string values"))

# test_destructure_boolean_values
(begin
  (def (a b) (list true false))
  (assert-eq a true "destructure boolean: a is true")
  (assert-eq b false "destructure boolean: b is false"))

# test_destructure_nil_in_list
(begin
  (def (a b) (list nil 2))
  (assert-eq a nil "destructure nil in list: a is nil")
  (assert-eq b 2 "destructure nil in list: b"))

# test_destructure_in_closure_capture
(begin
  (def (a b) (list 1 2))
  (def f (fn () (+ a b)))
  (assert-eq (f) 3 "destructure in closure capture"))

# test_let_destructure_in_closure
(assert-eq (let (((a b) (list 10 20))) ((fn () (+ a b)))) 30
  "let destructure in closure")

# ============================================================
# Wildcard _
# ============================================================

# test_wildcard_list_basic
(begin
  (def (_ b) (list 1 2))
  (assert-eq b 2 "wildcard list basic"))

# test_wildcard_list_middle
(begin
  (def (a _ c) (list 1 2 3))
  (assert-eq (+ a c) 4 "wildcard list middle"))

# test_wildcard_array_basic
(begin
  (def [_ y] [10 20])
  (assert-eq y 20 "wildcard array basic"))

# test_wildcard_multiple
(begin
  (def (_ _ c) (list 1 2 3))
  (assert-eq c 3 "wildcard multiple"))

# test_wildcard_in_let
(assert-eq (let (((_ b) (list 10 20))) b) 20
  "wildcard in let")

# test_wildcard_in_fn_param
(assert-eq ((fn ((_ b)) b) (list 10 20)) 20
  "wildcard in fn param")

# test_wildcard_nested
(begin
  (def ((_ b) c) (list (list 1 2) 3))
  (assert-eq (+ b c) 5 "wildcard nested"))

# ============================================================
# & rest: list destructuring
# ============================================================

# test_rest_list_basic
(begin
  (def (a & r) (list 1 2 3))
  (assert-eq a 1 "rest list basic: a")
  (assert-eq (first r) 2 "rest list basic: first r")
  (assert-eq (first (rest r)) 3 "rest list basic: second r"))

# test_rest_list_empty_rest
(begin
  (def (a b & r) (list 1 2))
  (assert-true (empty? r) "rest list empty rest"))

# test_rest_list_single_rest
(begin
  (def (a & r) (list 1))
  (assert-true (empty? r) "rest list single rest"))

# test_rest_list_all_rest
(begin
  (def (& r) (list 1 2 3))
  (assert-eq (first r) 1 "rest list all rest"))

# test_rest_list_in_let
(assert-eq (let (((a & r) (list 10 20 30))) (+ a (first r))) 30
  "rest list in let")

# test_rest_list_in_fn_param
(assert-eq ((fn ((a & r)) (+ a (first r))) (list 10 20)) 30
  "rest list in fn param")

# ============================================================
# & rest: array destructuring
# ============================================================

# test_rest_array_basic
(begin
  (def [a & r] [1 2 3])
  (assert-eq a 1 "rest array basic: a")
  (assert-eq (get r 0) 2 "rest array basic: r[0]")
  (assert-eq (get r 1) 3 "rest array basic: r[1]"))

# test_rest_array_empty_rest
(begin
  (def [a b & r] [1 2])
  (assert-eq (length r) 0 "rest array empty rest"))

# test_rest_array_in_let
(assert-eq (let (([a & r] [10 20 30])) (+ a (get r 0))) 30
  "rest array in let")

# test_rest_array_basic — rest binding preserves array type
(begin
  (def [a & r] [1 2 3])
  (assert-true (array? r) "rest array basic: r is an array")
  (assert-eq (get r 0) 2 "rest array basic: r[0]")
  (assert-eq (get r 1) 3 "rest array basic: r[1]"))

# test_rest_array_empty_rest
(begin
  (def [a b & r] [1 2])
  (assert-true (array? r) "rest array empty: r is an array")
  (assert-eq (length r) 0 "rest array empty rest"))

# test_rest_array_in_let
(assert-eq (let (([a & r] [10 20 30])) (+ a (get r 0))) 30
  "rest array in let")

# test_rest_array_type_preserved
(begin
  (def @[a & r] @[1 2 3])
  (assert-true (array? r) "rest array type: r is an array")
  (assert-eq (get r 0) 2 "rest array type: r[0]")
  (assert-eq (get r 1) 3 "rest array type: r[1]"))

# ============================================================
# Wildcard + rest combined
# ============================================================

# test_wildcard_with_rest
(begin
  (def (_ & r) (list 1 2 3))
  (assert-eq (first r) 2 "wildcard with rest"))

# test_wildcard_and_rest_array
(begin
  (def [_ & r] [10 20 30])
  (assert-eq (get r 0) 20 "wildcard and rest array"))

# ============================================================
# Variadic & rest in fn/lambda parameters
# ============================================================

# test_variadic_fn_rest_only
(assert-list-eq ((fn (& args) args) 1 2 3) (list 1 2 3)
  "variadic fn rest only")

# test_variadic_fn_rest_empty
(assert-true (empty? ((fn (& args) args)))
  "variadic fn rest empty")

# test_variadic_fn_fixed_and_rest
(assert-eq ((fn (a b & rest) (+ a b)) 10 20 30 40) 30
  "variadic fn fixed and rest")

# test_variadic_fn_rest_value
(assert-list-eq ((fn (a & rest) rest) 1 2 3) (list 2 3)
  "variadic fn rest value")

# test_variadic_fn_rest_single_extra
(assert-list-eq ((fn (a & rest) rest) 1 2) (list 2)
  "variadic fn rest single extra")

# test_variadic_fn_rest_no_extra
(assert-true (empty? ((fn (a & rest) rest) 1))
  "variadic fn rest no extra")

# test_variadic_defn
(begin
  (defn my-list (& items) items)
  (assert-list-eq (my-list 1 2 3) (list 1 2 3) "variadic defn"))

# test_variadic_defn_fixed_and_rest
(begin
  (defn f3 (x & rest) (cons x rest))
  (assert-list-eq (f3 1 2 3) (list 1 2 3) "variadic defn fixed and rest"))

# test_variadic_let_binding
(let ((f (fn (& args) args)))
  (assert-list-eq (f 10 20) (list 10 20) "variadic let binding"))

# test_variadic_recursive
(begin
  (defn my-len (& args)
    (def lst (first args))
    (if (empty? lst) 0
        (+ 1 (my-len (rest lst)))))
  (assert-eq (my-len (list 1 2 3)) 3 "variadic recursive"))

# test_variadic_tail_call
(begin
  (defn sum-list (acc lst)
    (if (empty? lst) acc
        (sum-list (+ acc (first lst)) (rest lst))))
  (defn sum-all (& nums)
    (sum-list 0 nums))
  (assert-eq (sum-all 1 2 3 4 5) 15 "variadic tail call"))

# test_variadic_closure_capture
(begin
  (def x 100)
  (defn add-to-x (& nums)
    (+ x (first nums)))
  (assert-eq (add-to-x 42) 142 "variadic closure capture"))

# test_variadic_higher_order
(begin
  (defn apply-fn (f & args)
    (f (first args)))
  (assert-eq (apply-fn (fn (x) (+ x 1)) 10) 11 "variadic higher order"))

# test_variadic_compile_time_arity_check
(begin
  (defn f4 (x & rest) x)
  (assert-eq (f4 1) 1 "variadic compile time arity: ok with 1 arg")
  (assert-err (fn () (eval '(begin (defn f5 (x & rest) x) (f5))))
    "variadic compile time arity: 0 args fails"))

# ============================================================
# Struct/@struct destructuring
# ============================================================

# test_def_struct_basic
(begin
  (def {:name n :age a} {:name "Alice" :age 30})
  (assert-string-eq n "Alice" "def struct basic: name")
  (assert-eq a 30 "def struct basic: age"))

# test_def_struct_missing_key — missing key signals error
(assert-err (fn () (def {:missing m} {:other 42}))
  "def struct missing key: error")

# test_def_struct_wrong_type — wrong type signals error
(assert-err (fn () (def {:x x} 42))
  "def struct wrong type: error")

# test_struct_missing_key_errors (was: test_struct_missing_key_is_nil)
(assert-err (fn () (def {:missing m2} {:other 42}))
  "struct missing key errors")

# test_struct_wrong_type_errors (was: test_struct_wrong_type_is_nil)
(assert-err (fn () (def {:x x2} 42))
  "struct wrong type errors")

# test_struct_empty_pattern
(begin
  (def {} {:x 1})
  (assert-eq :ok :ok "struct empty pattern"))

# test_var_struct
(begin
  (var {:x x3} {:x 99})
  (assert-eq x3 99 "var struct"))

# test_let_struct
(assert-eq (let (({:x x :y y} {:x 10 :y 20})) (+ x y)) 30
  "let struct")

# test_let_star_struct
(assert-eq (let* (({:x x} {:x 5}) ({:y y} {:y x})) (+ x y)) 10
  "let* struct")

# test_fn_param_struct
(begin
  (defn f6 ({:x x :y y}) (+ x y))
  (assert-eq (f6 {:x 3 :y 4}) 7 "fn param struct"))

# test_struct_nested
(begin
  (def {:point {:x px :y py}} {:point {:x 3 :y 4}})
  (assert-eq (+ px py) 7 "struct nested"))

# test_struct_with_mutable_@struct
(begin
    (def {:a a4} @{:a 99})
    (assert-eq a4 99 "struct with mutable @struct"))

# test_struct_in_match — bind match result to var (known bug workaround)
(var match-circle
  (match {:type :circle :radius 5}
    ({:type :circle :radius r} r)
    ({:type :square :side s} s)
    (_ 0)))
(assert-eq match-circle 5 "struct in match: circle")

# test_struct_match_fallthrough
(var match-square
  (match {:type :square :side 7}
    ({:type :circle :radius r} r)
    ({:type :square :side s} s)
    (_ 0)))
(assert-eq match-square 7 "struct match fallthrough: square")

# test_struct_match_wildcard_fallback
(var match-fallback
  (match 42
    ({:x x} x)
    (_ :no-match)))
(assert-eq match-fallback :no-match "struct match wildcard fallback")

# test_struct_expression_position
(assert-eq (get {:a 1 :b 2} :a) 1 "struct expression position")

# test_struct_empty
(begin
  (def {} {:x 1})
  (assert-eq :ok :ok "struct empty"))

# test_struct_mixed_with_list
(begin
  (def (a5 {:x x5}) (list 1 {:x 2}))
  (assert-eq (+ a5 x5) 3 "struct mixed with list"))

# test_struct_wildcard_value
(begin
  (def {:x _ :y y6} {:x 10 :y 20})
  (assert-eq y6 20 "struct wildcard value"))

# ============================================================
# Tuple destructuring
# ============================================================

# Helper: produce an error struct via fiber
(defn make-error-struct []
  "Trigger division-by-zero and capture the error struct"
  (let ([f (fiber/new (fn () (/ 1 0)) 1)])
    (fiber/resume f nil)
    (fiber/value f)))

# test_let_destructure_tuple
(let (({:error a :message b} (make-error-struct)))
  (assert-string-eq b "division by zero" "let destructure tuple: message"))

# test_let_destructure_tuple_first
(let (({:error a :message b} (make-error-struct)))
  (assert-eq a :division-by-zero "let destructure tuple: kind"))

# test_match_tuple_pattern_matches_tuple
(var match-tuple (match [1 2] ([a b] (+ a b)) (_ :no-match)))
(assert-eq match-tuple 3 "match array pattern matches array")

# test_match_tuple_pattern_does_not_match_array
(var match-tuple-arr (match @[1 2] ([a b] (+ a b)) (_ :no-match)))
(assert-eq match-tuple-arr :no-match "match array pattern does not match @array")

# test_match_array_pattern_matches_array
(var match-arr (match @[1 2] (@[a b] (+ a b)) (_ :no-match)))
(assert-eq match-arr 3 "match array pattern matches array")

# test_match_array_pattern_does_not_match_tuple
(var match-arr-tup (match [1 2] (@[a b] (+ a b)) (_ :no-match)))
(assert-eq match-arr-tup :no-match "match @array pattern does not match array")

# test_match_struct_pattern_matches_struct
(var match-struct (match {:a 1} ({:a x} x) (_ :no-match)))
(assert-eq match-struct 1 "match struct pattern matches struct")

# test_match_struct_pattern_does_not_match_mutable_@struct
(var match-struct-tbl (match @{:a 1} ({:a x} x) (_ :no-match)))
(assert-eq match-struct-tbl :no-match "match struct pattern does not match @struct")

# test_match_mutable_@struct_pattern_matches_mutable_@struct
(var match-tbl (match @{:a 1} (@{:a x} x) (_ :no-match)))
(assert-eq match-tbl 1 "match @struct pattern matches @struct")

# test_match_mutable_@struct_pattern_does_not_match_struct
(var match-tbl-str (match {:a 1} (@{:a x} x) (_ :no-match)))
(assert-eq match-tbl-str :no-match "match @struct pattern does not match struct")

# test_destructure_non_sequential_errors (was: test_destructure_non_sequential_gives_nil)
(assert-err (fn () (let (([a b] 42)) a))
  "destructure non-sequential int: error")
(assert-err (fn () (let (([a b] "hello")) a))
  "destructure non-sequential string: error")

# test_def_array_basic
(begin
    (def {:error a7 :message b7} (make-error-struct))
    (assert-eq a7 :division-by-zero "def array basic: kind")
    (assert-string-eq b7 "division by zero" "def array basic: message"))

# ============================================================
# &opt optional parameters
# ============================================================

# test_opt_basic_provided
(assert-eq ((fn (a &opt b) b) 1 2) 2 "opt basic provided")

# test_opt_basic_missing
(assert-eq ((fn (a &opt b) b) 1) nil "opt basic missing")

# test_opt_multiple
(assert-list-eq ((fn (a &opt b c) (list a b c)) 1)
  (list 1 nil nil) "opt multiple: none provided")
(assert-list-eq ((fn (a &opt b c) (list a b c)) 1 2)
  (list 1 2 nil) "opt multiple: one provided")
(assert-list-eq ((fn (a &opt b c) (list a b c)) 1 2 3)
  (list 1 2 3) "opt multiple: all provided")

# test_opt_too_many_args
(assert-err (fn () (eval '((fn (a &opt b) a) 1 2 3)))
  "opt too many args")

# test_opt_too_few_args
(assert-err (fn () (eval '((fn (a &opt b c) a))))
  "opt too few args")

# test_opt_with_rest
(assert-list-eq ((fn (a &opt b & rest) (list a b rest)) 1)
  (list 1 nil ()) "opt with rest: none")
(assert-list-eq ((fn (a &opt b & rest) (list a b rest)) 1 2)
  (list 1 2 ()) "opt with rest: opt only")
(assert-list-eq ((fn (a &opt b & rest) (list a b rest)) 1 2 3 4)
  (list 1 2 (list 3 4)) "opt with rest: opt and rest")

# test_opt_defn
(begin
  (defn f7 (a &opt b) (list a b))
  (assert-list-eq (f7 1) (list 1 nil) "opt defn: missing")
  (assert-list-eq (f7 1 2) (list 1 2) "opt defn: provided"))

# test_opt_compile_time_arity
(assert-err (fn () (eval '(begin (defn f8 (a &opt b) a) (f8))))
  "opt compile time arity: too few")
(assert-err (fn () (eval '(begin (defn f9 (a &opt b) a) (f9 1 2 3))))
  "opt compile time arity: too many")

# test_opt_no_params_after
(assert-err (fn () (eval '(fn (&opt) 1)))
  "opt no params after &opt")

# test_opt_after_rest_error
(assert-err (fn () (eval '(fn (a & rest &opt b) 1)))
  "opt after rest error")

# test_opt_only
(assert-list-eq ((fn (&opt a b) (list a b)))
  (list nil nil) "opt only: none")
(assert-list-eq ((fn (&opt a b) (list a b)) 1)
  (list 1 nil) "opt only: one")
(assert-list-eq ((fn (&opt a b) (list a b)) 1 2)
  (list 1 2) "opt only: both")

# ============================================================
# &keys keyword arguments
# ============================================================

# test_keys_basic
(assert-eq ((fn (a &keys opts) opts) 1 :x 10 :y 20)
  {:x 10 :y 20} "keys basic")

# test_keys_empty
(assert-eq ((fn (a &keys opts) opts) 1)
  {} "keys empty")

# test_keys_destructure
(assert-eq ((fn (a &keys {:x x :y y}) (+ x y)) 1 :x 10 :y 20) 30
  "keys destructure")

# test_keys_missing_key_destructure — missing key now signals an error
(assert-err (fn () ((fn (a &keys {:x x :y y}) y) 1 :x 10))
  "keys missing key destructure signals error")

# test_keys_all_present — all keys provided, still works
(assert-eq ((fn (a &keys {:x x :y y}) (+ x y)) 1 :x 10 :y 20) 30
  "keys destructure all present")

# test_keys_with_opt
(assert-list-eq ((fn (a &opt b &keys opts) (list a b opts)) 1)
  (list 1 nil {}) "keys with opt: none")
(assert-list-eq ((fn (a &opt b &keys opts) (list a b opts)) 1 2)
  (list 1 2 {}) "keys with opt: opt only")
(assert-list-eq ((fn (a &opt b &keys opts) (list a b opts)) 1 2 :x 10)
  (list 1 2 {:x 10}) "keys with opt: opt and keys")

# test_keys_odd_args_error
(assert-err (fn () ((fn (a &keys opts) opts) 1 :x 10 :y))
  "keys odd args error")

# test_keys_non_keyword_key_error
(assert-err (fn () ((fn (a &keys opts) opts) 1 42 10))
  "keys non-keyword key error")

# test_keys_and_rest_exclusive
(assert-err (fn () (eval '(fn (a &keys opts & rest) 1)))
  "keys and rest exclusive: keys then rest")
(assert-err (fn () (eval '(fn (a & rest &keys opts) 1)))
  "keys and rest exclusive: rest then keys")

# test_keys_defn
(begin
  (defn f10 (a &keys opts) opts)
  (assert-eq (f10 1 :host "db" :port 3306) {:host "db" :port 3306}
    "keys defn"))

# ============================================================
# &named strict named parameters
# ============================================================

# test_named_basic
(assert-list-eq ((fn (a &named host port) (list host port)) 1 :host "db" :port 3306)
  (list "db" 3306) "named basic")

# test_named_missing_key
(assert-eq ((fn (a &named host port) port) 1 :host "db") nil
  "named missing key")

# test_named_unknown_key_error
(assert-err (fn () ((fn (a &named host) host) 1 :host "db" :port 3306))
  "named unknown key error")

# test_named_with_opt
(assert-list-eq ((fn (a &opt b &named host) (list a b host)) 1 :host "db")
  (list 1 nil "db") "named with opt")

# test_named_defn
(begin
  (defn connect (host &named port) (list host port))
  (assert-list-eq (connect "db" :port 3306) (list "db" 3306) "named defn"))

# test_named_odd_args_error
(assert-err (fn () ((fn (a &named host) host) 1 :host))
  "named odd args error")

# test_named_and_keys_exclusive
(assert-err (fn () (eval '(fn (a &keys opts &named host) 1)))
  "named and keys exclusive: keys then named")
(assert-err (fn () (eval '(fn (a &named host &keys opts) 1)))
  "named and keys exclusive: named then keys")

# test_named_no_params_error
(assert-err (fn () (eval '(fn (a &named) 1)))
  "named no params error")

# test_named_non_symbol_error
(assert-err (fn () (eval '(fn (a &named [x]) 1)))
  "named non-symbol error")

# ============================================================
# Edge case tests
# ============================================================

# test_opt_destructuring_pattern
(assert-list-eq ((fn (a &opt (b c)) (list a b c)) (list 1 2))
  (list (list 1 2) nil nil) "opt destructuring pattern: not provided")
(assert-list-eq ((fn (a &opt (b c)) (list a b c)) 1 (list 2 3))
  (list 1 2 3) "opt destructuring pattern: provided")

# test_keys_mutable_capture
(assert-eq
  ((fn (&keys opts)
     (let ((f (fn () opts)))
       (f)))
   :x 10)
  {:x 10}
  "keys mutable capture")

# test_keys_tail_call_error
(assert-err
  (fn ()
    (begin
      (defn f11 (a &keys opts) opts)
      (defn g () (f11 1 :x))
      (g)))
  "keys tail call error")

# test_opt_fiber_resume
(let ([co (coro/new (fn (&opt a) (+ (or a 0) (yield a))))])
  (coro/resume co)
  (assert-eq (coro/resume co 42) 42 "opt fiber resume: no initial arg"))

(let ([co (coro/new (fn (&opt a) (yield a) a))])
  (coro/resume co 10)
  (assert-eq (coro/resume co) 10 "opt fiber resume: with initial arg"))

# ============================================================
# Symbol keys in struct/@struct destructuring (#424)
# ============================================================

# test_def_struct_symbol_key
(begin
  (def {'a v} (struct 'a 42))
  (assert-eq v 42 "def struct symbol key"))

# test_let_struct_symbol_key
(assert-eq (let (({'a v} (struct 'a 42))) v) 42
  "let struct symbol key")

# test_fn_param_struct_symbol_key
(assert-eq ((fn ({'a v}) v) (struct 'a 42)) 42
  "fn param struct symbol key")

# test_mixed_keyword_and_symbol_keys
(begin
  (def {:k kv 'a sv} (struct :k 1 'a 2))
  (assert-eq kv 1 "mixed keys: keyword")
  (assert-eq sv 2 "mixed keys: symbol"))

# test_symbol_key_missing_errors (was: test_symbol_key_missing_returns_nil)
(assert-err (fn () (def {'missing m} (struct 'other 1)))
  "symbol key missing: error")

# test_match_struct_symbol_key
(var match-sym
  (match (struct 'a 42)
    ({'a v} v)
    (_ :no-match)))
(assert-eq match-sym 42 "match struct symbol key")

# test_match_@struct_symbol_key
(var match-tbl-sym
   (match @{'a 42}
     (@{'a v} v)
     (_ :no-match)))
(assert-eq match-tbl-sym 42 "match @struct symbol key")

# test_match_struct_symbol_key_missing_gives_nil
# Struct patterns match any struct (IsStruct guard); missing keys give nil
(var match-sym-missing
  (match (struct 'b 99)
    ({'a v} v)
    (_ :no-match)))
(assert-eq match-sym-missing nil "match struct symbol key missing gives nil")

# test_nested_symbol_key
(begin
  (def {'point {'x px 'y py}} (struct 'point (struct 'x 3 'y 4)))
  (assert-eq (+ px py) 7 "nested symbol key"))

# ============================================================
# letrec destructuring (#331)
# ============================================================

# test_letrec_list_destructure
(assert-eq (letrec (((a b) (list 1 2))) (+ a b)) 3
  "letrec list destructure")

# test_letrec_struct_destructure
(assert-eq (letrec (({:x x} {:x 42})) x) 42
  "letrec struct destructure")

# test_letrec_array_destructure
(assert-eq (letrec (([a b] [10 20])) (+ a b)) 30
  "letrec array destructure")

# test_letrec_mixed_simple_and_destructure
(assert-eq
  (letrec ((f (fn (x) (+ x a)))
           ((a b) (list 10 20)))
    (f b))
  30
  "letrec mixed simple and destructure")

# test_letrec_destructure_with_recursion
(assert-eq
  (letrec ((f (fn (n) (if (= n 0) base (f (- n 1)))))
           ({:base base} {:base 42}))
    (f 5))
  42
  "letrec destructure with recursion")

# test_letrec_nested_destructure
(assert-eq
  (letrec ((((a b) c) (list (list 1 2) 3)))
    (+ a b c))
  6
  "letrec nested destructure")

# test_letrec_wildcard_destructure
(assert-eq
  (letrec (((_ b) (list 1 2)))
    b)
  2
  "letrec wildcard destructure")

# Struct destructuring properties
# Migrated from tests/property/destructuring.rs
# ============================================================

# def_struct_roundtrip_int: (def {:a v} {:a X}) yields v == X
(begin
  (def {:a v} {:a 42})
  (assert-eq v 42 "struct destructure roundtrip: 42"))
(begin
  (def {:a v2} {:a -7})
  (assert-eq v2 -7 "struct destructure roundtrip: -7"))
(begin
  (def {:a v3} {:a 0})
  (assert-eq v3 0 "struct destructure roundtrip: 0"))

# def_struct_equiv_get: destructuring ≡ manual get
(let ((t {:a 10 :b 20}))
  (let (({:a a :b b} {:a 10 :b 20}))
    (assert-eq (+ a b) (+ (get t :a) (get t :b))
      "struct destructure equiv get")))

# def_struct_multi_key
(begin
  (def {:x x :y y :z z} {:x 1 :y 2 :z 3})
  (assert-eq (+ x (+ y z)) 6 "struct destructure multi-key"))

# def_struct_missing_key_errors (was: def_struct_missing_key_is_nil)
(assert-err (fn () (def {:missing m} {:other 42}))
  "struct missing key errors (property)")

# def_struct_non_struct_errors (was: def_struct_non_struct_is_nil)
(assert-err (fn () (def {:a a} 42))
  "struct non-struct errors (property)")

# fn_param_struct_equiv_get
(begin
  (defn f-destr ({:a a :b b}) (+ a b))
  (defn g-manual (t) (+ (get t :a) (get t :b)))
  (assert-eq (f-destr {:a 10 :b 20}) (g-manual {:a 10 :b 20})
    "fn param struct equiv get"))

# fn_param_struct_mixed: struct param + regular param
(begin
  (defn f-mixed ({:x x} y) (+ x y))
  (assert-eq (f-mixed {:x 10} 20) 30 "fn param struct mixed"))

# let_struct_destr
(assert-eq (let (({:a a :b b} {:a 3 :b 7})) (+ a b)) 10
  "let struct destructure (property)")

# let_star_struct_forward_ref
(assert-eq (let* (({:x v} {:x 5}) ({:y w} {:y v})) (+ v w)) 10
  "let* struct forward ref (property)")

# nested_struct_destr
(begin
  (def {:p {:x px :y py}} {:p {:x 3 :y 4}})
  (assert-eq (+ px py) 7 "nested struct destructure (property)"))

# nested_struct_missing_inner_errors (was: nested_struct_missing_inner)
(assert-err (fn () (def {:p {:missing m}} {:p {:x 42}}))
  "nested struct missing inner errors (property)")

# match_struct_extracts
(var mt-extract (match {:val 42} ({:val v} v) (_ :fail)))
(assert-eq mt-extract 42 "match struct extracts (property)")

# match_struct_rejects_non_struct
(var mt-reject (match 42 ({:a a} a) (_ :no-match)))
(assert-eq mt-reject :no-match "match struct rejects non-struct (property)")

# match_struct_literal_key_discriminates
(var mt-disc (match {:type :a :val 42}
  ({:type :b :val v} (+ v 1000))
  ({:type :a :val v} v)
  (_ :fail)))
(assert-eq mt-disc 42 "match struct literal key discriminates (property)")

# match_struct_wrong_literal_falls_through
(var mt-fall (match {:type :square :val 10}
  ({:type :circle :val v} v)
  ({:type :square :val v} (+ v 100))
  (_ :fail)))
(assert-eq mt-fall 110 "match struct wrong literal falls through (property)")

# match_mutable_table
(var mt-mut (match @{:val 42} (@{:val v} v) (_ :fail)))
(assert-eq mt-mut 42 "match mutable @struct (property)")

# ============================================================================
# Error tests (from integration/destructuring.rs)
# ============================================================================

# def_destructured_bindings_are_immutable
(assert-err (fn () (eval '(begin (def (a b) (list 1 2)) (assign a 10))))
  "def destructured bindings are immutable")

# variadic_arity_check_too_few
(assert-err (fn () (eval '((fn (a b & rest) a) 1)))
  "variadic arity check: too few args")

# keys_duplicate_keys
(assert-err (fn () ((fn (a &keys opts) (get opts :x)) 1 :x 10 :x 20))
  "duplicate keyword keys error")

# ============================================================================
# Struct rest-destructuring {& more}
# ============================================================================

# test_struct_rest_basic
(assert-eq
  (let [({:a a & rest} {:a 1 :b 2 :c 3})]
    rest)
  {:b 2 :c 3}
  "struct rest basic: captures remaining keys")

# test_struct_rest_empty_remainder
(assert-eq
  (let [({:a a :b b & rest} {:a 1 :b 2})]
    rest)
  {}
  "struct rest empty: no extra keys → empty struct")

# test_struct_rest_all_explicit
(assert-eq
  (let [({:a a & rest} {:a 10})]
    [a rest])
  [10 {}]
  "struct rest: all keys explicit → rest is empty struct")

# test_struct_rest_in_fn
(assert-eq
  ((fn ({:x x & rest}) rest) {:x 1 :y 2 :z 3})
  {:y 2 :z 3}
  "struct rest in fn param")

# test_struct_rest_result_is_immutable
(assert-eq
  (type-of (let [({:a a & rest} {:a 1 :b 2})] rest))
  :struct
  "struct rest result is always immutable struct")

# test_struct_rest_from_mutable
(assert-eq
  (let [({:a a & rest} @{:a 1 :b 2 :c 3})]
    rest)
  {:b 2 :c 3}
  "struct rest from @struct input yields immutable rest")

# test_struct_rest_in_match
(assert-eq
  (match {:x 1 :y 2 :z 3}
    ({:x x & rest} rest)
    (_ nil))
  {:y 2 :z 3}
  "struct rest in match pattern")

# test_struct_rest_table_in_match
(assert-eq
  (match @{:x 1 :y 2 :z 3}
    (@{:x x & rest} rest)
    (_ nil))
  {:y 2 :z 3}
  "struct rest on @struct in match pattern")

# test_keys_destructure_with_rest — combined &keys + struct rest
(assert-eq
  ((fn (a &keys {:x x & rest}) rest) 1 :x 10 :y 20 :z 30)
  {:y 20 :z 30}
  "keys destructure with rest: captures extra kwargs")

# test_keys_destructure_with_rest_no_extra
(assert-eq
  ((fn (a &keys {:x x & rest}) rest) 1 :x 10)
  {}
  "keys destructure with rest, no extra keys")

# test_keys_destructure_missing_required_with_rest
(assert-err (fn () ((fn (a &keys {:x x & rest}) rest) 1 :y 20))
  "keys destructure missing required key signals error even with rest")