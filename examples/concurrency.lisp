#!/usr/bin/env elle

# Concurrency — parallel computation with spawn and join
#
# Demonstrates:
#   spawn, join          — create threads and collect results
#   Closure captures     — immutable values cross thread boundaries
#   current-thread-id    — threads have distinct identities
#   Parallel work        — split computation across threads, combine
#
# Note: closures that capture mutable values (tables, arrays) cannot
# be spawned — the runtime rejects non-sendable captures.
# Spawned threads get a fresh VM with primitives only; globals are not
# shared. Values the thread needs must be captured via closure scope.

(import-file "./examples/assertions.lisp")


# ========================================
# 1. Basic spawn/join
# ========================================

# spawn takes a zero-arg closure, runs it on a new OS thread.
# join blocks until the thread finishes and returns the result.
(let* ([x 10]
       [y 20]
       [handle (spawn (fn [] (+ x y)))]
       [result (join handle)])
  (display "  10 + 20 in another thread: ") (print result)
  (assert-eq result 30 "spawn/join computes 10+20"))


# ========================================
# 2. Multiple threads
# ========================================

# Spawn several computations, join all, combine results.
(let* ([h1 (spawn (fn [] (* 2 3)))]
       [h2 (spawn (fn [] (* 4 5)))]
       [h3 (spawn (fn [] (* 6 7)))]
       [r1 (join h1)]
       [r2 (join h2)]
       [r3 (join h3)])
  (display "  products: ") (display r1)
    (display " ") (display r2) (display " ") (print r3)
  (assert-eq r1 6 "thread 1: 2*3")
  (assert-eq r2 20 "thread 2: 4*5")
  (assert-eq r3 42 "thread 3: 6*7")
  (assert-eq (+ r1 r2 r3) 68 "sum of all thread results"))


# ========================================
# 3. Captures
# ========================================

# Spawned closures capture immutable values from the enclosing scope.
# Strings, numbers, tuples, structs — all fine.
(let* ([name "Alice"]
       [greeting (join (spawn (fn []
         (-> "Hello, "
             (append name)
             (append "! You are ")
             (append (string 30))
             (append " years old.")))))])
  (display "  ") (print greeting)
  (assert-true (string/contains? greeting "Alice") "captured string in thread")
  (assert-true (string/contains? greeting "30") "captured number conversion"))

# Tuples are immutable, so they cross thread boundaries.
(let* ([nums [10 20 30]]
       [result (join (spawn (fn []
         (+ (get nums 0) (get nums 1) (get nums 2)))))])
  (display "  sum of [10 20 30]: ") (print result)
  (assert-eq result 60 "tuple elements accessible in thread"))


# ========================================
# 4. Thread IDs
# ========================================

# Each thread has a distinct ID (returned as a string).
(let* ([main-id (current-thread-id)]
       [spawned-id (join (spawn (fn [] (current-thread-id))))])
  (display "  main thread: ") (display main-id)
    (display "  spawned thread: ") (print spawned-id)
  (assert-true (string? main-id) "thread ID is a string")
  (assert-true (string? spawned-id) "spawned thread ID is a string")
  (assert-true (not (= main-id spawned-id)) "threads have distinct IDs"))


# ========================================
# 5. Practical: parallel computation
# ========================================

# Split a computation across threads and combine the results.
# Spawned threads can't call user-defined functions (closures aren't
# sendable), so each thread does its work with primitives and loops.
# Gauss's formula: sum 1..n = n*(n+1)/2, so sum lo..hi = sum(hi) - sum(lo-1).
(let* ([t1 (spawn (fn [] (/ (* 25 26) 2)))]               # sum 1..25 = 325
       [t2 (spawn (fn [] (- (/ (* 50 51) 2)               # sum 26..50
                            (/ (* 25 26) 2))))]
       [t3 (spawn (fn [] (- (/ (* 75 76) 2)               # sum 51..75
                            (/ (* 50 51) 2))))]
       [t4 (spawn (fn [] (- (/ (* 100 101) 2)             # sum 76..100
                            (/ (* 75 76) 2))))]
       [total (+ (join t1) (join t2) (join t3) (join t4))])
  (display "  sum 1..100 across 4 threads: ") (print total)
  (assert-eq total 5050 "parallel sum of 1..100"))


(print "")
(print "all concurrency passed.")
