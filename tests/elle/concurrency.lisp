# Tests for concurrency primitives (spawn, join, current-thread-id)

(import-file "tests/elle/assert.lisp")

# ============================================================================
# Basic spawn/join tests
# ============================================================================

(assert-true
  (begin
    (let ((x 42))
      (let ((handle (spawn (fn () x))))
        (join handle)))
    true)
  "spawn closure with immutable capture")

(assert-true
  (begin
    (let ((msg "hello from thread"))
      (let ((handle (spawn (fn () msg))))
        (join handle)))
    true)
  "spawn closure with string capture")

(assert-true
  (begin
    (let ((v [1 2 3]))
      (let ((handle (spawn (fn () v))))
        (join handle)))
    true)
  "spawn closure with array capture")

(assert-true
  (begin
    (let ((x 10) (y 20))
      (let ((handle (spawn (fn () (+ x y)))))
        (join handle)))
    true)
  "spawn closure computation")

(assert-true
  (begin
    (let ((a 1) (b 2) (c 3))
      (let ((handle (spawn (fn () (+ a (+ b c))))))
        (join handle)))
    true)
  "spawn closure with multiple captures")

(assert-true
  (begin
    (let ((n nil))
      (let ((handle (spawn (fn () n))))
        (join handle)))
    true)
  "spawn closure with nil capture")

(assert-true
  (begin
    (let ((f 3.14159))
      (let ((handle (spawn (fn () f))))
        (join handle)))
    true)
  "spawn closure with float capture")

(assert-true
  (begin
    (let ((lst (list 1 2 3)))
      (let ((handle (spawn (fn () lst))))
        (join handle)))
    true)
  "spawn closure with list capture")

(assert-true
  (begin
    (let ((handle (spawn (fn () 42))))
      (join handle))
    true)
  "spawn closure no captures")

(assert-true
  (begin
    (let ((x 10))
      (let ((handle (spawn (fn () (if (> x 5) "big" "small")))))
        (join handle)))
    true)
  "spawn closure with conditional")

# ============================================================================
# current-thread-id tests
# ============================================================================

(assert-true
  (begin
    (let ((tid (current-thread-id)))
      (int? tid))
    true)
  "current thread id returns integer")

# ============================================================================
# JIT closure tests
# ============================================================================

(assert-true
  (begin
    (let ((x 42))
      (let ((closure (fn () x)))
        (let ((handle (spawn closure)))
          (join handle))))
    true)
  "spawn jit closure with capture")

(assert-true
  (begin
    (let ((a 10) (b 20))
      (let ((closure (fn () (+ a b))))
        (let ((handle (spawn closure)))
          (join handle))))
    true)
  "spawn jit closure with computation")

(assert-true
  (begin
    (let ((msg "hello from jit thread"))
      (let ((closure (fn () msg)))
        (let ((handle (spawn closure)))
          (join handle))))
    true)
  "spawn jit closure with string capture")

(assert-true
  (begin
    (let ((v [10 20 30]))
      (let ((closure (fn () v)))
        (let ((handle (spawn closure)))
          (join handle))))
    true)
  "spawn jit closure with array capture")

(assert-true
  (begin
    (let ((a 1) (b 2) (c 3))
      (let ((closure (fn () (+ a (+ b c)))))
        (let ((handle (spawn closure)))
          (join handle))))
    true)
  "spawn jit closure with multiple captures")

(assert-true
  (begin
    (let ((x 10))
      (let ((closure (fn () (if (> x 5) "big" "small"))))
        (let ((handle (spawn closure)))
          (join handle))))
    true)
  "spawn jit closure with conditional")

# ============================================================================
# Error tests (from integration/concurrency.rs)
# ============================================================================

# spawn_rejects_mutable_table_capture
(assert-err (fn ()
  (let ((t (table)))
    (spawn (fn () t))))
  "spawn rejects mutable table capture")

# spawn_rejects_native_function
(assert-err (fn () (spawn +))
  "spawn rejects native function")

# spawn_wrong_arity
(assert-err (fn () (eval '(spawn)))
  "spawn wrong arity: no args")

(assert-err (fn () (eval '(spawn (fn () 1) 2)))
  "spawn wrong arity: two args")

# join_wrong_arity
(assert-err (fn () (eval '(join)))
  "join wrong arity: no args")

(assert-err (fn () (eval '(join 1 2)))
  "join wrong arity: two args")

# join_invalid_argument
(assert-err (fn () (join 42))
  "join rejects non-thread-handle")

# sleep_negative_duration
(assert-err (fn () (time/sleep -1))
  "sleep rejects negative int")

(assert-err (fn () (time/sleep -0.5))
  "sleep rejects negative float")

# sleep_non_numeric
(assert-err (fn () (time/sleep "hello"))
  "sleep rejects non-numeric")
