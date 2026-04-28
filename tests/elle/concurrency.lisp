(elle/epoch 9)
# Tests for concurrency primitives (spawn, join, current-thread-id)


# ============================================================================
# Basic spawn/join tests
# ============================================================================

(assert (begin
          (let [x 42]
            (let [handle (spawn (fn () x))]
              (join handle)))
          true) "spawn closure with immutable capture")

(assert (begin
          (let [msg "hello from thread"]
            (let [handle (spawn (fn () msg))]
              (join handle)))
          true) "spawn closure with string capture")

(assert (begin
          (let [v [1 2 3]]
            (let [handle (spawn (fn () v))]
              (join handle)))
          true) "spawn closure with array capture")

(assert (begin
          (let [x 10
                y 20]
            (let [handle (spawn (fn () (+ x y)))]
              (join handle)))
          true) "spawn closure computation")

(assert (begin
          (let [a 1
                b 2
                c 3]
            (let [handle (spawn (fn () (+ a (+ b c))))]
              (join handle)))
          true) "spawn closure with multiple captures")

(assert (begin
          (let [n nil]
            (let [handle (spawn (fn () n))]
              (join handle)))
          true) "spawn closure with nil capture")

(assert (begin
          (let [f 3.14159]
            (let [handle (spawn (fn () f))]
              (join handle)))
          true) "spawn closure with float capture")

(assert (begin
          (let [lst (list 1 2 3)]
            (let [handle (spawn (fn () lst))]
              (join handle)))
          true) "spawn closure with list capture")

(assert (begin
          (let [handle (spawn (fn () 42))]
            (join handle))
          true) "spawn closure no captures")

(assert (begin
          (let [x 10]
            (let [handle (spawn (fn () (if (> x 5) "big" "small")))]
              (join handle)))
          true) "spawn closure with conditional")

# ============================================================================
# current-thread-id tests
# ============================================================================

(assert (begin
          (let [tid (current-thread-id)]
            (int? tid))
          true) "current thread id returns integer")

# ============================================================================
# JIT closure tests
# ============================================================================

(assert (begin
          (let [x 42]
            (let [closure (fn () x)]
              (let [handle (spawn closure)]
                (join handle))))
          true) "spawn jit closure with capture")

(assert (begin
          (let [a 10
                b 20]
            (let [closure (fn () (+ a b))]
              (let [handle (spawn closure)]
                (join handle))))
          true) "spawn jit closure with computation")

(assert (begin
          (let [msg "hello from jit thread"]
            (let [closure (fn () msg)]
              (let [handle (spawn closure)]
                (join handle))))
          true) "spawn jit closure with string capture")

(assert (begin
          (let [v [10 20 30]]
            (let [closure (fn () v)]
              (let [handle (spawn closure)]
                (join handle))))
          true) "spawn jit closure with array capture")

(assert (begin
          (let [a 1
                b 2
                c 3]
            (let [closure (fn () (+ a (+ b c)))]
              (let [handle (spawn closure)]
                (join handle))))
          true) "spawn jit closure with multiple captures")

(assert (begin
          (let [x 10]
            (let [closure (fn () (if (> x 5) "big" "small"))]
              (let [handle (spawn closure)]
                (join handle))))
          true) "spawn jit closure with conditional")

# ============================================================================
# Error tests (from integration/concurrency.rs)
# ============================================================================

# spawn_rejects_mutable_table_capture
(let [[ok? _] (protect ((fn ()
                          (let [t (@struct)]
                            (spawn (fn () t))))))]
  (assert (not ok?) "spawn rejects mutable @struct capture"))

# spawn_rejects_native_function
(let [[ok? _] (protect ((fn () (spawn +))))]
  (assert (not ok?) "spawn rejects native function"))

# spawn_wrong_arity
(let [[ok? _] (protect ((fn () (eval '(spawn)))))]
  (assert (not ok?) "spawn wrong arity: no args"))

(let [[ok? _] (protect ((fn () (eval '(spawn (fn () 1) 2)))))]
  (assert (not ok?) "spawn wrong arity: two args"))

# join_wrong_arity
(let [[ok? _] (protect ((fn () (eval '(join)))))]
  (assert (not ok?) "join wrong arity: no args"))

(let [[ok? _] (protect ((fn () (eval '(join 1 2)))))]
  (assert (not ok?) "join wrong arity: two args"))

# join_invalid_argument
(let [[ok? _] (protect ((fn () (join 42))))]
  (assert (not ok?) "join rejects non-thread-handle"))

# sleep_negative_duration
(let [[ok? _] (protect ((fn () (time/sleep -1))))]
  (assert (not ok?) "sleep rejects negative int"))

(let [[ok? _] (protect ((fn () (time/sleep -0.5))))]
  (assert (not ok?) "sleep rejects negative float"))

# sleep_non_numeric
(let [[ok? _] (protect ((fn () (time/sleep "hello"))))]
  (assert (not ok?) "sleep rejects non-numeric"))

# ============================================================================
# Closure capturing closure tests
# ============================================================================

(assert (= (let [add1 (fn (x) (+ x 1))]
             (join (spawn (fn () (add1 41))))) 42)
  "spawn closure capturing closure")

(assert (= (let [add1 (fn (x) (+ x 1))]
             (let [add2 (fn (x) (add1 (add1 x)))]
               (join (spawn (fn () (add2 40)))))) 42)
  "spawn closure capturing nested closures")

(assert (= (let [f (join (spawn (fn () (fn (x) (* x 2)))))]
             (f 21)) 42) "spawn closure returning closure")

(assert (= (let [offset 10]
             (let [add-offset (fn (x) (+ x offset))]
               (join (spawn (fn () (add-offset 32)))))) 42)
  "spawn closure capturing closure and data")

(let [[ok? _] (protect ((fn ()
                          (let [t (@struct)]
                            (let [f (fn () t)]
                              (spawn (fn () (f))))))))]
  (assert (not ok?) "spawn rejects closure capturing closure with @struct"))

# ============================================================================
# Recursive closure tests (letrec)
# ============================================================================

(assert (= (letrec [fact (fn (n)
                           (if (= n 0)
                             1
                             (* n (fact (- n 1)))))]
             (join (spawn (fn () (fact 6))))) 720)
  "spawn self-recursive closure")

(assert (= (letrec [even? (fn (n) (if (= n 0) true (odd? (- n 1))))
                    odd? (fn (n) (if (= n 0) false (even? (- n 1))))]
             (join (spawn (fn () (even? 10))))) true)
  "spawn mutually recursive closures")

(assert (= (letrec [even? (fn (n) (if (= n 0) true (odd? (- n 1))))
                    odd? (fn (n) (if (= n 0) false (even? (- n 1))))]
             (join (spawn (fn () (odd? 99))))) true)
  "spawn mutual recursion deep")

# ============================================================================
# JIT on spawned threads: closures capturing other closures in hot loops.
# The spawned closure calls the captured helper enough times to exceed the
# JIT threshold on the worker thread. Before the ClosureRef LIR-transfer fix
# (src/lir/types.rs::convert_value_consts_for_send), LIR containing
# closure-valued ValueConst instructions would be dropped on send, silently
# forcing the worker into the interpreter.
# ============================================================================

(assert (= (let [double (fn (x) (* x 2))]
             (letrec [loop (fn (n acc)
                             (if (= n 0)
                               acc
                               (loop (- n 1) (+ acc (double n)))))]
               (join (spawn (fn () (loop 100 0)))))) 10100)
  "spawn hot loop with captured closure (JIT on worker thread)")

(assert (= (let [inc (fn (x) (+ x 1))
                 sq (fn (x) (* x x))]
             (letrec [loop (fn (n acc)
                             (if (= n 0)
                               acc
                               (loop (- n 1) (+ acc (sq (inc n))))))]
               (join (spawn (fn () (loop 50 0)))))) 45525)
  "spawn hot loop with two captured closures")

(assert (= (let [compose (fn (f g) (fn (x) (f (g x))))]
             (let [inc (fn (x) (+ x 1))
                   dbl (fn (x) (* x 2))]
               (let [f (compose dbl inc)]
                 (letrec [loop (fn (n acc)
                                 (if (= n 0)
                                   acc
                                   (loop (- n 1) (+ acc (f n)))))]
                   (join (spawn (fn () (loop 100 0)))))))) 10300)
  "spawn hot loop with composed closures")

# ============================================================================
# Regression test for the ClosureRef LIR-transfer fix.
#
# When a closure is sent across a `sys/spawn` boundary, its LIR function is
# cloned for cross-thread transfer. If the LIR contains `ValueConst`
# instructions holding closure Values (which happens whenever user code
# inside the spawned closure references a stdlib function like `inc`,
# because stdlib functions are registered as primitives and lower to
# `ValueConst`), those Values have to be re-routed to the reconstructed
# closure on the receiving side. The fix in
# `src/lir/types.rs::convert_value_consts_for_send` + the
# `LirConst::ClosureRef` placeholder + `patch_lir_closure_refs` in
# `src/value/send.rs` does exactly that.
#
# Before the fix, `convert_value_consts_for_send` dropped the LIR function
# on any closure-valued ValueConst, silently forcing the worker thread into
# the interpreter and destroying the threaded speedup for e.g. mandelbrot.
#
# This test asserts the fix actually fires: it spawns a closure that calls
# a stdlib function, joins it, and checks that the counter incremented.
# If a future lowering change causes stdlib references to stop appearing as
# ValueConst (or the fix regresses), the assertion will fire and point
# directly at the broken contract.
# ============================================================================

(let [before (lir/closure-value-const-count)]
  (join (sys/spawn (fn [] (inc 41))))
  (let [after (lir/closure-value-const-count)]
    (assert (> after before)
      "ClosureRef LIR-transfer path fires when a spawned closure calls a stdlib function")))
