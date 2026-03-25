#!/usr/bin/env elle

# Concurrency — structured concurrency with fibers and OS threads
#
# Demonstrates:
#   Fibers (async)       — ev/spawn, ev/join, ev/join-protected
#   Structured patterns  — ev/select, ev/race, ev/timeout, ev/scope, ev/map
#   Bounded concurrency  — ev/map-limited, ev/as-completed
#   Abort / cancel       — ev/abort
#   OS threads           — spawn, join (for CPU-bound parallel work)
#
# Fibers are cooperative (single-threaded, no data races) and run under
# the async scheduler.  OS threads are preemptive (multi-core) but cannot
# share mutable state.


# ========================================
# 1. ev/spawn + ev/join — basic fiber concurrency
# ========================================

# ev/spawn creates a fiber; ev/join waits for its result.
(let ([f (ev/spawn (fn [] (* 6 7)))])
  (let ([result (ev/join f)])
    (print "  spawn + join: ") (println result)
    (assert (= result 42) "spawn/join computes 6*7")))

# Join a sequence of fibers — results in input order.
(let ([fibers [(ev/spawn (fn [] :a))
               (ev/spawn (fn [] :b))
               (ev/spawn (fn [] :c))]])
  (let ([results (ev/join fibers)])
    (print "  join sequence: ") (println results)
    (assert (= results [:a :b :c]) "join sequence returns ordered results")))


# ========================================
# 2. ev/join-protected — error handling without crashing
# ========================================

# ev/join-protected returns [ok? value] — like protect but async-aware.
(let (([ok? val] (ev/join-protected (ev/spawn (fn [] 42)))))
  (print "  join-protected success: ") (println [ok? val])
  (assert ok? "success returns true")
  (assert (= 42 val) "success returns value"))

(let (([ok? val] (ev/join-protected (ev/spawn (fn [] (error "oops"))))))
  (print "  join-protected failure: ") (println [ok? val])
  (assert (not ok?) "failure returns false"))

# Sequence variant: joins all, never short-circuits.
(let ([results (ev/join-protected [(ev/spawn (fn [] 1))
                                    (ev/spawn (fn [] (error "fail")))
                                    (ev/spawn (fn [] 3))])])
  (print "  join-protected sequence: ") (println results)
  (assert (= true (get (get results 0) 0)) "first succeeds")
  (assert (= false (get (get results 1) 0)) "second fails")
  (assert (= true (get (get results 2) 0)) "third succeeds"))


# ========================================
# 3. ev/select — wait for the first of N
# ========================================

# Returns [completed-fiber remaining-fibers].
(let ([fast (ev/spawn (fn [] :fast))]
      [slow (ev/spawn (fn [] (ev/sleep 10) :slow))])
  (let (([done remaining] (ev/select [fast slow])))
    (print "  select winner: ") (println (ev/join done))
    (assert (= (ev/join done) :fast) "fast fiber wins")
    (assert (= 1 (length remaining)) "one loser remains")
    (each f in remaining (ev/abort f))))


# ========================================
# 4. ev/race — first wins, rest aborted
# ========================================

(let ([result (ev/race [(ev/spawn (fn [] :winner))
                         (ev/spawn (fn [] (ev/sleep 10) :loser))])])
  (print "  race winner: ") (println result)
  (assert (= :winner result) "race returns winner's value"))


# ========================================
# 5. ev/timeout — deadline on a computation
# ========================================

# Fast work finishes before timeout.
(let ([result (ev/timeout 10 (fn [] 42))])
  (print "  timeout (fast): ") (println result)
  (assert (= 42 result) "fast work returns value"))

# Slow work exceeds timeout.
(let (([ok? val] (protect (ev/timeout 0.01 (fn [] (ev/sleep 100))))))
  (print "  timeout (slow): ") (println [ok? val])
  (assert (not ok?) "timeout fires")
  (assert (= :timeout (get val :error)) "error is :timeout"))


# ========================================
# 6. ev/abort — graceful fiber cancellation
# ========================================

# Abort a sleeping fiber; defer blocks run.
# Yield first (ev/sleep 0) to let the spawned fiber start — a :new fiber
# has no defer blocks to run, so abort sets :error directly.
(let* ([cleaned @[false]]
       [f (ev/spawn (fn []
             (defer (put cleaned 0 true)
               (ev/sleep 100))))])
  (ev/sleep 0)
  (ev/abort f)
  (assert (= :error (fiber/status f)) "aborted fiber is :error")
  (assert (get cleaned 0) "defer block ran during abort"))

# Abort on already-dead fiber is a no-op.
(let ([f (ev/spawn (fn [] 42))])
  (ev/join f)
  (ev/abort f)
  (assert (= :dead (fiber/status f)) "abort on dead fiber is no-op"))


# ========================================
# 7. ev/scope — structured concurrency nursery
# ========================================

# All children must complete before scope exits.
(let ([result (ev/scope (fn [spawn]
                (let ([a (spawn (fn [] 10))]
                      [b (spawn (fn [] 20))])
                  (+ (ev/join a) (ev/join b)))))])
  (print "  scope result: ") (println result)
  (assert (= 30 result) "scope joins children and returns body result"))

# On error, remaining siblings are aborted.
(let (([ok? val] (protect (ev/scope (fn [spawn]
    (spawn (fn [] (ev/sleep 100)))
    (spawn (fn [] (error "child-error")))
    (ev/sleep 100))))))
  (print "  scope error: ") (println [ok? val])
  (assert (not ok?) "scope propagates first error"))


# ========================================
# 8. ev/map — parallel map
# ========================================

(let ([squares (ev/map (fn [x] (* x x)) [1 2 3 4 5])])
  (print "  ev/map squares: ") (println squares)
  (assert (= [1 4 9 16 25] squares) "ev/map returns ordered results"))


# ========================================
# 9. ev/as-completed — lazy completion iterator
# ========================================

# Process fibers as they finish (order depends on scheduling).
(let ([fibers [(ev/spawn (fn [] :a))
               (ev/spawn (fn [] :b))
               (ev/spawn (fn [] :c))]]
      [collected @[]])
  (let (([next pool] (ev/as-completed fibers)))
    (forever
      (let ([done (next)])
        (when (nil? done) (break nil))
        (push collected (ev/join done)))))
  (print "  as-completed: ") (println collected)
  (assert (= 3 (length collected)) "all three fibers collected"))


# ========================================
# 10. OS threads — preemptive parallelism
# ========================================

# spawn/join create OS threads for CPU-bound work.
# Spawned threads get a fresh VM; closures must capture only sendable values.
(let* ([x 10]
       [y 20]
       [handle (spawn (fn [] (+ x y)))]
       [result (join handle)])
  (print "  OS thread: 10 + 20 = ") (println result)
  (assert (= result 30) "OS thread computes 10+20"))

# Parallel sum across 4 OS threads.
(let* ([t1 (spawn (fn [] (/ (* 25 26) 2)))]
       [t2 (spawn (fn [] (- (/ (* 50 51) 2) (/ (* 25 26) 2))))]
       [t3 (spawn (fn [] (- (/ (* 75 76) 2) (/ (* 50 51) 2))))]
       [t4 (spawn (fn [] (- (/ (* 100 101) 2) (/ (* 75 76) 2))))]
       [total (+ (join t1) (join t2) (join t3) (join t4))])
  (print "  parallel sum 1..100: ") (println total)
  (assert (= total 5050) "parallel sum of 1..100"))


(println "")
(println "all concurrency passed.")
