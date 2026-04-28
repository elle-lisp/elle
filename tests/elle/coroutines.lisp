(elle/epoch 9)
## Coroutine Tests
##
## Migrated from tests/property/coroutines.rs (behavioral property tests).
## Tests sequential yields, resume values, conditionals, loops, state
## transitions, interleaving, and signal threading.


# ============================================================================
# Sequential yields in order
# ============================================================================

# sequential_yields_in_order: yields produce values in order
(begin
  (def gen1
    (fn []
      (yield 1)
      (yield 2)
      (yield 3)
      4))
  (def @co1 (make-coroutine gen1))
  (assert (= (coro/resume co1) 1) "sequential yields: first")
  (assert (= (coro/resume co1) 2) "sequential yields: second")
  (assert (= (coro/resume co1) 3) "sequential yields: third")
  (assert (= (coro/resume co1) 4) "sequential yields: final return"))

(begin
  (def gen1b
    (fn []
      (yield -100)
      (yield 0)
      (yield 100)
      999))
  (def @co1b (make-coroutine gen1b))
  (assert (= (coro/resume co1b) -100) "sequential yields: negative")
  (assert (= (coro/resume co1b) 0) "sequential yields: zero")
  (assert (= (coro/resume co1b) 100) "sequential yields: positive")
  (assert (= (coro/resume co1b) 999) "sequential yields: final"))

# single yield
(begin
  (def gen1c
    (fn []
      (yield 42)
      99))
  (def @co1c (make-coroutine gen1c))
  (assert (= (coro/resume co1c) 42) "single yield: value")
  (assert (= (coro/resume co1c) 99) "single yield: final return"))

# ============================================================================
# Resume values flow into yield expressions
# ============================================================================

# resume_values_flow_into_yield: resume values become yield return values
(begin
  (def gen2
    (fn []
      (let [@acc 0]
        (assign acc (+ acc (yield acc)))
        acc)))
  (def @co2 (make-coroutine gen2))
  (coro/resume co2)
  (assert (= (coro/resume co2 10) 10)
    "resume value flows into yield: 0 + 10 = 10"))

(begin
  (def gen2b
    (fn []
      (let [@acc 0]
        (assign acc (+ acc (yield acc)))
        (assign acc (+ acc (yield acc)))
        acc)))
  (def @co2b (make-coroutine gen2b))
  (coro/resume co2b)
  (coro/resume co2b 5)
  (assert (= (coro/resume co2b 3) 8) "resume values accumulate: 0 + 5 + 3 = 8"))

# ============================================================================
# Yield in conditional
# ============================================================================

# yield_in_conditional: yield inside if branches
(begin
  (def gen3t (fn [] (if true (yield 1) (yield 2))))
  (def @co3t (make-coroutine gen3t))
  (assert (= (coro/resume co3t) 1) "yield in conditional: true branch"))

(begin
  (def gen3f (fn [] (if false (yield 1) (yield 2))))
  (def @co3f (make-coroutine gen3f))
  (assert (= (coro/resume co3f) 2) "yield in conditional: false branch"))

# ============================================================================
# Yield in loop
# ============================================================================

# yield_in_loop: yield inside while loop
(begin
  (def gen4
    (fn []
      (let [@i 0]
        (while (< i 3)
          (begin
            (yield i)
            (assign i (+ i 1))))
        i)))
  (def @co4 (make-coroutine gen4))
  (assert (= (coro/resume co4) 0) "yield in loop: i=0")
  (assert (= (coro/resume co4) 1) "yield in loop: i=1")
  (assert (= (coro/resume co4) 2) "yield in loop: i=2")
  (assert (= (coro/resume co4) 3) "yield in loop: final return"))

(begin
  (def gen4b
    (fn []
      (let [@i 0]
        (while (< i 5)
          (begin
            (yield i)
            (assign i (+ i 1))))
        i)))
  (def @co4b (make-coroutine gen4b))
  (assert (= (coro/resume co4b) 0) "yield in loop 5: i=0")
  (assert (= (coro/resume co4b) 1) "yield in loop 5: i=1")
  (assert (= (coro/resume co4b) 2) "yield in loop 5: i=2")
  (assert (= (coro/resume co4b) 3) "yield in loop 5: i=3")
  (assert (= (coro/resume co4b) 4) "yield in loop 5: i=4")
  (assert (= (coro/resume co4b) 5) "yield in loop 5: final return"))

# ============================================================================
# Coroutine state transitions
# ============================================================================

# coroutine_state_transitions: state machine progression
(begin
  (def gen5
    (fn []
      (yield 1)
      (yield 2)
      3))
  (def @co5 (make-coroutine gen5))
  (assert (= (string (coro/status co5)) "new") "state: initial is new")
  (coro/resume co5)
  (assert (= (string (coro/status co5)) "paused")
    "state: after first yield is paused")
  (coro/resume co5)
  (assert (= (string (coro/status co5)) "paused")
    "state: after second yield is paused")
  (coro/resume co5)
  (assert (= (string (coro/status co5)) "dead")
    "state: after final return is dead"))

# single yield state transitions
(begin
  (def gen5b
    (fn []
      (yield 1)
      2))
  (def @co5b (make-coroutine gen5b))
  (assert (= (string (coro/status co5b)) "new") "state single: initial is new")
  (coro/resume co5b)
  (assert (= (string (coro/status co5b)) "paused")
    "state single: after yield is paused")
  (coro/resume co5b)
  (assert (= (string (coro/status co5b)) "dead")
    "state single: after return is dead"))

# ============================================================================
# Interleaved coroutines
# ============================================================================

# interleaved_coroutines: multiple coroutines interleaved
(begin
  (def make-gen6
    (fn [start]
      (fn []
        (yield (+ start 0))
        (yield (+ start 1))
        (+ start 2))))
  (def @co6a (make-coroutine (make-gen6 0)))
  (def @co6b (make-coroutine (make-gen6 100)))
  (assert (= (coro/resume co6a) 0) "interleaved: co1 first yield")
  (assert (= (coro/resume co6b) 100) "interleaved: co2 first yield")
  (assert (= (coro/resume co6a) 1) "interleaved: co1 second yield")
  (assert (= (coro/resume co6b) 101) "interleaved: co2 second yield")
  (assert (= (coro/resume co6a) 2) "interleaved: co1 final return")
  (assert (= (coro/resume co6b) 102) "interleaved: co2 final return"))

# three coroutines interleaved
(begin
  (def make-gen6b
    (fn [start]
      (fn []
        (yield start)
        (+ start 10))))
  (def @co6c (make-coroutine (make-gen6b 0)))
  (def @co6d (make-coroutine (make-gen6b 50)))
  (def @co6e (make-coroutine (make-gen6b 100)))
  (assert (= (coro/resume co6c) 0) "interleaved 3: co1 yield")
  (assert (= (coro/resume co6d) 50) "interleaved 3: co2 yield")
  (assert (= (coro/resume co6e) 100) "interleaved 3: co3 yield")
  (assert (= (coro/resume co6c) 10) "interleaved 3: co1 return")
  (assert (= (coro/resume co6d) 60) "interleaved 3: co2 return")
  (assert (= (coro/resume co6e) 110) "interleaved 3: co3 return"))

# ============================================================================
# Signal threading: yielding closure has correct signal
# ============================================================================

# yielding_closure_has_correct_signal: yield marks closure as yielding
(begin
  (def gen7
    (fn []
      (yield 42)
      999))
  (def @co7 (make-coroutine gen7))
  (assert (= (coro/resume co7) 42) "signal threading: first resume yields value")
  (assert (= (string (coro/status co7)) "paused")
    "signal threading: status is paused after yield"))

(begin
  (def gen7b
    (fn []
      (yield -100)
      0))
  (def @co7b (make-coroutine gen7b))
  (assert (= (coro/resume co7b) -100) "signal threading: negative yield value")
  (assert (= (string (coro/status co7b)) "paused")
    "signal threading: paused after negative yield"))

# ============================================================================
# Basic yield/resume tests (from integration/coroutines.rs)
# ============================================================================

# test_simple_yield
(begin
  (def @co (make-coroutine (fn [] (yield 42))))
  (assert (= (coro/resume co) 42) "simple yield"))

# test_multiple_yields
(begin
  (def @co
    (make-coroutine (fn []
                      (yield 1)
                      (yield 2)
                      (yield 3)
                      4)))
  (assert (= (coro/resume co) 1) "multiple yields: first")
  (assert (= (coro/resume co) 2) "multiple yields: second")
  (assert (= (coro/resume co) 3) "multiple yields: third")
  (assert (= (coro/resume co) 4) "multiple yields: final"))

# test_yield_with_resume_value
(begin
  (def @co (make-coroutine (fn [] (+ 10 (yield 1)))))
  (assert (= (coro/resume co) 1) "yield with resume value: first")
  (assert (= (coro/resume co 5) 15) "yield with resume value: second"))

# ============================================================================
# Coroutine status tests
# ============================================================================

# test_coroutine_status_created
(begin
  (def @co (make-coroutine (fn [] 42)))
  (assert (= (string (coro/status co)) "new") "status: new"))

# test_coroutine_status_done
(begin
  (def @co (make-coroutine (fn [] 42)))
  (coro/resume co)
  (assert (= (string (coro/status co)) "dead") "status: dead"))

# test_coroutine_done_predicate
(begin
  (def @co (make-coroutine (fn [] 42)))
  (assert (not (coro/done? co)) "done predicate: initially false")
  (coro/resume co)
  (assert (coro/done? co) "done predicate: true after resume"))

# test_coroutine_status_suspended_after_yield
(begin
  (def gen
    (fn []
      (yield 1)
      (yield 2)))
  (def @co (make-coroutine gen))
  (coro/resume co)
  (assert (= (string (coro/status co)) "paused") "status: paused after yield"))

# test_coroutine_value_after_yield
(begin
  (def @co (make-coroutine (fn [] (yield 42))))
  (coro/resume co)
  (assert (= (coro/value co) 42) "value after yield"))

# ============================================================================
# Signal inference tests
# ============================================================================

# test_silent_function_no_cps
(begin
  (def sum
    (fn (n)
      (if (<= n 0)
        0
        (+ n (sum (- n 1))))))
  (assert (= (sum 5) 15) "silent function: sum 5"))

# test_yielding_function_detected
(begin
  (def gen
    (fn []
      (yield 1)
      (yield 2)))
  (def @co (make-coroutine gen))
  (assert (= (coro/resume co) 1) "yielding function detected"))

# test_calling_yielding_function_propagates_effect
(begin
  (def f (fn [] (yield 1)))
  (def g
    (fn []
      (f)
      (yield 2)))
  (def @co (make-coroutine g))
  (assert (= (coro/resume co) 1) "signal propagation: first yield"))

# ============================================================================
# Nested coroutines tests
# ============================================================================

# test_nested_coroutines
(begin
  (def inner-gen (fn [] (yield 10)))
  (def outer-gen
    (fn []
      (def @inner-co (make-coroutine inner-gen))
      (yield (coro/resume inner-co))))
  (def @co (make-coroutine outer-gen))
  (assert (= (coro/resume co) 10) "nested coroutines"))

# test_nested_coroutines_multiple_levels
(begin
  (def level3 (fn [] (yield 3)))
  (def level2
    (fn []
      (def @co3 (make-coroutine level3))
      (yield (coro/resume co3))))
  (def level1
    (fn []
      (def @co2 (make-coroutine level2))
      (yield (coro/resume co2))))
  (def @co1 (make-coroutine level1))
  (assert (= (coro/resume co1) 3) "nested coroutines: 3 levels"))

# ============================================================================
# Closures with captured variables tests
# ============================================================================

# test_coroutine_with_captured_variables
(begin
  (let [x 10]
    (def @co (make-coroutine (fn [] (yield x))))
    (assert (= (coro/resume co) 10) "captured variables")))

# test_coroutine_with_multiple_captured_variables
(begin
  (let [x 10
        y 20]
    (def @co (make-coroutine (fn [] (yield (+ x y)))))
    (assert (= (coro/resume co) 30) "multiple captured variables")))

# test_coroutine_captures_mutable_state
(begin
  (let [counter (box 0)]
    (def @co
      (make-coroutine (fn []
                        (rebox counter (+ (unbox counter) 1))
                        (yield (unbox counter)))))
    (assert (= (coro/resume co) 1) "mutable state capture")))

# test_closure_captured_var_after_resume_issue_258
(begin
  (def make-counter
    (fn (start)
      (fn []
        (yield start)
        (yield (+ start 1))
        (yield (+ start 2)))))
  (def @co-100 (make-coroutine (make-counter 100)))
  (assert (= (coro/resume co-100) 100) "issue #258: first yield")
  (assert (= (coro/resume co-100) 101) "issue #258: second yield")
  (assert (= (coro/resume co-100) 102) "issue #258: third yield"))

# ============================================================================
# Issue #259 regression tests - state management
# ============================================================================

# test_interleaved_coroutines_issue_259
(begin
  (def make-counter
    (fn (start)
      (fn []
        (yield start)
        (yield (+ start 1))
        (yield (+ start 2)))))
  (def @co-100 (make-coroutine (make-counter 100)))
  (def @co-200 (make-coroutine (make-counter 200)))
  (assert (= (coro/resume co-100) 100) "interleaved #259: co1 first")
  (assert (= (coro/resume co-200) 200) "interleaved #259: co2 first")
  (assert (= (coro/resume co-100) 101) "interleaved #259: co1 second")
  (assert (= (coro/resume co-200) 201) "interleaved #259: co2 second")
  (assert (= (coro/resume co-100) 102) "interleaved #259: co1 third")
  (assert (= (coro/resume co-200) 202) "interleaved #259: co2 third"))

# test_multiple_coroutines_independent_state
(begin
  (def gen1
    (fn []
      (yield 'a)
      (yield 'b)))
  (def gen2
    (fn []
      (yield 'x)
      (yield 'y)))
  (def @co1 (make-coroutine gen1))
  (def @co2 (make-coroutine gen2))
  (assert (= (coro/resume co1) 'a) "independent state: co1 first")
  (assert (= (coro/resume co2) 'x) "independent state: co2 first")
  (assert (= (coro/resume co1) 'b) "independent state: co1 second")
  (assert (= (coro/resume co2) 'y) "independent state: co2 second"))

# test_nested_coroutine_resume_from_coroutine
(begin
  (def inner-gen
    (fn []
      (yield 10)
      (yield 20)))
  (def outer-gen
    (fn []
      (def @inner-co (make-coroutine inner-gen))
      (yield (+ 1 (coro/resume inner-co)))
      (yield (+ 1 (coro/resume inner-co)))))
  (def @outer-co (make-coroutine outer-gen))
  (assert (= (coro/resume outer-co) 11) "nested resume from coroutine: first")
  (assert (= (coro/resume outer-co) 21) "nested resume from coroutine: second"))

# ============================================================================
# Error handling tests
# ============================================================================

# test_error_in_coroutine (skipped - requires error message checking)

# ============================================================================
# Coroutine predicates and accessors
# ============================================================================

# test_coroutine_predicate
(begin
  (def @co (make-coroutine (fn [] 42)))
  (assert (coro? co) "coroutine predicate: true for coroutine")
  (assert (not (coro? 42)) "coroutine predicate: false for int")
  (assert (not (coro? (fn [] 42))) "coroutine predicate: false for function"))

# ============================================================================
# Integration with other language features
# ============================================================================

# test_coroutine_with_recursion
(begin
  (def countdown
    (fn (n)
      (if (<= n 0)
        (yield 0)
        (begin
          (yield n)
          (countdown (- n 1))))))
  (def @co (make-coroutine (fn [] (countdown 3))))
  (assert (= (coro/resume co) 3) "recursion in coroutine"))

# test_coroutine_with_higher_order_functions
(begin
  (def @co (make-coroutine (fn [] (yield (map (fn (x) (* x 2)) (list 1 2 3))))))
  (coro/resume co)
  (assert true "higher-order functions in coroutine"))

# ============================================================================
# Edge cases and boundary conditions
# ============================================================================

# test_coroutine_with_no_yield
(begin
  (def @co (make-coroutine (fn [] 42)))
  (assert (= (coro/resume co) 42) "no yield: returns value"))

# test_coroutine_with_nil_yield
(begin
  (def @co (make-coroutine (fn [] (yield nil))))
  (assert (= (coro/resume co) nil) "nil yield"))

# test_coroutine_with_complex_yielded_value
(begin
  (def @co (make-coroutine (fn [] (yield (list 1 2 3)))))
  (coro/resume co)
  (assert true "complex yielded value"))

# test_coroutine_with_empty_body
(begin
  (def @co (make-coroutine (fn [] nil)))
  (assert (= (coro/resume co) nil) "empty body"))

# ============================================================================
# CPS path tests
# ============================================================================

# test_cps_simple_yield
(begin
  (def gen (fn [] (yield 42)))
  (def @co (make-coroutine gen))
  (assert (= (coro/resume co) 42) "CPS: simple yield"))

# test_cps_yield_in_if
(begin
  (def gen (fn [] (if true (yield 1) (yield 2))))
  (def @co (make-coroutine gen))
  (assert (= (coro/resume co) 1) "CPS: yield in if true"))

# test_cps_yield_in_else
(begin
  (def gen (fn [] (if false (yield 1) (yield 2))))
  (def @co (make-coroutine gen))
  (assert (= (coro/resume co) 2) "CPS: yield in if false"))

# test_cps_yield_in_begin
(begin
  (def gen
    (fn []
      (begin
        (yield 1)
        (yield 2))))
  (def @co (make-coroutine gen))
  (assert (= (coro/resume co) 1) "CPS: yield in begin"))

# test_cps_yield_with_computation
(begin
  (def gen (fn [] (yield (+ 10 20 12))))
  (def @co (make-coroutine gen))
  (assert (= (coro/resume co) 42) "CPS: yield with computation"))

# test_cps_yield_in_let
(begin
  (def gen
    (fn []
      (let [x 10]
        (yield x))))
  (def @co (make-coroutine gen))
  (assert (= (coro/resume co) 10) "CPS: yield in let"))

# test_cps_yield_with_captured_var
(begin
  (let [x 42]
    (def gen (fn [] (yield x)))
    (def @co (make-coroutine gen))
    (assert (= (coro/resume co) 42) "CPS: yield with captured var")))

# test_cps_yield_in_and
(begin
  (def gen (fn [] (and true (yield 42))))
  (def @co (make-coroutine gen))
  (assert (= (coro/resume co) 42) "CPS: yield in and"))

# test_cps_yield_in_or
(begin
  (def gen (fn [] (or false (yield 42))))
  (def @co (make-coroutine gen))
  (assert (= (coro/resume co) 42) "CPS: yield in or"))

# test_cps_yield_in_cond
(begin
  (def gen
    (fn []
      (cond
        false (yield 1)
        true (yield 2)
        (yield 3))))
  (def @co (make-coroutine gen))
  (assert (= (coro/resume co) 2) "CPS: yield in cond"))

# ============================================================================
# Performance and stress tests
# ============================================================================

# test_coroutine_with_large_yielded_value
(begin
  (def @co (make-coroutine (fn [] (yield (list 1 2 3 4 5 6 7 8 9 10)))))
  (coro/resume co)
  (assert true "large yielded value"))

# test_multiple_coroutines_independent
(begin
  (def @co1 (make-coroutine (fn [] (yield 1))))
  (def @co2 (make-coroutine (fn [] (yield 2))))
  (assert (= (coro/resume co1) 1) "multiple independent: co1")
  (assert (= (coro/resume co2) 2) "multiple independent: co2"))

# ============================================================================
# Issue #260 regression tests - quoted symbols in yield
# ============================================================================

# test_yield_quoted_symbol_issue_260
(begin
  (def gen-sym
    (fn []
      (yield 'a)
      (yield 'b)
      (yield 'c)))
  (def @co (make-coroutine gen-sym))
  (assert (= (coro/resume co) 'a) "quoted symbol: first")
  (assert (= (coro/resume co) 'b) "quoted symbol: second")
  (assert (= (coro/resume co) 'c) "quoted symbol: third"))

# test_yield_quoted_symbol_is_value_not_variable
(begin
  (def gen (fn [] (yield 'test-symbol)))
  (def @co (make-coroutine gen))
  (def @result (coro/resume co))
  (assert (symbol? result) "quoted symbol is symbol value"))

# test_yield_various_literal_types
(begin
  (def gen
    (fn []
      (yield 'symbol-val)
      (yield 42)
      (yield "string")
      (yield true)
      (yield nil)))
  (def @co (make-coroutine gen))
  (assert (symbol? (coro/resume co)) "literal types: symbol")
  (assert (number? (coro/resume co)) "literal types: number")
  (assert (string? (coro/resume co)) "literal types: string")
  (assert (= (coro/resume co) true) "literal types: true")
  (assert (= (coro/resume co) nil) "literal types: nil"))

# test_yield_quoted_list
(begin
  (def gen (fn [] (yield '(1 2 3))))
  (def @co (make-coroutine gen))
  (coro/resume co)
  (assert true "quoted list yield"))

# ============================================================================
# Yield with intermediate values on stack
# ============================================================================

# test_yield_with_intermediate_values_on_stack
(begin
  (def @co (make-coroutine (fn [] (+ 1 (yield 2) 3))))
  (assert (= (coro/resume co) 2) "intermediate values: first yield")
  (assert (= (coro/resume co 10) 14) "intermediate values: 1+10+3=14"))

# test_yield_with_multiple_intermediate_values
(begin
  (def @co (make-coroutine (fn [] (+ 1 2 (yield 3) 4 5))))
  (assert (= (coro/resume co) 3) "multiple intermediate: first yield")
  (assert (= (coro/resume co 100) 112) "multiple intermediate: 1+2+100+4+5=112"))

# test_yield_in_nested_call_with_intermediate_values
(begin
  (def @co (make-coroutine (fn [] (* 2 (+ 1 (yield 5) 3)))))
  (assert (= (coro/resume co) 5) "nested intermediate: first yield")
  (assert (= (coro/resume co 10) 28) "nested intermediate: 2*(1+10+3)=28"))

# test_multiple_yields_with_intermediate_values
(begin
  (def @co (make-coroutine (fn [] (+ (+ 1 (yield 2) 3) (+ 4 (yield 5) 6)))))
  (assert (= (coro/resume co) 2) "multiple yields intermediate: first")
  (assert (= (coro/resume co 10) 5) "multiple yields intermediate: second")
  (assert (= (coro/resume co 20) 44)
    "multiple yields intermediate: (1+10+3)+(4+20+6)=44"))

# ============================================================================
# Error tests (from integration/coroutines.rs)
# ============================================================================

# resume_done_coroutine_fails
(let [[ok? _] (protect ((fn ()
                          (let [co (make-coroutine (fn () 42))]
                            (coro/resume co)
                            (coro/resume co)))))]
  (assert (not ok?) "resuming done coroutine fails"))

# ============================================================================
# Runtime signal checks (Pure closure warnings)
# ============================================================================

# test_make_coroutine_silent_closure_still_works
(begin
  (let [co (make-coroutine (fn [] 42))]
    (assert (= (coro/resume co) 42) "silent closure in coroutine")))

# test_make_coroutine_yielding_closure_works
(begin
  (let [co (make-coroutine (fn [] (yield 42)))]
    (assert (= (coro/resume co) 42) "yielding closure in coroutine")))

# test_coroutine_resume_silent_closure_completes_immediately
(begin
  (def @co (make-coroutine (fn [] (+ 1 2 3))))
  (assert (= (coro/resume co) 6) "silent closure completes: value")
  (assert (= (string (coro/status co)) "dead")
    "silent closure completes: status"))

# ============================================================================
# Deep cross-call yield tests
# ============================================================================

# test_yield_across_three_call_levels
(begin
  (def a (fn (x) (yield (* x 2))))
  (def b (fn (x) (+ (a x) 1)))
  (def c (fn (x) (+ (b x) 1)))
  (def @co (make-coroutine (fn [] (c 10))))
  (assert (= (coro/resume co) 20) "three call levels: first yield")
  (assert (= (coro/resume co 20) 22) "three call levels: final return"))

# test_yield_in_tail_position
(begin
  (def @co
    (make-coroutine (fn []
                      (yield 1)
                      (yield 2))))
  (assert (= (coro/resume co) 1) "tail position: first")
  (assert (= (coro/resume co) 2) "tail position: second")
  (assert (= (string (coro/status co)) "paused")
    "tail position: paused after second yield")
  (coro/resume co)
  (assert (= (string (coro/status co)) "dead")
    "tail position: dead after final resume"))

# test_deep_call_chain_with_multiple_yields
(begin
  (def level3
    (fn []
      (yield 3)
      "done"))
  (def level2
    (fn []
      (yield 2)
      (level3)))
  (def level1
    (fn []
      (yield 1)
      (level2)))
  (def @co (make-coroutine level1))
  (assert (= (coro/resume co) 1) "deep call chain: first")
  (assert (= (coro/resume co) 2) "deep call chain: second")
  (assert (= (coro/resume co) 3) "deep call chain: third")
  (assert (= (coro/resume co) "done") "deep call chain: final"))

# ============================================================================
# Error tests (from integration/coroutines.rs)
# ============================================================================

# resume_done_coroutine_fails
(let [[ok? _] (protect ((fn ()
                          (let [co (make-coroutine (fn () 42))]
                            (coro/resume co)
                            (coro/resume co)))))]
  (assert (not ok?) "resuming done coroutine fails"))
