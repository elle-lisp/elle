#!/usr/bin/env elle

# Fiber fuel — instruction budgets for cooperative scheduling
#
# Demonstrates:
#   fiber/set-fuel     — set an instruction budget on a fiber
#   fiber/fuel         — read remaining budget (integer or nil)
#   fiber/clear-fuel   — remove the budget, restoring unlimited execution
#   |:fuel| mask       — fibers that surface :fuel signals to the parent
#   Round-robin scheduler — the motivating use case: time-sliced execution
## ── Basic fuel exhaustion ──────────────────────────────────────────

# A fiber counting from 0 upward. With a small budget it runs partway,
# then pauses on a :fuel signal so the caller can decide what to do next.
#
# Note: functional accumulator style (acc passed as argument) so state
# is preserved correctly across fuel-pause resumes.
(defn make-sum-fiber [limit]
  "Return a fiber that sums 0..limit-1 using a functional loop."
  (fiber/new
    (fn []
      (letrec ((loop (fn (i acc)
                  (if (< i limit)
                    (loop (+ i 1) (+ acc i))
                    acc))))
        (loop 0 0)))
    |:fuel|))

(def f1 (make-sum-fiber 1000))

# Set a small fuel budget — not enough to finish.
(fiber/set-fuel f1 50)
(println "  fuel before first resume: " (fiber/fuel f1))
(assert (= (fiber/fuel f1) 50) "fuel is 50 before resume")

# Resume: runs until fuel is exhausted, then pauses.
(fiber/resume f1)
(println "  status after fuel exhaustion: " (fiber/status f1))
(assert (= (fiber/status f1) :paused) "fiber pauses when fuel runs out")

# After pausing on :fuel, the remaining budget is 0.
(println "  fuel after exhaustion: " (fiber/fuel f1))
(assert (= (fiber/fuel f1) 0) "fuel reads 0 after exhaustion")
## ── Refuel and resume to completion ────────────────────────────────

# Refuel with a generous budget and let the fiber run to completion.
(fiber/set-fuel f1 100000)
(println "  fuel after refuel: " (fiber/fuel f1))
(assert (= (fiber/fuel f1) 100000) "fuel is 100000 after refuel")

(fiber/resume f1)
(println "  status after completion: " (fiber/status f1))
(assert (= (fiber/status f1) :dead) "fiber reaches :dead after completion")

# The return value (sum of 0..999 = 499500) is accessible via fiber/value.
(def total (fiber/value f1))
(println "  sum 0..999: " total)
(assert (= total 499500) "sum of 0..999 is 499500")
## ── Unlimited fiber ────────────────────────────────────────────────

# A fiber with no fuel limit runs to completion in a single resume.
(def f2 (fiber/new
  (fn []
    (letrec ((loop (fn (i acc)
                (if (< i 100)
                  (loop (+ i 1) (+ acc i))
                  acc))))
      (loop 0 0)))
  |:fuel|))

# fiber/fuel returns nil when no budget is set.
(println "  fuel on unlimited fiber: " (fiber/fuel f2))
(assert (nil? (fiber/fuel f2)) "unlimited fiber has nil fuel")

(fiber/resume f2)
(println "  status after single resume: " (fiber/status f2))
(assert (= (fiber/status f2) :dead) "unlimited fiber runs to :dead in one resume")
(assert (= (fiber/value f2) 4950) "sum 0..99 = 4950")
## ── Round-robin scheduler ──────────────────────────────────────────

# The motivating use case: run N fibers concurrently, giving each a time
# slice, cycling until all are done. Each fiber does independent work and
# they interleave transparently — no explicit yield needed.
#
# Slice size: 50 instructions. Each fiber does 70 iterations of 1-instruction
# tail calls, so each fiber pauses once and then completes on the second
# slice — two rounds of scheduling.
#
# Only use 2-argument arithmetic in the fiber loops. Two-argument arithmetic
# compiles to opcodes (free); only tail calls to closures consume fuel.

# run-timeslice gives a fiber one time slice and returns whether it's still running.
(defn run-timeslice [fiber slice]
  "Give fiber a slice-sized fuel budget and resume it once.
  Returns true if still running, false if done."
  (fiber/set-fuel fiber slice)
  (fiber/resume fiber)
  (= (fiber/status fiber) :paused))

# Three independent worker fibers computing different sums over 1..70.
(var sum-fiber (fiber/new      # sum 1..70 = 2485
  (fn []
    (letrec ((loop (fn (i acc)
                (if (<= i 70)
                  (loop (+ i 1) (+ acc i))
                  acc))))
      (loop 1 0)))
  |:fuel|))

(var squares-fiber (fiber/new  # sum of squares 1..70 = 116795
  (fn []
    (letrec ((loop (fn (i acc)
                (if (<= i 70)
                  (loop (+ i 1) (+ acc (* i i)))
                  acc))))
      (loop 1 0)))
  |:fuel|))

(var doubles-fiber (fiber/new  # sum of doubles 1..70 = 4970
  (fn []
    (letrec ((loop (fn (i acc)
                (if (<= i 70)
                  (loop (+ i 1) (+ acc (* i 2)))
                  acc))))
      (loop 1 0)))
  |:fuel|))

(println "  running 3 fibers round-robin..." "")

# Simple round-robin: maintain an active list, give each fiber one slice per round.
(var active @[sum-fiber squares-fiber doubles-fiber])
(var rr-results @[])
(var round-count 0)

(forever
  (when (= (length active) 0) (break))
  (assign round-count (+ round-count 1))
  (var next-active @[])
  (var i 0)
  (forever
    (when (>= i (length active)) (break))
    (def f (get active i))
    (if (run-timeslice f 50)
      (push next-active f)
      (push rr-results (fiber/value f)))
    (assign i (+ i 1)))
  (assign active next-active))

(println "  completed in rounds: " round-count)
(println "  sum 1..70: " (get rr-results 0))
(println "  sum of squares: " (get rr-results 1))
(println "  sum of doubles: " (get rr-results 2))

# All 3 fibers interleaved across exactly 2 scheduling rounds.
(assert (= round-count 2) "3 fibers complete in 2 rounds with slice=50")
(assert (= (get rr-results 0) 2485) "sum 1..70 = 2485")
(assert (= (get rr-results 1) 116795) "sum of squares 1..70 = 116795")
(assert (= (get rr-results 2) 4970) "sum of doubles 1..70 = 4970")

(assert (= (fiber/status sum-fiber) :dead) "sum-fiber is :dead")
(assert (= (fiber/status squares-fiber) :dead) "squares-fiber is :dead")
(assert (= (fiber/status doubles-fiber) :dead) "doubles-fiber is :dead")
## ── fiber/clear-fuel ───────────────────────────────────────────────

# A paused fiber can have its fuel limit removed entirely.
# After clear-fuel, the next resume runs to completion without interruption.
(def f3 (make-sum-fiber 200))
(fiber/set-fuel f3 20)
(fiber/resume f3)
(assert (= (fiber/status f3) :paused) "f3 pauses on small fuel budget")

# Remove the fuel limit — next resume runs all the way to :dead.
(fiber/clear-fuel f3)
(println "  fuel after clear-fuel: " (fiber/fuel f3))
(assert (nil? (fiber/fuel f3)) "fuel is nil after clear-fuel")

(fiber/resume f3)
(println "  status after clear-fuel resume: " (fiber/status f3))
(assert (= (fiber/status f3) :dead) "fiber runs to :dead after clear-fuel")

# sum of 0..199 = 199*200/2 = 19900
(assert (= (fiber/value f3) 19900) "sum 0..199 = 19900 after clear-fuel resume")


(println "")
(println "all fuel tests passed.")
