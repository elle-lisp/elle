## Coroutine Example Tests
##
## Migrated from tests/property/coroutines.rs (example-based #[test] functions).
## Property-based tests remain in Rust.

(import-file "./examples/assertions.lisp")

# ============================================================================
# Yield across call boundaries
# ============================================================================

(let* ([helper (fn [x] (yield (* x 2)))]
       [gen (fn [] (helper 21))]
       [co (make-coroutine gen)])
  (assert-eq (coro/resume co) 42 "yield across call: helper yields doubled value"))

# ============================================================================
# Yield across two call levels
# ============================================================================

(let* ([inner (fn [x] (yield (* x 3)))]
       [outer (fn [x] (inner (+ x 1)))]
       [gen (fn [] (outer 10))]
       [co (make-coroutine gen)])
  # (outer 10) -> (inner 11) -> (yield 33)
  (assert-eq (coro/resume co) 33 "yield across two levels: inner yields tripled"))

# ============================================================================
# Yield across call then resume then yield
# ============================================================================

(let* ([helper (fn [x]
                 (let ([first (yield x)])
                   (yield (+ first x))))]
       [gen (fn [] (helper 10))]
       [co (make-coroutine gen)])
  (assert-eq (coro/resume co) 10 "yield-resume-yield: first yield is 10")
  (assert-eq (coro/resume co 5) 15 "yield-resume-yield: second yield is 5+10=15"))

# ============================================================================
# Yield across call with return value
# ============================================================================

(let* ([helper (fn [x]
                 (yield x)
                 (* x 2))]
       [gen (fn []
              (let ([result (helper 5)])
                (+ result 100)))]
       [co (make-coroutine gen)])
  (assert-eq (coro/resume co) 5 "yield-return: first resume yields 5")
  (assert-eq (coro/resume co) 110 "yield-return: second resume returns 110")
  (assert-eq (keyword->string (coro/status co)) "done" "yield-return: status is done"))

# ============================================================================
# Coroutine that never yields
# ============================================================================

(let ([co (make-coroutine (fn [] (+ 1 2 3)))])
  (assert-eq (coro/resume co) 6 "pure function as coroutine returns 6"))

# ============================================================================
# Mutable local preserved across resume
# ============================================================================

(let* ([gen (fn []
              (let ([x 0])
                (set x 10)
                (yield x)
                (set x (+ x 5))
                (yield x)
                x))]
       [co (make-coroutine gen)])
  (assert-eq (coro/resume co) 10 "mutable local: first yield is 10")
  (assert-eq (coro/resume co) 15 "mutable local: second yield is 15")
  (assert-eq (coro/resume co) 15 "mutable local: final return is 15"))

# ============================================================================
# Effect threading: yielding closure has correct effect
# ============================================================================

(let* ([gen (fn [] (yield 42) (yield 43) 44)]
       [co (make-coroutine gen)])
  (assert-eq (keyword->string (coro/status co)) "created"
             "effect threading: initial status is created"))
