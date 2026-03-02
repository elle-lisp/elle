#!/usr/bin/env elle

# Introspection — looking inside the machine
#
# Demonstrates:
#   Clock primitives   — clock/monotonic, clock/realtime, clock/cpu
#   Timing             — time/elapsed, time/stopwatch
#   Closure inspection — closure?, pure?, arity, captures, bytecode-size
#   Disassembly        — disbit (bytecode), disjit (Cranelift IR)
#   Debug utilities    — debug-print, trace
#   Micro-benchmarking — timing loops with clock/monotonic

(import-file "./examples/assertions.lisp")


# ========================================
# 1. Clock primitives
# ========================================

# Three clocks, three purposes: wall time, epoch time, CPU time.
(var mono1 (clock/monotonic))
(var mono2 (clock/monotonic))
(assert-true (number? mono1) "clock/monotonic returns a number")
(assert-true (>= mono2 mono1) "clock/monotonic is non-decreasing")
(display "  monotonic: ") (print mono1)

(var epoch (clock/realtime))
(assert-true (number? epoch) "clock/realtime returns a number")
(assert-true (> epoch 1700000000.0) "clock/realtime is a plausible epoch")
(display "  realtime (epoch): ") (print epoch)

(var cpu (clock/cpu))
(assert-true (number? cpu) "clock/cpu returns a number")
(assert-true (>= cpu 0.0) "clock/cpu is non-negative")
(display "  cpu time: ") (print cpu)


# ========================================
# 2. Timing
# ========================================

# time/elapsed wraps a thunk, returns (result elapsed-seconds).
(var elapsed-result (time/elapsed (fn [] (+ 21 21))))
(assert-true (list? elapsed-result) "time/elapsed returns a list")
(assert-eq (first elapsed-result) 42 "time/elapsed preserves return value")
(var elapsed-secs (first (rest elapsed-result)))
(assert-true (number? elapsed-secs) "elapsed time is a number")
(assert-true (>= elapsed-secs 0.0) "elapsed time is non-negative")
(display "  (+ 21 21) took ") (display elapsed-secs) (print " seconds")

# time/stopwatch is a coroutine that yields elapsed seconds on each resume.
(var sw (time/stopwatch))
(assert-true (coro? sw) "time/stopwatch returns a coroutine")
(var t-first (coro/resume sw))
(var t-second (coro/resume sw))
(assert-true (number? t-first) "stopwatch sample is a number")
(assert-true (>= t-second t-first) "stopwatch samples are non-decreasing")
(display "  stopwatch: ") (display t-first) (display " → ") (print t-second)


# ========================================
# 3. Closure introspection
# ========================================

# Define some functions to inspect.
(defn add [x y]
  "Add two numbers."
  (+ x y))

(defn identity-fn [x]
  "Return the argument unchanged."
  x)

(defn make-adder [n]
  "Return a closure that adds n to its argument."
  (fn [x] (+ x n)))

(var add5 (make-adder 5))

# closure? — true for user-defined functions, false for primitives and non-fns
(assert-true (closure? add) "defn produces a closure")
(assert-true (closure? identity-fn) "identity-fn is a closure")
(assert-true (closure? add5) "returned lambda is a closure")
(assert-false (closure? +) "+ is a primitive, not a closure")
(assert-false (closure? 42) "42 is not a closure")
(display "  closure?: add=") (display (closure? add))
(display " +=") (print (closure? +))

# pure? — true for closures with no side effects
(assert-true (pure? add) "add is pure")
(assert-true (pure? identity-fn) "identity-fn is pure")
(display "  pure?: add=") (print (pure? add))

# arity — number of parameters (nil for non-closures)
(assert-eq (arity add) 2 "add has arity 2")
(assert-eq (arity identity-fn) 1 "identity-fn has arity 1")
(assert-eq (arity add5) 1 "add5 has arity 1")
(assert-eq (arity 42) nil "non-closure arity is nil")
(display "  arity: add=") (display (arity add))
(display " identity=") (display (arity identity-fn))
(display " add5=") (print (arity add5))

# captures — number of captured variables
(assert-eq (captures add) 0 "add captures nothing")
(assert-eq (captures add5) 1 "add5 captures one variable")
(display "  captures: add=") (display (captures add))
(display " add5=") (print (captures add5))

# bytecode-size — bytecode length in bytes
(assert-true (> (bytecode-size add) 0) "add has bytecode")
(assert-eq (bytecode-size 42) nil "non-closure bytecode-size is nil")
(display "  bytecode-size: add=") (print (bytecode-size add))

# disbit — bytecode disassembly (returns array of strings)
(var disasm (disbit add))
(assert-true (> (length disasm) 0) "disbit returns non-empty result")
(assert-true (string? (get disasm 0)) "disbit elements are strings")
(display "  disbit add (") (display (length disasm)) (print " instructions):")
(each line in disasm
  (display "    ") (print line))

# disjit — Cranelift IR (nil if LIR not stored for this function)
(var jit (disjit add))
(assert-true (or (nil? jit)
                 (and (> (length jit) 0)
                      (string? (get jit 0))))
             "disjit returns nil or array of strings")
(display "  disjit add: ") (print (if (nil? jit) "nil (no LIR)" "available"))


# ========================================
# 4. Debug utilities
# ========================================

# debug-print writes to stderr — won't appear in captured stdout,
# but proves the primitive exists and doesn't crash.
(debug-print "introspection.lisp: debug-print works")
(debug-print (string/join (list "add5(10) = " (string (add5 10))) ""))
(display "  debug-print: ok (output on stderr)") (print "")

# trace wraps an expression, prints debug info to stderr, returns the value.
(var traced (trace "add" (add 3 4)))
(assert-eq traced 7 "trace preserves return value")
(display "  trace: (add 3 4) = ") (print traced)


# ========================================
# 5. Micro-benchmark pattern
# ========================================

# A simple bench loop: run a thunk N times, report ns/call.
(defn bench [label n thunk]
  "Run thunk n times, print and return nanoseconds per call."
  (var t0 (clock/monotonic))
  (var i 0)
  (while (< i n)
    (begin (thunk) (set i (+ i 1))))
  (var elapsed (- (clock/monotonic) t0))
  (var ns-per (/ (* elapsed 1000000000) n))
  (display "  ") (display label) (display ": ")
  (display ns-per) (print " ns/call")
  ns-per)

(var iters 500)

(var ns-mono (bench "clock/monotonic" iters (fn [] (clock/monotonic))))
(assert-true (>= ns-mono 0.0) "monotonic bench is non-negative")

(var ns-add (bench "add(1, 2)      " iters (fn [] (add 1 2))))
(assert-true (>= ns-add 0.0) "add bench is non-negative")

(var ns-elapsed (bench "time/elapsed   " iters (fn [] (time/elapsed (fn [] 42)))))
(assert-true (>= ns-elapsed 0.0) "elapsed bench is non-negative")


(print "")
(print "all introspection passed.")
