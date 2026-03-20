(elle/epoch 1)
# Tests for tail call optimization
#
# Comprehensive tests for tail call optimization including:
# - Basic tail recursion patterns
# - Block body tail calls (new for #333)
# - Break value tail calls (new for #333)
# - Deep recursion tests (prove TCO works)
# - Fiber tail position tests
# - Coroutine tail position tests


# ============================================================================
# Basic tail recursion (existing patterns)
# ============================================================================

# Accumulator-based tail recursion: sum-to
# This should be tail-optimized and handle large n without overflow
(defn sum-to [n acc]
  "Sum 1..n using tail recursion with accumulator"
  (if (= n 0)
      acc
      (sum-to (- n 1) (+ acc n))))

(assert (= (sum-to 10 0) 55) "sum-to 10")
(assert (= (sum-to 100 0) 5050) "sum-to 100")

# Simple countdown to 0
# This is a basic tail-recursive pattern
(defn countdown [n]
  "Count down from n to 0, returning n"
  (if (<= n 0)
      0
      (countdown (- n 1))))

(assert (= (countdown 5) 0) "countdown 5")
(assert (= (countdown 10) 0) "countdown 10")

# ============================================================================
# Block body tail calls (new for #333)
# ============================================================================

# Block in tail position with a call as the last expression
# The call to sum-to should be marked as tail
(defn sum-via-block [n]
  "Sum using block with tail call in body"
  (block :sum
    (sum-to n 0)))

(assert (= (sum-via-block 10) 55) "block with tail call in body")

# Nested blocks with tail calls
# Both blocks are in tail position, inner call should be tail
(defn nested-blocks [n]
  "Nested blocks with tail call"
  (block :outer
    (block :inner
      (sum-to n 0))))

(assert (= (nested-blocks 10) 55) "nested blocks with tail call")

# Block with break and tail call in body
# The last expression (call) should be tail
(defn block-with-break [n]
  "Block with break and tail call"
  (block :b
    (if (< n 0)
        (break :b -1)
        (sum-to n 0))))

(assert (= (block-with-break 10) 55) "block with break, positive n")
(assert (= (block-with-break -5) -1) "block with break, negative n")

# ============================================================================
# Break value tail calls (new for #333)
# ============================================================================

# Break value that is a call (should be tail)
# When breaking with a call, that call should be tail
(defn break-with-call [n]
  "Break with a call as the value"
  (block :b
    (if (< n 0)
        (break :b (sum-to (- n) 0))
        (sum-to n 0))))

(assert (= (break-with-call 10) 55) "break with call, positive n")
(assert (= (break-with-call -10) 55) "break with call, negative n")

# Nested breaks with tail calls
# Both the break value and the final expression should be tail
(defn nested-breaks [n]
  "Nested breaks with tail calls"
  (block :outer
    (block :inner
      (if (< n 0)
          (break :outer (sum-to (- n) 0))
          (sum-to n 0)))))

(assert (= (nested-breaks 10) 55) "nested breaks, positive n")
(assert (= (nested-breaks -10) 55) "nested breaks, negative n")

# ============================================================================
# Deep recursion tests (prove TCO works)
# ============================================================================

# Countdown to 100,000 (would overflow without TCO)
# This test proves tail call optimization is working
(defn countdown-large [n]
  "Count down from n to 0 (tail-optimized)"
  (if (<= n 0)
      :done
      (countdown-large (- n 1))))

# This would overflow without TCO
(assert (= (countdown-large 100000) :done) "countdown 100,000 (TCO required)")

# Sum to 10,000 (would overflow without TCO)
# Accumulator-based recursion at large scale
(defn sum-large [n acc]
  "Sum 1..n with large n (tail-optimized)"
  (if (= n 0)
      acc
      (sum-large (- n 1) (+ acc n))))

# This would overflow without TCO
(assert (= (sum-large 10000 0) 50005000) "sum to 10,000 (TCO required)")

# Fibonacci iterative with tail recursion
# Compute fib(n) using tail recursion with accumulators
(defn fib-iter [n a b]
  "Compute fibonacci(n) iteratively with tail recursion"
  (if (<= n 0)
      a
      (fib-iter (- n 1) b (+ a b))))

(defn fib [n]
  "Fibonacci using tail-recursive helper"
  (fib-iter n 0 1))

(assert (= (fib 0) 0) "fib(0)")
(assert (= (fib 1) 1) "fib(1)")
(assert (= (fib 5) 5) "fib(5)")
(assert (= (fib 10) 55) "fib(10)")
(assert (= (fib 20) 6765) "fib(20)")

# Large fibonacci (would overflow without TCO)
# fib(50) = 12586269025 (within i64 range)
(assert (= (fib 50) 12586269025) "fib(50) (TCO required)")

# ============================================================================
# Fiber tail position tests
# ============================================================================

# fiber/resume in tail position
# When a fiber/resume is the last expression, it should be tail
(defn fiber-tail-test [n]
  "Test fiber/resume in tail position"
  (block :b
    (let ([f (fiber/new (fn () (sum-to n 0)) 1)])
      (fiber/resume f nil))))

(assert (= (fiber-tail-test 10) 55) "fiber/resume in tail position")

# fiber/cancel in tail position
# When a fiber/cancel is the last expression, it should be tail
(defn fiber-cancel-tail-test [n]
  "Test fiber/cancel in tail position"
  (block :b
    (let ([f (fiber/new (fn () (sum-to n 0)) 1)])
      (fiber/cancel f))))

(assert (= (fiber-cancel-tail-test 10) nil) "fiber/cancel in tail position")

# ============================================================================
# Coroutine tail position tests
# ============================================================================

# yield in tail position
# When yield is the last expression, it should be tail
(defn coro-yield-tail-test [n]
  "Test yield in tail position"
  (block :b
    (let ([co (coro/new (fn () (yield (sum-to n 0))))])
      (coro/resume co))))

(assert (= (coro-yield-tail-test 10) 55) "yield in tail position")

# Multiple yields with tail calls
# Each yield should be tail when in tail position
(defn coro-multi-yield [n]
  "Test multiple yields with tail calls"
  (coro/new (fn ()
    (yield (sum-to n 0))
    (yield (sum-to (+ n 1) 0))
    (sum-to (+ n 2) 0))))

(begin
  (let ([co (coro-multi-yield 5)])
    (assert (= (coro/resume co) 15) "first yield in coro")
    (assert (= (coro/resume co) 21) "second yield in coro")
    (assert (= (coro/resume co) 28) "final value in coro")))

# ============================================================================
# Complex tail call patterns
# ============================================================================

# Mutual recursion with tail calls
(defn is-even [n]
  "Check if n is even using mutual recursion"
  (if (= n 0)
      true
      (is-odd (- n 1))))

(defn is-odd [n]
  "Check if n is odd using mutual recursion"
  (if (= n 0)
      false
      (is-even (- n 1))))

(assert (= (is-even 0) true) "is-even 0")
(assert (= (is-even 4) true) "is-even 4")
(assert (= (is-odd 0) false) "is-odd 0")
(assert (= (is-odd 5) true) "is-odd 5")

# Mutual recursion with large n (proves TCO)
(assert (= (is-even 10000) true) "is-even 10,000 (TCO required)")
(assert (= (is-odd 10001) true) "is-odd 10,001 (TCO required)")

# Tail call in conditional branches
(defn conditional-tail [n]
  "Tail calls in both branches of conditional"
  (if (< n 0)
      (countdown-large (- n))
      (countdown-large n)))

(assert (= (conditional-tail 100) :done) "conditional tail call positive")
(assert (= (conditional-tail -100) :done) "conditional tail call negative")

# Tail call in loop body (via while)
(begin
  (var result 0)
  (var i 0)
  (while (< i 10)
    (begin
      (assign result (+ result i))
      (assign i (+ i 1))))
  (assert (= result 45) "accumulation in while loop"))
