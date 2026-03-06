## Fiber Primitive Tests
##
## Migrated from tests/property/fibers.rs (behavioral property tests).
## Tests yield/resume order, signal masks, cancel, propagate, and nesting.

(import-file "./examples/assertions.lisp")

# ============================================================================
# Yield/resume order
# ============================================================================

# fiber_yield_resume_order: yields produce values in order
(let ([f (fiber/new (fn [] (fiber/signal 2 1) (fiber/signal 2 2) 3) 2)])
  (assert-eq (fiber/resume f) 1 "yield order: first")
  (assert-eq (fiber/resume f) 2 "yield order: second")
  (assert-eq (fiber/resume f) 3 "yield order: final return"))

(let ([f (fiber/new (fn [] (fiber/signal 2 -50) (fiber/signal 2 0) (fiber/signal 2 50) 999) 2)])
  (assert-eq (fiber/resume f) -50 "yield order: negative")
  (assert-eq (fiber/resume f) 0 "yield order: zero")
  (assert-eq (fiber/resume f) 50 "yield order: positive")
  (assert-eq (fiber/resume f) 999 "yield order: final"))

# ============================================================================
# Signal mask catch behavior
# ============================================================================

# signal_mask_catch_behavior: mask determines whether signal is caught
# mask=2 catches SIG_YIELD (bit 2)
(let ([f (fiber/new (fn [] (fiber/signal 2 42)) 2)])
  (assert-eq (fiber/resume f) 42
    "signal mask: yield caught by mask=2"))

# mask=0 does not catch SIG_YIELD — signal propagates to parent.
# When the parent catches it (mask=2), the child suspends rather than errors.
# We verify the propagation by observing the wrapper catches the yield value.
(let ([f (fiber/new (fn [] (fiber/signal 2 42)) 0)])
  (let ([wrapper (fiber/new (fn [] (fiber/resume f)) 2)])
    (assert-eq (fiber/resume wrapper) 42
      "signal mask: uncaught yield propagates to parent")))

# mask=1 catches SIG_ERROR (bit 1)
(let ([f (fiber/new (fn [] (fiber/signal 1 99)) 1)])
  (assert-eq (fiber/resume f) 99
    "signal mask: error caught by mask=1"))

# mask=0 does not catch SIG_ERROR — signal propagates to parent.
# We verify by wrapping in a fiber with mask=1 that catches the error.
(let ([f (fiber/new (fn [] (fiber/signal 1 99)) 0)])
  (let ([wrapper (fiber/new (fn [] (fiber/resume f)) 1)])
    (assert-eq (fiber/resume wrapper) 99
      "signal mask: uncaught error propagates to parent")))

# mask=3 catches both SIG_ERROR and SIG_YIELD
(let ([f (fiber/new (fn [] (fiber/signal 2 77)) 3)])
  (assert-eq (fiber/resume f) 77
    "signal mask: yield caught by mask=3"))
(let ([f (fiber/new (fn [] (fiber/signal 1 88)) 3)])
  (assert-eq (fiber/resume f) 88
    "signal mask: error caught by mask=3"))

# ============================================================================
# Cancel delivers value
# ============================================================================

# cancel_delivers_value_to_new_fiber: cancel a new fiber
(let ([f (fiber/new (fn [] 42) 1)])
  (let ([result (fiber/cancel f 99)])
    (assert-eq result 99 "cancel new fiber: result is payload")
    (assert-eq (fiber/value f) 99 "cancel new fiber: fiber/value is payload")))

(let ([f (fiber/new (fn [] 42) 1)])
  (let ([result (fiber/cancel f -50)])
    (assert-eq result -50 "cancel new fiber: negative payload")
    (assert-eq (fiber/value f) -50 "cancel new fiber: fiber/value negative")))

# cancel_delivers_value_to_suspended_fiber: cancel a suspended fiber
(let ([f (fiber/new (fn [] (fiber/signal 2 0) 99) 3)])
  (fiber/resume f)
  (let ([result (fiber/cancel f 88)])
    (assert-eq result 88 "cancel suspended fiber: result is payload")
    (assert-eq (fiber/value f) 88 "cancel suspended fiber: fiber/value is payload")))

(let ([f (fiber/new (fn [] (fiber/signal 2 0) 99) 3)])
  (fiber/resume f)
  (let ([result (fiber/cancel f -25)])
    (assert-eq result -25 "cancel suspended fiber: negative payload")
    (assert-eq (fiber/value f) -25 "cancel suspended fiber: fiber/value negative")))

# ============================================================================
# Propagate valid/invalid boundary
# ============================================================================

# propagate_rejects_dead_fibers: propagate fails for completed fibers
(assert-err (fn []
  (let ([f (fiber/new (fn [] 42) 0)])
    (fiber/resume f)
    (fiber/propagate f)))
  "propagate rejects dead fiber (42)")

(assert-err (fn []
  (let ([f (fiber/new (fn [] -100) 0)])
    (fiber/resume f)
    (fiber/propagate f)))
  "propagate rejects dead fiber (-100)")

# propagate_succeeds_for_errored_fibers: propagate re-raises error
(assert-err (fn []
  (let ([f (fiber/new (fn [] (fiber/signal 1 99)) 1)])
    (fiber/resume f)
    (fiber/propagate f)))
  "propagate re-raises error (99)")

(assert-err (fn []
  (let ([f (fiber/new (fn [] (fiber/signal 1 -50)) 1)])
    (fiber/resume f)
    (fiber/propagate f)))
  "propagate re-raises error (-50)")

# ============================================================================
# Cancel rejects invalid states
# ============================================================================

# cancel_rejects_dead_fibers: cancel fails for completed fibers
(assert-err (fn []
  (let ([f (fiber/new (fn [] 42) 0)])
    (fiber/resume f)
    (fiber/cancel f "too late")))
  "cancel rejects dead fiber (42)")

(assert-err (fn []
  (let ([f (fiber/new (fn [] -100) 0)])
    (fiber/resume f)
    (fiber/cancel f "too late")))
  "cancel rejects dead fiber (-100)")

# cancel_accepts_suspended_after_caught_error: cancel works on suspended fiber
(let ([f (fiber/new (fn [] (fiber/signal 1 99)) 1)])
  (fiber/resume f)
  (fiber/cancel f "cancelling suspended")
  (assert-eq (keyword->string (fiber/status f)) "error"
    "cancelled suspended fiber is in error status"))

# cancel_rejects_errored_fibers: cancel fails for errored fibers
(assert-err (fn []
  (let ([f (fiber/new (fn [] (fiber/signal 1 99)) 0)])
    (let ([wrapper (fiber/new (fn [] (fiber/resume f)) 1)])
      (fiber/resume wrapper)
      (fiber/cancel f "already errored"))))
  "cancel rejects errored fiber")

# ============================================================================
# Nested fiber resume preserves values
# ============================================================================

# nested_fiber_resume_preserves_values: A resumes B, gets B's yield value
(let ([inner (fiber/new (fn [] (fiber/signal 2 10)) 2)])
  (let ([outer (fiber/new (fn [] (+ (fiber/resume inner) 5)) 0)])
    (assert-eq (fiber/resume outer) 15
      "nested resume: 10 + 5 = 15")))

(let ([inner (fiber/new (fn [] (fiber/signal 2 -30)) 2)])
  (let ([outer (fiber/new (fn [] (+ (fiber/resume inner) 20)) 0)])
    (assert-eq (fiber/resume outer) -10
      "nested resume: -30 + 20 = -10")))

# ============================================================================
# Multi-frame yield chain
# ============================================================================

# multi_frame_yield_chain: yield propagates through call chain
(begin
  (def helper (fn [x] (yield (* x 2))))
  (def caller (fn [x] (+ (helper x) 1)))
  (var co (make-coroutine (fn [] (caller 5))))
  (assert-eq (coro/resume co) 10 "multi-frame yield: first yield is 5*2=10")
  (assert-eq (coro/resume co 7) 8 "multi-frame yield: resume 7, caller adds 1 = 8"))

(begin
  (def helper2 (fn [x] (yield (* x 2))))
  (def caller2 (fn [x] (+ (helper2 x) 1)))
  (var co2 (make-coroutine (fn [] (caller2 -25))))
  (assert-eq (coro/resume co2) -50 "multi-frame yield: first yield is -25*2=-50")
  (assert-eq (coro/resume co2 10) 11 "multi-frame yield: resume 10, caller adds 1 = 11"))

# ============================================================================
# Re-yield at different depth
# ============================================================================

# re_yield_at_different_depth: yield from helper, then yield from gen
(begin
  (def rh (fn [x] (yield x)))
  (def rgen (fn []
    (rh 10)
    (yield 20)
    42))
  (var rco (make-coroutine rgen))
  (assert-eq (coro/resume rco) 10 "re-yield: first yield from helper")
  (assert-eq (coro/resume rco 5) 20 "re-yield: second yield from gen")
  (assert-eq (coro/resume rco) 42 "re-yield: final return"))

(begin
  (def rh2 (fn [x] (yield x)))
  (def rgen2 (fn []
    (rh2 -30)
    (yield 50)
    99))
  (var rco2 (make-coroutine rgen2))
  (assert-eq (coro/resume rco2) -30 "re-yield: first yield -30")
  (assert-eq (coro/resume rco2 0) 50 "re-yield: second yield 50")
  (assert-eq (coro/resume rco2) 99 "re-yield: final return 99"))

# ============================================================================
# Error during multi-frame resume
# ============================================================================

# error_during_multi_frame_resume: error propagates through suspended frames
(begin
  (def eh (fn [x]
    (yield x)
    (/ 1 0)))
  (def egen (fn [] (+ (eh 5) 1)))
  (var eco (make-coroutine egen))
  (coro/resume eco)
  (assert-err (fn [] (coro/resume eco))
    "error during multi-frame resume: division by zero"))

# ============================================================================
# Three-level nested fiber resume
# ============================================================================

# three_level_nested_fiber_resume: A -> B -> C value threading
(let ([c (fiber/new (fn [] (fiber/signal 2 10)) 2)])
  (let ([b (fiber/new (fn [] (+ (fiber/resume c) 5)) 0)])
    (let ([a (fiber/new (fn [] (+ (fiber/resume b) 3)) 0)])
      (assert-eq (fiber/resume a) 18
        "3-level nested: 10 + 5 + 3 = 18"))))

(let ([c (fiber/new (fn [] (fiber/signal 2 -20)) 2)])
  (let ([b (fiber/new (fn [] (+ (fiber/resume c) 30)) 0)])
    (let ([a (fiber/new (fn [] (+ (fiber/resume b) -5)) 0)])
      (assert-eq (fiber/resume a) 5
        "3-level nested: -20 + 30 + -5 = 5"))))
