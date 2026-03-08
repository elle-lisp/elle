(import-file "tests/elle/assert.lisp")

# ============================================================================
# SECTION 1: Deeply Nested Captures (4+ levels)
# ============================================================================

(assert-eq (((((fn (a) (fn (b) (fn (c) (fn (d) (+ a b c d))))) 1) 2) 3) 4) 10 "test_capture_from_great_grandparent")

(assert-eq ((((fn (x) (fn (y) (fn (z) (+ x z)))) 10) 20) 5) 15 "test_capture_skip_levels")

(assert-eq ((((((fn (a) (fn (b) (fn (c) (fn (d) (fn (e) (+ a b c d e)))))) 1) 2) 3) 4) 5) 15 "test_five_level_nesting")

(assert-eq (((((fn (a) (fn (b) (fn (c) (fn (d) (+ a c))))) 10) 20) 30) 40) 40 "test_capture_alternating_levels")

(assert-eq (((((fn (a) (fn (b) (fn (c) (fn (d) (* a (+ b (- c d))))))) 2) 3) 4) 1) 12 "test_deeply_nested_all_params")

# ============================================================================
# SECTION 2: Mixed Let/Lambda Captures
# ============================================================================

(assert-eq (let ((f ((fn (x)
                      (let ((y 10))
                        (fn () (+ x y)))) 5)))
             (f)) 15 "test_let_inside_lambda_capture")

(assert-eq (let ((a 1))
             (let ((f ((fn (b)
                         (let ((c 3))
                           (fn () (+ a b c)))) 2)))
               (f))) 6 "test_nested_let_lambda_let")

(assert-eq (let ((x 5))
             (let ((f (fn () x)))
               (f))) 5 "test_lambda_captures_let_binding")

(assert-eq (let ((x 10) (y 20))
             (let ((f1 (fn () x))
                   (f2 (fn () y)))
               (+ (f1) (f2)))) 30 "test_multiple_lambdas_same_let_scope")

(assert-eq (let ((outer 100))
             (let ((inner 50))
               (let ((f (fn () (+ outer inner))))
                 (f)))) 150 "test_lambda_in_let_captures_outer_let")

(assert-eq (let* ((x 1)
                  (y (+ x 1))
                  (f (fn () (+ x y))))
             (f)) 3 "test_let_star_with_lambda_capture")

# ============================================================================
# SECTION 3: Mutable Capture Edge Cases
# ============================================================================

(assert-eq (let ((x 0))
              (let ((inc (fn () (begin (assign x (+ x 1)) x))))
                (begin (inc) (inc) (inc)))) 3 "test_set_on_let_bound_capture")

(assert-eq ((fn ()
              (begin
                (var counter 0)
                 (def inc (fn () (begin (assign counter (+ counter 1)) counter)))
                (begin (inc) (inc) (inc))))) 3 "test_set_on_locally_defined_capture")

(assert-eq (let ((x 0))
              (let ((inc (fn () (assign x (+ x 1))))
                    (get (fn () x)))
                (begin (inc) (inc) (get)))) 2 "test_multiple_closures_share_mutable_capture")

(assert-eq (let ((x 0))
             (let ((f (fn () (let ((y 0))
                                (fn () (begin (assign x (+ x 1)) (assign y (+ y 1)) (+ x y)))))))
               (let ((g (f)))
                 (begin (g) (g) (g))))) 6 "test_nested_mutable_captures")

(assert-eq (let ((counter 0))
              (let ((f (fn () (fn () (begin (assign counter (+ counter 1)) counter)))))
               (let ((g (f)))
                 (begin (g) (g) (g))))) 3 "test_mutable_capture_across_lambda_levels")

(assert-eq (let ((x 0) (y 0))
              (let ((inc-x (fn () (assign x (+ x 1))))
                    (inc-y (fn () (assign y (+ y 1))))
                   (sum (fn () (+ x y))))
               (begin (inc-x) (inc-y) (inc-x) (sum)))) 3 "test_multiple_mutable_captures")

# ============================================================================
# SECTION 4: CPS/Coroutine Captures
# ============================================================================

(assert-eq (let ((x 10))
             (let ((y 20))
               (let ((gen (fn () (yield (+ x y)))))
                 (let ((co (make-coroutine gen)))
                   (coro/resume co))))) 30 "test_coroutine_captures_from_nested_let")

(assert-eq ((fn (base)
              (let ((gen (fn () (yield base))))
                (let ((co (make-coroutine gen)))
                  (coro/resume co)))) 42) 42 "test_coroutine_captures_lambda_param")

(assert-eq ((fn (a)
              ((fn (b)
                 (let ((gen (fn () (yield (+ a b)))))
                   (let ((co (make-coroutine gen)))
                     (coro/resume co)))) 20)) 10) 30 "test_coroutine_captures_multiple_levels")

(assert-eq (let ((counter 0))
              (let ((gen (fn () (begin (assign counter (+ counter 1)) (yield counter)))))
               (let ((co (make-coroutine gen)))
                 (coro/resume co)))) 1 "test_coroutine_with_mutable_capture")

(assert-eq (let* ((x 5)
                  (y (+ x 10))
                  (gen (fn () (yield (+ x y))))
                  (co (make-coroutine gen)))
             (coro/resume co)) 20 "test_coroutine_captures_let_star_binding")

# ============================================================================
# SECTION 5: Complex Interaction Tests
# ============================================================================

(assert-eq (let ((x 5))
             (let ((f (fn () (fn () x))))
               (let ((g (f)))
                 (g)))) 5 "test_closure_returning_closure_with_captures")

(assert-eq (let ((x 10))
             (let ((f (fn (x) (fn () x))))
               (let ((g (f 20)))
                 (g)))) 20 "test_shadowing_in_nested_scopes")

(assert-eq (let ((x 10))
             (let ((f (fn () (let ((x 20)) (fn () x)))))
               (let ((g (f)))
                 (g)))) 20 "test_capture_with_shadowing_outer")

(assert-eq (let ((x 5))
             (let ((f (fn () x))
                   (g (fn () (+ x x))))
               (+ (f) (g)))) 15 "test_multiple_captures_same_variable")

(assert-eq (let ((x 10))
             (let ((f (fn (cond) (if cond (fn () x) (fn () 0)))))
               (let ((g (f true)))
                 (g)))) 10 "test_capture_in_conditional")

(assert-eq (let ((x 0))
              (let ((f (fn () (begin (assign x (+ x 1)) x))))
                (begin (f) (f) (f) x))) 3 "test_capture_in_loop_body")

# ============================================================================
# SECTION 6: Edge Cases and Stress Tests
# ============================================================================

(assert-eq ((fn () 42)) 42 "test_empty_lambda_capture")

(assert-eq ((fn (x) 42) 10) 42 "test_lambda_unused_parameter")

(assert-eq (let ((x 10) (y 20))
             (let ((f (fn () x)))
               (f))) 10 "test_capture_unused_let_binding")

(assert-eq (let ((a 1) (b 2) (c 3) (d 4) (e 5))
             (let ((f (fn () (+ a b c d e))))
               (f))) 15 "test_many_captures_same_closure")

(assert-eq (let* ((a 1)
                  (b (+ a 1))
                  (c (+ b 1))
                  (f (fn () (+ a b c))))
             (f)) 6 "test_capture_in_nested_let_star")

(assert-eq (let ((x 10))
             (let ((f (fn (x) (+ x 5))))
               (f 20))) 25 "test_lambda_param_shadows_let_binding")

(assert-eq (let ((x 10))
             (let ((f (fn (x) (fn (x) x))))
               (let ((g (f 20)))
                 (g 30)))) 30 "test_nested_lambda_param_shadowing")

(assert-eq (let ((x 10))
             (let ((f (fn () (begin (var y (+ x 5)) y))))
               (f))) 15 "test_capture_with_define_in_lambda")

(assert-eq (let ((limit 4))
             (begin
               (def is-even (fn (n) (if (= n 0) true (is-odd (- n 1)))))
               (def is-odd (fn (n) (if (= n 0) false (is-even (- n 1)))))
               (is-even limit))) true "test_mutual_recursion_with_captures")

(assert-eq (let ((x 10))
             (begin
               (def f (fn () x))
               (f))) 10 "test_capture_across_define_boundary")

# ============================================================================
# SECTION 7: Regression Tests for Locally-Defined Variables
# ============================================================================

(assert-eq ((fn (n)
              (begin
                (def fact (fn (x) (if (= x 0) 1 (* x (fact (- x 1))))))
                (fact n))) 6) 720 "test_self_recursive_function_via_define_inside_fn")

(assert-eq ((fn ()
              (begin
                (var x 42)
                (def f (fn () x))
                (f)))) 42 "test_nested_lambda_capturing_locally_defined_variable")

(assert-eq ((fn (initial)
              (begin
                (var value initial)
                (def getter (fn () value))
                 (def setter (fn (new-val) (assign value new-val)))
                (setter 42)
                (getter))) 0) 42 "test_multiple_closures_sharing_mutable_state_via_define")

(assert-eq ((fn ()
              (begin
                (def is-even (fn (n) (if (= n 0) true (is-odd (- n 1)))))
                (def is-odd (fn (n) (if (= n 0) false (is-even (- n 1)))))
                (is-even 8)))) true "test_mutual_recursion_via_define_inside_fn")

# ============================================================================
# SECTION 8: let/letrec inside closures must use StoreCapture, not StoreLocal
# ============================================================================

(assert-eq (begin
             (def check (fn (val)
               (let ((temp (+ val 1)))
                 temp)))
             (let ((x 100))
               (check 5)
               x)) 100 "test_let_inside_closure_does_not_corrupt_caller_stack")

(assert-eq (begin
             (def f (fn (a b)
               (let ((sum (+ a b))
                     (diff (- a b)))
                 (+ sum diff))))
             (f 10 3)) 20 "test_let_inside_closure_returns_correct_value")

(assert-eq (begin
             (def process (fn (n)
               (letrec ((helper (fn (x) (if (= x 0) 0 (+ x (helper (- x 1)))))))
                 (helper n))))
             (let ((result 999))
               (process 5)
               result)) 999 "test_letrec_inside_closure_does_not_corrupt_caller_stack")

(assert-eq (begin
             (def f (fn (x) (let ((a (+ x 1))) a)))
             (def g (fn (x) (let ((b (* x 2))) b)))
             (let ((r1 (f 10))
                   (r2 (g 20)))
               (+ r1 r2))) 51 "test_multiple_closures_with_let_dont_interfere")

(assert-eq (begin
             (def checker (fn (s)
               (let ((result (string-contains? s "hello")))
                 result)))
             (let ((msg "say hello world"))
               (checker msg))) true "test_closure_let_with_string_operations")

# ============================================================================
# SECTION 9: `assign` form (variable mutation)
# ============================================================================

(assert-eq (let ((x 0))
             (begin (assign x 10) x)) 10 "test_set_basic")

(assert-eq (let ((x 0))
             (let ((inc (fn () (begin (assign x (+ x 1)) x))))
               (begin (inc) (inc) (inc)))) 3 "test_set_in_closure")

(assert-eq (let ((x 5))
             (begin (assign x 10) (assign x 20) (assign x 30) x)) 30 "test_set_multiple_times")

(assert-eq (let ((x 5))
             (begin (assign x (+ x 10)) x)) 15 "test_set_with_expression")

(assert-eq (let ((x 0))
             (let ((inc (fn () (assign x (+ x 1))))
                   (get (fn () x)))
               (begin (inc) (inc) (get)))) 2 "test_set_shared_capture")
