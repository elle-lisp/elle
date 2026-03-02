#!/usr/bin/env elle

# Coroutines — generators, delegation, and cooperative multitasking
#
# Demonstrates:
#   coro/new, coro/resume   — creation and stepping
#   yield                   — suspending with a value
#   coro/status, coro/done? — lifecycle tracking
#   coro/value              — last yielded value
#   coro?                   — type predicate
#   Closure captures        — independent generator instances
#   Interleaving            — multiple coroutines with independent state
#   Nested coroutines       — inner coroutine driven by outer
#   yield*                  — delegation to a sub-coroutine
#   forever + var/set       — infinite generators with mutable state

(import-file "./examples/assertions.lisp")


# ========================================
# 1. Basic creation, yield, resume
# ========================================

# coro/new wraps a zero-arg function into a coroutine.
# coro/resume steps it forward; yield suspends and returns a value.
(def co (coro/new (fn [] (yield 42))))
(assert-true (coro? co) "coro/new returns a coroutine")
(assert-eq (coro/status co) :created "initial status is :created")

(def v (coro/resume co))
(display "  first resume: ") (print v)
(assert-eq v 42 "first resume returns yielded value")
(assert-eq (coro/status co) :suspended "status after yield is :suspended")
(assert-false (coro/done? co) "not done while suspended")

(coro/resume co)
(assert-eq (coro/status co) :done "status after body completes is :done")
(assert-true (coro/done? co) "done after completion")


# ========================================
# 2. Lifecycle and coro/value
# ========================================

# coro/value returns the most recently yielded value without resuming.
(def co2 (coro/new (fn [] (yield 10) (yield 20) (yield 30))))

(coro/resume co2)
(display "  after 1st yield, value: ") (print (coro/value co2))
(assert-eq (coro/value co2) 10 "value after first yield")

(coro/resume co2)
(assert-eq (coro/value co2) 20 "value after second yield")

(coro/resume co2)
(assert-eq (coro/value co2) 30 "value after third yield")
(assert-eq (coro/status co2) :suspended "still suspended after final yield")


# ========================================
# 3. Expressions in yield
# ========================================

# yield evaluates its argument before suspending.
(def co3 (coro/new (fn []
  (yield (+ 1 2 3))
  (yield (* 4 5))
  (yield (if true 100 200)))))

(assert-eq (coro/resume co3) 6 "yield sum")
(assert-eq (coro/resume co3) 20 "yield product")
(assert-eq (coro/resume co3) 100 "yield conditional")
(display "  (+ 1 2 3)=") (display 6)
  (display "  (* 4 5)=") (display 20)
  (display "  (if true 100 200)=") (print 100)


# ========================================
# 4. Fibonacci generator
# ========================================

# The real thing: an infinite generator using mutable state.
# Each resume yields the next Fibonacci number.
(defn make-fib []
  "Return a coroutine that generates the Fibonacci sequence."
  (coro/new (fn []
    (var a 0)
    (var b 1)
    (forever
      (yield a)
      (def next (+ a b))
      (set a b)
      (set b next)))))

(def fib (make-fib))
(display "  fib: ")
(display (coro/resume fib)) (display " ")
(display (coro/resume fib)) (display " ")
(display (coro/resume fib)) (display " ")
(display (coro/resume fib)) (display " ")
(display (coro/resume fib)) (display " ")
(display (coro/resume fib)) (display " ")
(display (coro/resume fib)) (display " ")
(display (coro/resume fib)) (display " ")
(display (coro/resume fib)) (display " ")
(print (coro/resume fib))
# 0 1 1 2 3 5 8 13 21 34

# Verify the sequence
(def fib2 (make-fib))
(assert-eq (coro/resume fib2) 0 "fib(0)")
(assert-eq (coro/resume fib2) 1 "fib(1)")
(assert-eq (coro/resume fib2) 1 "fib(2)")
(assert-eq (coro/resume fib2) 2 "fib(3)")
(assert-eq (coro/resume fib2) 3 "fib(4)")
(assert-eq (coro/resume fib2) 5 "fib(5)")
(assert-eq (coro/resume fib2) 8 "fib(6)")
(assert-eq (coro/resume fib2) 13 "fib(7)")
(assert-eq (coro/resume fib2) 21 "fib(8)")
(assert-eq (coro/resume fib2) 34 "fib(9)")
(assert-false (coro/done? fib2) "infinite generator never done")


# ========================================
# 5. Closure captures — factory pattern
# ========================================

# A factory function creates independent generators with captured state.
(defn make-counter [start]
  "Return a coroutine counting up from start."
  (coro/new (fn []
    (var n start)
    (forever
      (yield n)
      (set n (+ n 1))))))

(def from-10 (make-counter 10))
(def from-99 (make-counter 99))

(assert-eq (coro/resume from-10) 10 "counter from 10")
(assert-eq (coro/resume from-99) 99 "counter from 99")
(assert-eq (coro/resume from-10) 11 "counter from 10, step 2")
(assert-eq (coro/resume from-99) 100 "counter from 99, step 2")
(display "  from-10: 10, 11  from-99: 99, 100") (print "")


# ========================================
# 6. Interleaving — independent state
# ========================================

# Two coroutines resumed in alternation maintain independent state.
(def odds (coro/new (fn [] (yield 1) (yield 3) (yield 5))))
(def evens (coro/new (fn [] (yield 2) (yield 4) (yield 6))))

(display "  interleaved: ")
(display (coro/resume odds)) (display " ")
(display (coro/resume evens)) (display " ")
(display (coro/resume odds)) (display " ")
(display (coro/resume evens)) (display " ")
(display (coro/resume odds)) (display " ")
(print (coro/resume evens))
# 1 2 3 4 5 6

(assert-eq (coro/status odds) :suspended "odds still suspended")
(assert-eq (coro/status evens) :suspended "evens still suspended")


# ========================================
# 7. Nested coroutines
# ========================================

# An outer coroutine drives an inner one, yielding its results.
(def outer (coro/new (fn []
  (var inner (coro/new (fn [] (yield 100) (yield 200))))
  (yield (coro/resume inner))
  (yield (coro/resume inner))
  (yield 300))))

(assert-eq (coro/resume outer) 100 "nested: inner first")
(assert-eq (coro/resume outer) 200 "nested: inner second")
(assert-eq (coro/resume outer) 300 "nested: outer continues")
(display "  nested: 100 200 300") (print "")


# ========================================
# 8. coro? type predicate
# ========================================

(assert-true (coro? (coro/new (fn [] (yield 1)))) "coroutine is coro?")
(assert-false (coro? 42) "int is not coro?")
(assert-false (coro? (fn [] 1)) "function is not coro?")
(assert-false (coro? nil) "nil is not coro?")
(assert-false (coro? '()) "empty list is not coro?")


# ========================================
# 9. yield* delegation
# ========================================

# yield* delegates to a sub-coroutine: the outer coroutine yields
# each value from the sub-coroutine, then continues its own body.
(def sub (coro/new (fn [] (yield 10) (yield 20))))
(def main (coro/new (fn [] (yield* sub) (yield 30))))

(display "  delegated: ")
(display (coro/resume main)) (display " ")
(display (coro/resume main)) (display " ")
(print (coro/resume main))
# 10 20 30

(assert-eq (coro/status main) :suspended "main suspended after final yield")

# Verify values came through correctly
(def sub2 (coro/new (fn [] (yield :a) (yield :b))))
(def main2 (coro/new (fn [] (yield* sub2) (yield :c))))
(assert-eq (coro/resume main2) :a "delegated first")
(assert-eq (coro/resume main2) :b "delegated second")
(assert-eq (coro/resume main2) :c "own yield after delegation")


(print "")
(print "all coroutines passed.")
