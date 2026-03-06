## Coroutine Tests
##
## Migrated from tests/property/coroutines.rs (behavioral property tests).
## Tests sequential yields, resume values, conditionals, loops, state
## transitions, interleaving, and effect threading.

(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false :assert-list-eq assert-list-eq :assert-equal assert-equal :assert-not-nil assert-not-nil :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind} ((import-file "tests/elle/assert.lisp")))

# ============================================================================
# Sequential yields in order
# ============================================================================

# sequential_yields_in_order: yields produce values in order
(begin
  (def gen1 (fn [] (yield 1) (yield 2) (yield 3) 4))
  (var co1 (make-coroutine gen1))
  (assert-eq (coro/resume co1) 1 "sequential yields: first")
  (assert-eq (coro/resume co1) 2 "sequential yields: second")
  (assert-eq (coro/resume co1) 3 "sequential yields: third")
  (assert-eq (coro/resume co1) 4 "sequential yields: final return"))

(begin
  (def gen1b (fn [] (yield -100) (yield 0) (yield 100) 999))
  (var co1b (make-coroutine gen1b))
  (assert-eq (coro/resume co1b) -100 "sequential yields: negative")
  (assert-eq (coro/resume co1b) 0 "sequential yields: zero")
  (assert-eq (coro/resume co1b) 100 "sequential yields: positive")
  (assert-eq (coro/resume co1b) 999 "sequential yields: final"))

# single yield
(begin
  (def gen1c (fn [] (yield 42) 99))
  (var co1c (make-coroutine gen1c))
  (assert-eq (coro/resume co1c) 42 "single yield: value")
  (assert-eq (coro/resume co1c) 99 "single yield: final return"))

# ============================================================================
# Resume values flow into yield expressions
# ============================================================================

# resume_values_flow_into_yield: resume values become yield return values
(begin
  (def gen2 (fn []
    (let ([acc 0])
      (set acc (+ acc (yield acc)))
      acc)))
  (var co2 (make-coroutine gen2))
  (coro/resume co2)
  (assert-eq (coro/resume co2 10) 10
    "resume value flows into yield: 0 + 10 = 10"))

(begin
  (def gen2b (fn []
    (let ([acc 0])
      (set acc (+ acc (yield acc)))
      (set acc (+ acc (yield acc)))
      acc)))
  (var co2b (make-coroutine gen2b))
  (coro/resume co2b)
  (coro/resume co2b 5)
  (assert-eq (coro/resume co2b 3) 8
    "resume values accumulate: 0 + 5 + 3 = 8"))

# ============================================================================
# Yield in conditional
# ============================================================================

# yield_in_conditional: yield inside if branches
(begin
  (def gen3t (fn [] (if true (yield 1) (yield 2))))
  (var co3t (make-coroutine gen3t))
  (assert-eq (coro/resume co3t) 1
    "yield in conditional: true branch"))

(begin
  (def gen3f (fn [] (if false (yield 1) (yield 2))))
  (var co3f (make-coroutine gen3f))
  (assert-eq (coro/resume co3f) 2
    "yield in conditional: false branch"))

# ============================================================================
# Yield in loop
# ============================================================================

# yield_in_loop: yield inside while loop
(begin
  (def gen4 (fn []
    (let ([i 0])
      (while (< i 3)
        (begin
          (yield i)
          (set i (+ i 1))))
      i)))
  (var co4 (make-coroutine gen4))
  (assert-eq (coro/resume co4) 0 "yield in loop: i=0")
  (assert-eq (coro/resume co4) 1 "yield in loop: i=1")
  (assert-eq (coro/resume co4) 2 "yield in loop: i=2")
  (assert-eq (coro/resume co4) 3 "yield in loop: final return"))

(begin
  (def gen4b (fn []
    (let ([i 0])
      (while (< i 5)
        (begin
          (yield i)
          (set i (+ i 1))))
      i)))
  (var co4b (make-coroutine gen4b))
  (assert-eq (coro/resume co4b) 0 "yield in loop 5: i=0")
  (assert-eq (coro/resume co4b) 1 "yield in loop 5: i=1")
  (assert-eq (coro/resume co4b) 2 "yield in loop 5: i=2")
  (assert-eq (coro/resume co4b) 3 "yield in loop 5: i=3")
  (assert-eq (coro/resume co4b) 4 "yield in loop 5: i=4")
  (assert-eq (coro/resume co4b) 5 "yield in loop 5: final return"))

# ============================================================================
# Coroutine state transitions
# ============================================================================

# coroutine_state_transitions: state machine progression
(begin
  (def gen5 (fn [] (yield 1) (yield 2) 3))
  (var co5 (make-coroutine gen5))
  (assert-eq (keyword->string (coro/status co5)) "created"
    "state: initial is created")
  (coro/resume co5)
  (assert-eq (keyword->string (coro/status co5)) "suspended"
    "state: after first yield is suspended")
  (coro/resume co5)
  (assert-eq (keyword->string (coro/status co5)) "suspended"
    "state: after second yield is suspended")
  (coro/resume co5)
  (assert-eq (keyword->string (coro/status co5)) "done"
    "state: after final return is done"))

# single yield state transitions
(begin
  (def gen5b (fn [] (yield 1) 2))
  (var co5b (make-coroutine gen5b))
  (assert-eq (keyword->string (coro/status co5b)) "created"
    "state single: initial is created")
  (coro/resume co5b)
  (assert-eq (keyword->string (coro/status co5b)) "suspended"
    "state single: after yield is suspended")
  (coro/resume co5b)
  (assert-eq (keyword->string (coro/status co5b)) "done"
    "state single: after return is done"))

# ============================================================================
# Interleaved coroutines
# ============================================================================

# interleaved_coroutines: multiple coroutines interleaved
(begin
  (def make-gen6 (fn [start] (fn [] (yield (+ start 0)) (yield (+ start 1)) (+ start 2))))
  (var co6a (make-coroutine (make-gen6 0)))
  (var co6b (make-coroutine (make-gen6 100)))
  (assert-eq (coro/resume co6a) 0 "interleaved: co1 first yield")
  (assert-eq (coro/resume co6b) 100 "interleaved: co2 first yield")
  (assert-eq (coro/resume co6a) 1 "interleaved: co1 second yield")
  (assert-eq (coro/resume co6b) 101 "interleaved: co2 second yield")
  (assert-eq (coro/resume co6a) 2 "interleaved: co1 final return")
  (assert-eq (coro/resume co6b) 102 "interleaved: co2 final return"))

# three coroutines interleaved
(begin
  (def make-gen6b (fn [start] (fn [] (yield start) (+ start 10))))
  (var co6c (make-coroutine (make-gen6b 0)))
  (var co6d (make-coroutine (make-gen6b 50)))
  (var co6e (make-coroutine (make-gen6b 100)))
  (assert-eq (coro/resume co6c) 0 "interleaved 3: co1 yield")
  (assert-eq (coro/resume co6d) 50 "interleaved 3: co2 yield")
  (assert-eq (coro/resume co6e) 100 "interleaved 3: co3 yield")
  (assert-eq (coro/resume co6c) 10 "interleaved 3: co1 return")
  (assert-eq (coro/resume co6d) 60 "interleaved 3: co2 return")
  (assert-eq (coro/resume co6e) 110 "interleaved 3: co3 return"))

# ============================================================================
# Effect threading: yielding closure has correct effect
# ============================================================================

# yielding_closure_has_correct_effect: yield marks closure as yielding
(begin
  (def gen7 (fn [] (yield 42) 999))
  (var co7 (make-coroutine gen7))
  (assert-eq (coro/resume co7) 42
    "effect threading: first resume yields value")
  (assert-eq (keyword->string (coro/status co7)) "suspended"
    "effect threading: status is suspended after yield"))

(begin
  (def gen7b (fn [] (yield -100) 0))
  (var co7b (make-coroutine gen7b))
  (assert-eq (coro/resume co7b) -100
    "effect threading: negative yield value")
  (assert-eq (keyword->string (coro/status co7b)) "suspended"
    "effect threading: suspended after negative yield"))
