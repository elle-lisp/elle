## JIT Yield Tests
##
## Tests that verify yield propagates correctly through JIT-compiled
## call chains. The JIT side-exits to the interpreter on yield; resume
## goes through the interpreter.


# ============================================================================
# JIT yield through call
# ============================================================================

# test_jit_yield_through_call
(begin
  (def inner (fn [] (yield 42) 99))
  (def outer (fn [] (inner)))
  (def run (fn [] (outer)))

  # Warm up: run calls outer via call_inner (profiling outer),
  # outer calls inner via call_inner (profiling inner).
  (var warmup-i 0)
  (forever
    (if (>= warmup-i 15) (break))
    (def warmup-c (make-coroutine run))
    (coro/resume warmup-c)
    (coro/resume warmup-c)
    (assign warmup-i (+ warmup-i 1)))

  # Now outer should be JIT-compiled. Test it.
  (def c (make-coroutine run))
  (def v1 (coro/resume c))
  (def v2 (coro/resume c))
  (assert (= v1 42) "JIT yield through call: first")
  (assert (= v2 99) "JIT yield through call: second"))

# ============================================================================
# JIT yield with resume value
# ============================================================================

# test_jit_yield_through_call_with_resume_value
(begin
  (def inner (fn [] (def x (yield 1)) (+ x 10)))
  (def outer (fn [] (inner)))
  (def run (fn [] (outer)))

  (var warmup-i 0)
  (forever
    (if (>= warmup-i 15) (break))
    (def warmup-c (make-coroutine run))
    (coro/resume warmup-c)
    (coro/resume warmup-c 0)
    (assign warmup-i (+ warmup-i 1)))

  (def c (make-coroutine run))
  (coro/resume c)
  (assert (= (coro/resume c 5) 15) "JIT yield with resume value"))

# ============================================================================
# JIT yield through multiple yields
# ============================================================================

# test_jit_yield_through_call_multiple_yields
(begin
  (def inner (fn [] (yield 1) (yield 2) (yield 3) 4))
  (def outer (fn [] (inner)))
  (def run (fn [] (outer)))

  (var warmup-i 0)
  (forever
    (if (>= warmup-i 15) (break))
    (def warmup-c (make-coroutine run))
    (coro/resume warmup-c)
    (coro/resume warmup-c)
    (coro/resume warmup-c)
    (coro/resume warmup-c)
    (assign warmup-i (+ warmup-i 1)))

  (def c (make-coroutine run))
  (assert (= (coro/resume c) 1) "JIT multiple yields: first")
  (assert (= (coro/resume c) 2) "JIT multiple yields: second")
  (assert (= (coro/resume c) 3) "JIT multiple yields: third")
  (assert (= (coro/resume c) 4) "JIT multiple yields: fourth"))

# ============================================================================
# JIT yield stack preservation
# ============================================================================

# test_jit_yield_through_call_stack_preservation
(begin
  (def inner (fn [] (yield 10) 20))
  (def outer (fn [] (+ 1 (inner))))
  (def run (fn [] (outer)))

  (var warmup-i 0)
  (forever
    (if (>= warmup-i 15) (break))
    (def warmup-c (make-coroutine run))
    (coro/resume warmup-c)
    (coro/resume warmup-c)
    (assign warmup-i (+ warmup-i 1)))

  (def c (make-coroutine run))
  (def v1 (coro/resume c))
  (def v2 (coro/resume c))
  (assert (= v1 10) "JIT stack preservation: first")
  (assert (= v2 21) "JIT stack preservation: 1+20=21"))

# ============================================================================
# JIT yield through nested calls
# ============================================================================

# test_jit_yield_through_nested_calls
(begin
  (def inner (fn [] (yield 42) 99))
  (def middle (fn [] (inner)))
  (def outer (fn [] (middle)))
  (def run (fn [] (outer)))

  (var warmup-i 0)
  (forever
    (if (>= warmup-i 15) (break))
    (def warmup-c (make-coroutine run))
    (coro/resume warmup-c)
    (coro/resume warmup-c)
    (assign warmup-i (+ warmup-i 1)))

  (def c (make-coroutine run))
  (def v1 (coro/resume c))
  (def v2 (coro/resume c))
  (assert (= v1 42) "JIT nested calls: first")
  (assert (= v2 99) "JIT nested calls: second"))

# ============================================================================
# JIT yield with captures
# ============================================================================

# test_jit_yield_with_captures
(begin
  (def make-gen (fn (offset)
    (fn [] (yield offset) (+ offset 1))))
  (def outer (fn [] ((make-gen 10))))
  (def run (fn [] (outer)))

  (var warmup-i 0)
  (forever
    (if (>= warmup-i 15) (break))
    (def warmup-c (make-coroutine run))
    (coro/resume warmup-c)
    (coro/resume warmup-c)
    (assign warmup-i (+ warmup-i 1)))

  (def c (make-coroutine run))
  (def v1 (coro/resume c))
  (def v2 (coro/resume c))
  (assert (= v1 10) "JIT with captures: first")
  (assert (= v2 11) "JIT with captures: second"))

# ============================================================================
# JIT yield locals survive yield/resume
# ============================================================================

# test_jit_yield_locals_survive_yield_resume
(begin
  (def inner (fn [] (yield 1) 2))
  (def outer (fn []
    (let ((x 10))
      (let ((y (inner)))
        (+ x y)))))
  (def run (fn [] (outer)))

  (var warmup-i 0)
  (forever
    (if (>= warmup-i 15) (break))
    (def warmup-c (make-coroutine run))
    (coro/resume warmup-c)
    (coro/resume warmup-c)
    (assign warmup-i (+ warmup-i 1)))

  (def c (make-coroutine run))
  (def v1 (coro/resume c))
  (def v2 (coro/resume c))
  (assert (= v1 1) "JIT locals survive: first")
  (assert (= v2 12) "JIT locals survive: 10+2=12"))

# ============================================================================
# JIT yield multiple locals survive
# ============================================================================

# test_jit_yield_multiple_locals_survive
(begin
  (def inner (fn [] (yield 100) 200))
  (def outer (fn []
    (let ((a 1))
      (let ((b 2))
        (let ((c 3))
          (let ((d (inner)))
            (+ a (+ b (+ c d)))))))))
  (def run (fn [] (outer)))

  (var warmup-i 0)
  (forever
    (if (>= warmup-i 15) (break))
    (def warmup-c (make-coroutine run))
    (coro/resume warmup-c)
    (coro/resume warmup-c)
    (assign warmup-i (+ warmup-i 1)))

  (def c (make-coroutine run))
  (def v1 (coro/resume c))
  (def v2 (coro/resume c))
  (assert (= v1 100) "JIT multiple locals: first")
  (assert (= v2 206) "JIT multiple locals: 1+2+3+200=206"))
