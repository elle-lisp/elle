(elle/epoch 12)
# Regression: an argument value must survive a later argument's inline loop.
#
# The bytecode VM is stack-based (src/lir/emit/): a call parks each already-lowered
# argument on the operand stack while it lowers the next. A `while`-loop as a LATER
# argument lowers a back-edge, and re-entering the loop head resets the operand
# stack to its head layout — which does NOT include an earlier argument value
# parked below the loop's working set. Before the fix that earlier argument was
# dropped and the call read whatever sat in its slot (a stray closure), so a call
# like `(m2 (fn [x] …) (let* […] (while …) …))` raised `:arity-error` /
# `:type-error` or computed a garbage value — on VM AND JIT, from pure user code.
# The fix (src/lir/lower/control/call.rs, `spill_across_loop`) spills every
# argument to a local when a later one contains a loop, reloading them adjacent to
# the call. `if`/`begin` are forward merges that already carried the stack across,
# so only a back-edge loop needed this.
#
# `range!` is the offending shape written INLINE at the call site (a helper `defn`
# would move the loop into its own function body, where it never touches the
# caller's operand stack — so it would not reproduce the bug). It builds [lo..hi).

(defmacro range! [lo hi]
  `(let* [a @[]]
     (def @i ,lo)
     (while (< i ,hi)
       (push a i)
       (assign i (+ i 1)))
     (freeze a)))

# The array itself is correct on its own (baseline: the loop is fine in isolation).
(assert (= (range! 3 5) [3 4]) "inline loop builds the array")

# A LAMBDA argument before a loop argument, then CALLED (the original trigger:
# the lambda was mis-read as an arity-2 closure).
(defn apply-first [f a]
  (f (get a 0)))
(assert (= (apply-first (fn [x] (* x 10)) (range! 3 5)) 30)
        "lambda arg survives a later loop arg and is called with correct arity")

# An IMMEDIATE argument before a loop argument, then USED (immediates are not
# local-backed, so an emitter-only reload could not have covered this).
(defn add-first [n a]
  (+ n (get a 0)))
(assert (= (add-first 100 (range! 3 5)) 103)
        "immediate arg survives a later loop arg")

# A HEAP-string argument before a loop argument.
(defn cat-first [s a]
  (string s (get a 0)))
(assert (= (cat-first "x" (range! 3 5)) "x3")
        "heap-string arg survives a later loop arg")

# Three arguments with the loop in the MIDDLE: the first arg must cross the loop,
# and the third (lowered after the loop) must not disturb the reloaded first.
(defn pick [x a y]
  (+ x (get a 0) y))
(assert (= (pick 1 (range! 3 5) 2) 6) "first arg survives a mid loop arg")

# The loop as the FIRST argument (value second) already worked — the value is
# pushed above the settled loop result — but pin it so the spill gate does not
# regress it.
(defn sum-first [a n]
  (+ (get a 0) n))
(assert (= (sum-first (range! 3 5) 7) 10)
        "loop-first, value-second stays correct")

# A loop nested inside another argument expression (a wrapper call and an `if`):
# the loop still resets the caller's operand stack, so the earlier arg needs the
# same protection.
(assert (= (apply-first (fn [x] (* x 10)) (identity (range! 3 5))) 30)
        "loop wrapped in a call arg still protects the earlier arg")
(assert (= (apply-first (fn [x] (+ x 1)) (if true (range! 3 5) [0 0])) 4)
        "loop inside an if-arm still protects the earlier arg")

# Nested calls each with the pattern.
(assert (= (apply-first (fn [x] (* x 2)) (range! (get (range! 3 5) 0) 6)) 6)
        "nested loop-args each protect their siblings")

(println "call-arg-across-loop: ok")
