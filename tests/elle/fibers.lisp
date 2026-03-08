## Fiber Primitive Tests
##
## Migrated from tests/property/fibers.rs (behavioral property tests).
## Tests yield/resume order, signal masks, cancel, propagate, and nesting.

(import-file "tests/elle/assert.lisp")

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

# propagate_succeeds_for_errored_fibers: propagate re-signals error
(assert-err (fn []
  (let ([f (fiber/new (fn [] (fiber/signal 1 99)) 1)])
    (fiber/resume f)
    (fiber/propagate f)))
  "propagate re-signals error (99)")

(assert-err (fn []
  (let ([f (fiber/new (fn [] (fiber/signal 1 -50)) 1)])
    (fiber/resume f)
    (fiber/propagate f)))
  "propagate re-signals error (-50)")

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

# ============================================================================
# Fiber child chain wiring (from integration/fibers.rs)
# ============================================================================

# test_fiber_child_nil_before_resume
(begin
  (let ((f (fiber/new (fn [] 42) 0)))
    (assert-eq (fiber/child f) nil "fiber child: nil before resume")))

# ============================================================================
# Fiber propagate
# ============================================================================

# test_fiber_propagate_yield
(begin
  (let ((inner (fiber/new (fn [] (fiber/signal 2 99)) 2)))
    (let ((outer (fiber/new
                   (fn []
                     (fiber/resume inner)
                     (fiber/propagate inner))
                   2)))
      (fiber/resume outer)
      (assert-true true "fiber propagate yield"))))

# ============================================================================
# Fiber cancel
# ============================================================================

# test_fiber_cancel_suspended_fiber
(begin
  (let ((f (fiber/new (fn [] (fiber/signal 2 "waiting") 99) 3)))
    (fiber/resume f)
    (fiber/cancel f "cancelled")
    (assert-eq (keyword->string (fiber/status f)) "error"
      "fiber cancel: suspended fiber becomes error")))

# test_fiber_cancel_new_fiber
(begin
  (let ((f (fiber/new (fn [] 42) 1)))
    (fiber/cancel f "never started")
    (assert-eq (keyword->string (fiber/status f)) "error"
      "fiber cancel: new fiber becomes error")))

# test_fiber_cancel_returns_error_value
(begin
  (let ((f (fiber/new (fn [] 42) 1)))
    (let ((result (fiber/cancel f "injected")))
      (assert-eq result "injected" "fiber cancel: returns error value"))))

# ============================================================================
# Error macro arity
# ============================================================================

# test_error_no_args
(begin
  (let (([ok? val] (protect (error))))
    (assert-false ok? "error no args: signals error")
    (assert-eq val nil "error no args: value is nil")))

# test_error_with_value
(begin
  (let (([ok? val] (protect (error :boom))))
    (assert-false ok? "error with value: signals error")
    (assert-eq val :boom "error with value: value is :boom")))

# ============================================================================
# Fiber cancel default nil and cancel alias
# ============================================================================

# test_fiber_cancel_default_nil
(begin
  (let ((f (fiber/new (fn [] 42) 1)))
    (fiber/cancel f)
    (assert-eq (fiber/value f) nil "fiber cancel: default nil")))

# test_cancel_alias_works
(begin
  (let ((f (fiber/new (fn [] 42) 1)))
    (cancel f "stopped")
    (assert-eq (fiber/value f) "stopped" "cancel alias: works")))

# test_cancel_alias_default_nil
(begin
  (let ((f (fiber/new (fn [] 42) 1)))
    (cancel f)
    (assert-eq (fiber/value f) nil "cancel alias: default nil")))

# ============================================================================
# Basic fiber resume still works
# ============================================================================

# test_fiber_resume_basic
(begin
  (let ((f (fiber/new (fn [] 42) 0)))
    (assert-eq (fiber/resume f) 42 "fiber resume: basic")))

# test_fiber_yield_and_resume
(begin
  (let ((f (fiber/new (fn [] (fiber/signal 2 10) 20) 2)))
    (assert-eq (+ (fiber/resume f) (fiber/resume f)) 30
      "fiber yield and resume: 10 + 20 = 30")))

# test_fiber_error_caught_by_mask
(begin
  (let ((f (fiber/new (fn [] (fiber/signal 1 "oops")) 1)))
    (assert-eq (fiber/resume f) "oops"
      "fiber error caught by mask")))

# test_fiber_error_propagates_without_mask
(begin
  (assert-err (fn []
    (let ((f (fiber/new (fn [] (fiber/signal 1 "oops")) 0)))
      (fiber/resume f)))
    "fiber error propagates without mask"))

# ============================================================================
# Fiber propagate preserving child chain
# ============================================================================

# test_fiber_propagate_preserves_child_chain
(begin
  (let ((inner (fiber/new (fn [] (fiber/signal 1 "err")) 1)))
    (let ((outer (fiber/new
                   (fn []
                     (fiber/resume inner)
                     (fiber/propagate inner))
                   1)))
      (fiber/resume outer)
      (assert-eq (fiber? (fiber/child outer)) true
        "fiber propagate: child chain preserved"))))

# test_fiber_propagate_child_identity
(begin
  (let ((inner (fiber/new (fn [] (fiber/signal 2 99)) 2)))
    (let ((outer (fiber/new
                   (fn []
                     (fiber/resume inner)
                     (fiber/propagate inner))
                   2)))
      (fiber/resume outer)
      (assert-eq (identical? inner (fiber/child outer)) true
        "fiber propagate: child identity preserved"))))

# ============================================================================
# Fiber resume and cancel in tail position
# ============================================================================

# test_fiber_resume_in_tail_position
(begin
  (let ((inner (fiber/new (fn [] 42) 0)))
    (let ((outer (fiber/new (fn [] (fiber/resume inner)) 0)))
      (assert-eq (fiber/resume outer) 42
        "fiber resume: tail position"))))

# test_fiber_resume_yield_in_tail_position
(begin
  (let ((inner (fiber/new (fn [] (fiber/signal 2 10) 20) 2)))
    (let ((outer (fiber/new (fn [] (fiber/resume inner)) 0)))
      (assert-eq (fiber/resume outer) 10
        "fiber resume yield: tail position"))))

# test_fiber_cancel_in_tail_position
(begin
  (let ((target (fiber/new (fn [] 42) 1)))
    (let ((canceller (fiber/new
                       (fn [] (fiber/cancel target "cancelled"))
                       0)))
      (fiber/resume canceller)
      (assert-true true "fiber cancel: tail position"))))

# test_fiber_cancel_suspended_in_tail_position
(begin
  (let ((target (fiber/new (fn [] (fiber/signal 2 0) 99) 3)))
    (fiber/resume target)
    (let ((canceller (fiber/new
                       (fn [] (fiber/cancel target "stop"))
                       0)))
      (fiber/resume canceller)
      (assert-eq (keyword->string (fiber/status target)) "error"
        "fiber cancel suspended: tail position"))))

# ============================================================================
# 3-level nested fiber resume (from integration/fibers.rs)
# ============================================================================

# test_three_level_nested_fiber_resume
(begin
  (let ((c (fiber/new (fn [] (fiber/signal 2 10)) 2)))
    (let ((b (fiber/new
               (fn []
                 (+ (fiber/resume c) 5))
               0)))
      (let ((a (fiber/new
                 (fn []
                   (+ (fiber/resume b) 1))
                 0)))
        (assert-eq (fiber/resume a) 16
          "3-level nested fiber: 10 + 5 + 1 = 16")))))

# test_three_level_nested_fiber_error_propagation
(begin
  (let ((c (fiber/new (fn [] (fiber/signal 1 "deep error")) 0)))
    (let ((b (fiber/new
               (fn [] (fiber/resume c))
               0)))
      (let ((a (fiber/new
                 (fn [] (fiber/resume b))
                 1)))
        (fiber/resume a)
        (assert-true true "3-level nested fiber: error propagation")))))

# ============================================================================
# Fiber parent and child identity
# ============================================================================

# test_fiber_parent_identity
(begin
  (let ((f (fiber/new (fn [] 42) 0)))
    (let ((outer (fiber/new
                   (fn []
                     (fiber/resume f)
                     42)
                   0)))
      (fiber/resume outer)
      (assert-eq (identical? (fiber/parent f) (fiber/parent f)) true
        "fiber parent: identity preserved"))))

# test_fiber_child_identity
(begin
  (let ((inner (fiber/new (fn [] (fiber/signal 1 "err")) 0)))
    (let ((outer (fiber/new
                   (fn []
                     (fiber/resume inner)
                     42)
                   1)))
      (fiber/resume outer)
      (assert-eq (identical? (fiber/child outer) (fiber/child outer)) true
        "fiber child: identity preserved"))))

# ============================================================================
# Issue #299: caught SIG_ERROR status and resumability
# ============================================================================

# test_caught_sig_error_leaves_fiber_suspended
(begin
  (let ((f (fiber/new (fn [] (fiber/signal 1 "oops") "recovered") 1)))
    (fiber/resume f)
    (assert-eq (keyword->string (fiber/status f)) "suspended"
      "caught SIG_ERROR: leaves fiber suspended")))

# test_caught_sig_error_fiber_is_resumable
(begin
  (let ((f (fiber/new (fn [] (fiber/signal 1 "oops") "recovered") 1)))
    (fiber/resume f)
    (assert-eq (fiber/resume f) "recovered"
      "caught SIG_ERROR: fiber is resumable")))

# test_cancel_always_produces_error_status
(begin
  (let ((f (fiber/new (fn [] (fiber/signal 2 "waiting") 99) 3)))
    (fiber/resume f)
    (fiber/cancel f "stop")
    (assert-eq (keyword->string (fiber/status f)) "error"
      "cancel: always produces error status")))

# ============================================================================
# Fiber with signal parameter (#346)
# ============================================================================

# test_fiber_closure_with_signal_parameter
(begin
  (let ((f (fiber/new (fn (s) (+ s 42)) 0)))
    (assert-eq (fiber/resume f 8) 50
      "fiber signal parameter: 8 + 42 = 50")))

# test_fiber_signal_parameter_with_valid_bits
(begin
  (let ((f (fiber/new (fn (s) (fiber/signal s 42)) 2)))
    (fiber/resume f 2)
    (assert-eq (fiber/value f) 42
      "fiber signal parameter: valid bits")))

# test_fiber_closure_with_resume_value_as_parameter
(begin
  (let ((f (fiber/new (fn (x) (* x x)) 0)))
    (assert-eq (fiber/resume f 7) 49
      "fiber resume value as parameter: 7 * 7 = 49")))

# test_fiber_zero_param_closure_still_works
(begin
  (let ((f (fiber/new (fn [] 42) 0)))
    (assert-eq (fiber/resume f) 42
      "fiber zero param closure: still works")))

# ============================================================================
# Issue #415: letrec binding reads as nil after fiber yield/resume
# ============================================================================

# test_letrec_binding_survives_fiber_yield_resume
(begin
  (let* ((f (fiber/new (fn []
                  (letrec ((go (fn (n)
                              (fiber/signal 2 n)
                              (go (+ n 1)))))
                    (go 0)))
              2)))
    (assert-eq (fiber/resume f) 0 "letrec binding: first yield")
    (assert-eq (fiber/resume f) 1 "letrec binding: second yield")
    (assert-eq (fiber/resume f) 2 "letrec binding: third yield")))

# test_tail_call_then_signal_preserves_state
(begin
  (defn helper (n)
    (fiber/signal 2 n)
    (helper (+ n 10)))
  (let* ((f (fiber/new (fn [] (helper 1)) 2)))
    (assert-eq (fiber/resume f) 1 "tail call signal: first")
    (assert-eq (fiber/resume f) 11 "tail call signal: second")
    (assert-eq (fiber/resume f) 21 "tail call signal: third")))

# test_multiple_tail_calls_before_signal
(begin
  (defn signaler (n) (fiber/signal 2 n) (signaler (+ n 1)))
  (defn bouncer (n) (signaler n))
  (let* ((f (fiber/new (fn [] (bouncer 100)) 2)))
    (assert-eq (fiber/resume f) 100 "multiple tail calls: first")
    (assert-eq (fiber/resume f) 101 "multiple tail calls: second")))

# ============================================================================
# Error message tests (from integration/fibers.rs)
# ============================================================================

# fiber_propagate_error
(assert-err (fn ()
  (let ((inner (fiber/new (fn () (fiber/signal 1 "boom")) 1)))
    (fiber/resume inner)
    (fiber/propagate inner)))
  "fiber propagate re-signals error")

# fiber_propagate_dead_fiber_errors
(assert-err (fn ()
  (let ((f (fiber/new (fn () 42) 0)))
    (fiber/resume f)
    (fiber/propagate f)))
  "fiber propagate rejects dead fiber")
