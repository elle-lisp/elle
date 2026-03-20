# Tests for concurrency primitives (spawn, join, current-thread-id)


# ============================================================================
# Basic spawn/join tests
# ============================================================================

(assert (begin
    (let ((x 42))
      (let ((handle (spawn (fn () x))))
        (join handle)))
    true) "spawn closure with immutable capture")

(assert (begin
    (let ((msg "hello from thread"))
      (let ((handle (spawn (fn () msg))))
        (join handle)))
    true) "spawn closure with string capture")

(assert (begin
    (let ((v [1 2 3]))
      (let ((handle (spawn (fn () v))))
        (join handle)))
    true) "spawn closure with array capture")

(assert (begin
    (let ((x 10) (y 20))
      (let ((handle (spawn (fn () (+ x y)))))
        (join handle)))
    true) "spawn closure computation")

(assert (begin
    (let ((a 1) (b 2) (c 3))
      (let ((handle (spawn (fn () (+ a (+ b c))))))
        (join handle)))
    true) "spawn closure with multiple captures")

(assert (begin
    (let ((n nil))
      (let ((handle (spawn (fn () n))))
        (join handle)))
    true) "spawn closure with nil capture")

(assert (begin
    (let ((f 3.14159))
      (let ((handle (spawn (fn () f))))
        (join handle)))
    true) "spawn closure with float capture")

(assert (begin
    (let ((lst (list 1 2 3)))
      (let ((handle (spawn (fn () lst))))
        (join handle)))
    true) "spawn closure with list capture")

(assert (begin
    (let ((handle (spawn (fn () 42))))
      (join handle))
    true) "spawn closure no captures")

(assert (begin
    (let ((x 10))
      (let ((handle (spawn (fn () (if (> x 5) "big" "small")))))
        (join handle)))
    true) "spawn closure with conditional")

# ============================================================================
# current-thread-id tests
# ============================================================================

(assert (begin
    (let ((tid (current-thread-id)))
      (int? tid))
    true) "current thread id returns integer")

# ============================================================================
# JIT closure tests
# ============================================================================

(assert (begin
    (let ((x 42))
      (let ((closure (fn () x)))
        (let ((handle (spawn closure)))
          (join handle))))
    true) "spawn jit closure with capture")

(assert (begin
    (let ((a 10) (b 20))
      (let ((closure (fn () (+ a b))))
        (let ((handle (spawn closure)))
          (join handle))))
    true) "spawn jit closure with computation")

(assert (begin
    (let ((msg "hello from jit thread"))
      (let ((closure (fn () msg)))
        (let ((handle (spawn closure)))
          (join handle))))
    true) "spawn jit closure with string capture")

(assert (begin
    (let ((v [10 20 30]))
      (let ((closure (fn () v)))
        (let ((handle (spawn closure)))
          (join handle))))
    true) "spawn jit closure with array capture")

(assert (begin
    (let ((a 1) (b 2) (c 3))
      (let ((closure (fn () (+ a (+ b c)))))
        (let ((handle (spawn closure)))
          (join handle))))
    true) "spawn jit closure with multiple captures")

(assert (begin
    (let ((x 10))
      (let ((closure (fn () (if (> x 5) "big" "small"))))
        (let ((handle (spawn closure)))
          (join handle))))
    true) "spawn jit closure with conditional")

# ============================================================================
# Error tests (from integration/concurrency.rs)
# ============================================================================

# spawn_rejects_mutable_table_capture
(let (([ok? _] (protect ((fn ()
  (let ((t (@struct)))
    (spawn (fn () t)))))))) (assert (not ok?) "spawn rejects mutable @struct capture"))

# spawn_rejects_native_function
(let (([ok? _] (protect ((fn () (spawn +)))))) (assert (not ok?) "spawn rejects native function"))

# spawn_wrong_arity
(let (([ok? _] (protect ((fn () (eval '(spawn))))))) (assert (not ok?) "spawn wrong arity: no args"))

(let (([ok? _] (protect ((fn () (eval '(spawn (fn () 1) 2))))))) (assert (not ok?) "spawn wrong arity: two args"))

# join_wrong_arity
(let (([ok? _] (protect ((fn () (eval '(join))))))) (assert (not ok?) "join wrong arity: no args"))

(let (([ok? _] (protect ((fn () (eval '(join 1 2))))))) (assert (not ok?) "join wrong arity: two args"))

# join_invalid_argument
(let (([ok? _] (protect ((fn () (join 42)))))) (assert (not ok?) "join rejects non-thread-handle"))

# sleep_negative_duration
(let (([ok? _] (protect ((fn () (time/sleep -1)))))) (assert (not ok?) "sleep rejects negative int"))

(let (([ok? _] (protect ((fn () (time/sleep -0.5)))))) (assert (not ok?) "sleep rejects negative float"))

# sleep_non_numeric
(let (([ok? _] (protect ((fn () (time/sleep "hello")))))) (assert (not ok?) "sleep rejects non-numeric"))

# ============================================================================
# Closure capturing closure tests
# ============================================================================

(assert (= (let ((add1 (fn (x) (+ x 1))))
    (join (spawn (fn () (add1 41))))) 42) "spawn closure capturing closure")

(assert (= (let ((add1 (fn (x) (+ x 1))))
    (let ((add2 (fn (x) (add1 (add1 x)))))
      (join (spawn (fn () (add2 40)))))) 42) "spawn closure capturing nested closures")

(assert (= (let ((f (join (spawn (fn () (fn (x) (* x 2)))))))
    (f 21)) 42) "spawn closure returning closure")

(assert (= (let ((offset 10))
    (let ((add-offset (fn (x) (+ x offset))))
      (join (spawn (fn () (add-offset 32)))))) 42) "spawn closure capturing closure and data")

(let (([ok? _] (protect ((fn ()
  (let ((t (@struct)))
    (let ((f (fn () t)))
      (spawn (fn () (f)))))))))) (assert (not ok?) "spawn rejects closure capturing closure with @struct"))

# ============================================================================
# Recursive closure tests (letrec)
# ============================================================================

(assert (= (letrec ((fact (fn (n) (if (= n 0) 1 (* n (fact (- n 1)))))))
    (join (spawn (fn () (fact 6))))) 720) "spawn self-recursive closure")

(assert (= (letrec ((even? (fn (n) (if (= n 0) true (odd? (- n 1)))))
           (odd?  (fn (n) (if (= n 0) false (even? (- n 1))))))
    (join (spawn (fn () (even? 10))))) true) "spawn mutually recursive closures")

(assert (= (letrec ((even? (fn (n) (if (= n 0) true (odd? (- n 1)))))
           (odd?  (fn (n) (if (= n 0) false (even? (- n 1))))))
    (join (spawn (fn () (odd? 99))))) true) "spawn mutual recursion deep")
