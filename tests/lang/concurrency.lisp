(elle/epoch 12)
# audited: 2026-09-29
# sys/spawn runs a closure on another thread, and the captures and traits it carries survive the crossing.
# docs/threads.md
#

# ============================================================================
# Basic spawn/join tests
# ============================================================================

(assert (begin
          (let [x 42]
            (let [handle (sys/spawn-vm (fn () x))]
              (sys/join handle)))
          true) "spawn closure with immutable capture")

(assert (begin
          (let [msg "hello from thread"]
            (let [handle (sys/spawn-vm (fn () msg))]
              (sys/join handle)))
          true) "spawn closure with string capture")

(assert (begin
          (let [v [1 2 3]]
            (let [handle (sys/spawn-vm (fn () v))]
              (sys/join handle)))
          true) "spawn closure with array capture")

(assert (begin
          (let [x 10
                y 20]
            (let [handle (sys/spawn-vm (fn () (+ x y)))]
              (sys/join handle)))
          true) "spawn closure computation")

(assert (begin
          (let [a 1
                b 2
                c 3]
            (let [handle (sys/spawn-vm (fn () (+ a (+ b c))))]
              (sys/join handle)))
          true) "spawn closure with multiple captures")

(assert (begin
          (let [n nil]
            (let [handle (sys/spawn-vm (fn () n))]
              (sys/join handle)))
          true) "spawn closure with nil capture")

(assert (begin
          (let [f 3.14159]
            (let [handle (sys/spawn-vm (fn () f))]
              (sys/join handle)))
          true) "spawn closure with float capture")

(assert (begin
          (let [lst (list 1 2 3)]
            (let [handle (sys/spawn-vm (fn () lst))]
              (sys/join handle)))
          true) "spawn closure with list capture")

(assert (begin
          (let [handle (sys/spawn-vm (fn () 42))]
            (sys/join handle))
          true) "spawn closure no captures")

(assert (begin
          (let [x 10]
            (let [handle (sys/spawn-vm (fn () (if (> x 5) "big" "small")))]
              (sys/join handle)))
          true) "spawn closure with conditional")

# ============================================================================
# current-thread-id tests
# ============================================================================

(assert (begin
          (let [tid (current-thread-id)]
            (int? tid))
          true) "current thread id returns integer")

# ============================================================================
# Closure tests
# ============================================================================

(assert (begin
          (let [x 42]
            (let [closure (fn () x)]
              (let [handle (sys/spawn-vm closure)]
                (sys/join handle))))
          true) "spawn jit closure with capture")

(assert (begin
          (let [a 10
                b 20]
            (let [closure (fn () (+ a b))]
              (let [handle (sys/spawn-vm closure)]
                (sys/join handle))))
          true) "spawn jit closure with computation")

(assert (begin
          (let [msg "hello from jit thread"]
            (let [closure (fn () msg)]
              (let [handle (sys/spawn-vm closure)]
                (sys/join handle))))
          true) "spawn jit closure with string capture")

(assert (begin
          (let [v [10 20 30]]
            (let [closure (fn () v)]
              (let [handle (sys/spawn-vm closure)]
                (sys/join handle))))
          true) "spawn jit closure with array capture")

(assert (begin
          (let [a 1
                b 2
                c 3]
            (let [closure (fn () (+ a (+ b c)))]
              (let [handle (sys/spawn-vm closure)]
                (sys/join handle))))
          true) "spawn jit closure with multiple captures")

(assert (begin
          (let [x 10]
            (let [closure (fn () (if (> x 5) "big" "small"))]
              (let [handle (sys/spawn-vm closure)]
                (sys/join handle))))
          true) "spawn jit closure with conditional")

# ============================================================================
# Error tests
# ============================================================================

# spawn_sends_mutable_struct_capture
(let [[ok? result] (protect ((fn ()
                               (let [t (@struct :a 1)]
                                 (sys/join (sys/spawn-vm (fn () (t :a))))))))]
  (assert ok? "spawn sends mutable @struct capture")
  (assert (= result 1) "spawned @struct preserves data"))

# spawn_rejects_native_function
# `parse-int`, not `abs`: abs is stdlib Elle (a closure, legitimately
# spawnable) — the subject must be a real native fn for the rejection
# path to be what's tested.
(assert (native-fn? parse-int) "rejection subject must be a native fn")
(let [[ok? _] (protect ((fn () (sys/spawn-vm parse-int))))]
  (assert (not ok?) "spawn rejects native function"))

# spawn_wrong_arity
(let [[ok? _] (protect ((fn () (eval '(spawn)))))]
  (assert (not ok?) "spawn wrong arity: no args"))

(let [[ok? _] (protect ((fn () (eval '(sys/spawn-vm (fn () 1) 2)))))]
  (assert (not ok?) "spawn wrong arity: two args"))

# join_wrong_arity
(let [[ok? _] (protect ((fn () (eval '(sys/join)))))]
  (assert (not ok?) "join wrong arity: no args"))

(let [[ok? _] (protect ((fn () (eval '(sys/join 1 2)))))]
  (assert (not ok?) "join wrong arity: two args"))

# join_invalid_argument
(let [[ok? _] (protect ((fn () (sys/join 42))))]
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
             (sys/join (sys/spawn-vm (fn () (add1 41))))) 42)
        "spawn closure capturing closure")

(assert (= (let [add1 (fn (x) (+ x 1))]
             (let [add2 (fn (x) (add1 (add1 x)))]
               (sys/join (sys/spawn-vm (fn () (add2 40)))))) 42)
        "spawn closure capturing nested closures")

(assert (= (let [f (sys/join (sys/spawn-vm (fn () (fn (x) (* x 2)))))]
             (f 21)) 42) "spawn closure returning closure")

(assert (= (let [offset 10]
             (let [add-offset (fn (x) (+ x offset))]
               (sys/join (sys/spawn-vm (fn () (add-offset 32)))))) 42)
        "spawn closure capturing closure and data")

(let [[ok? result] (protect ((fn ()
                               (let [t (@struct :x 42)]
                                 (let [f (fn () (t :x))]
                                   (sys/join (sys/spawn-vm (fn () (f)))))))))]
  (assert ok? "spawn sends closure capturing closure with @struct")
  (assert (= result 42) "spawned @struct through closure preserves data"))

# ============================================================================
# Cross-thread trait survival
# ============================================================================

# User-attached traits survive cross-thread send
(begin
  (def v (with-traits [1 2 3] {:tag :my-type}))
  (def result (sys/join (sys/spawn-vm (fn [] (traits v)))))
  (assert (not (nil? result)) "user traits survive cross-thread send")
  (assert (= (result :tag) :my-type) "user trait data preserved across threads"))

# Default traits are re-stamped on the receiving thread
(begin
  (def v [10 20 30])
  (def result (sys/join (sys/spawn-vm (fn [] (first v)))))
  (assert (= result 10) "default trait dispatch works across threads"))

# ============================================================================
# Recursive closure tests (letrec)
# ============================================================================

(assert (= (letrec [fact (fn (n)
                           (if (= n 0)
                             1
                             (* n (fact (- n 1)))))]
             (sys/join (sys/spawn-vm (fn () (fact 6))))) 720)
        "spawn self-recursive closure")

(assert (= (letrec [even? (fn (n) (if (= n 0) true (odd? (- n 1))))
                    odd? (fn (n) (if (= n 0) false (even? (- n 1))))]
             (sys/join (sys/spawn-vm (fn () (even? 10))))) true)
        "spawn mutually recursive closures")

(assert (= (letrec [even? (fn (n) (if (= n 0) true (odd? (- n 1))))
                    odd? (fn (n) (if (= n 0) false (even? (- n 1))))]
             (sys/join (sys/spawn-vm (fn () (odd? 99))))) true)
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
               (sys/join (sys/spawn-vm (fn () (loop 100 0)))))) 10100)
        "spawn hot loop with captured closure (JIT on worker thread)")

(assert (= (let [inc (fn (x) (+ x 1))
                 sq (fn (x) (* x x))]
             (letrec [loop (fn (n acc)
                             (if (= n 0)
                               acc
                               (loop (- n 1) (+ acc (sq (inc n))))))]
               (sys/join (sys/spawn-vm (fn () (loop 50 0)))))) 45525)
        "spawn hot loop with two captured closures")

(assert (= (let [compose (fn (f g) (fn (x) (f (g x))))]
             (let [inc (fn (x) (+ x 1))
                   dbl (fn (x) (* x 2))]
               (let [f (compose dbl inc)]
                 (letrec [loop (fn (n acc)
                                 (if (= n 0)
                                   acc
                                   (loop (- n 1) (+ acc (f n)))))]
                   (sys/join (sys/spawn-vm (fn () (loop 100 0)))))))) 10300)
        "spawn hot loop with composed closures")

