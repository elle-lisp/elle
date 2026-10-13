(elle/epoch 15)
# audited: 2026-10-13
# A tail call to a variadic callee costs one pass over its arguments, read against the same call in non-tail position.
# docs/regions/performance.md
# docs/testing.md
#
# The counter-factual: counting each value's occurrences once per rest
# argument, rescanning the whole list each time, reads about 40 times the
# control at 4000 arguments, and the ratio grows with the count. One pass
# reads about 1. A spliced call such as `(apply sink xs)` never reaches the
# count, because it moves nothing, so the macro below writes the call out.
#
# The trap: the fastest of nine alternating wall-clock timings still read a
# ratio of 5.6 on a shared runner, from a child that ran on a CPU for 1.4 s of
# its 2.5 s. The wait landed in every tail timing and missed at least one
# non-tail timing. So both calls are timed on the thread's CPU clock, which the
# wait does not advance.

(def r ((import "std/ratchet")))

(def n 4000)
(def samples 9)

(defn sink [& xs]
  (length xs))

# `(sink x x … x)` with 4000 copies of X: a plain call, so in tail position it
# is a `TailCall` whose arguments the caller moves to the callee.
(defmacro sink-call (x)
  (let [@xs ()]
    (repeat 4000 (assign xs (pair x xs)))
    `(sink ,;xs)))

(defn call-tail [x]
  (sink-call x))

# `+` forces the result into an argument position, so the call is not in
# tail position and the callee takes the owned-parameter path.
(defn call-nontail [x]
  (+ 0 (sink-call x)))

(defn run [f]
  (fn [] (assert (= (f (bytes 1 2 3 4)) n) "the callee received every argument")))

(def tail-run (run call-tail))
(def nontail-run (run call-nontail))

# Warm up both paths so neither measurement pays first-call compilation.
(tail-run)
(nontail-run)

(def [control tail] (r:best-of samples nontail-run tail-run))

(defn us [seconds]
  (round (* seconds 1000000.0)))

(println "apply-tail-linear: n=" n ", the fastest of " samples ": non-tail "
         (us control) " us, tail " (us tail) " us")

(assert (< tail (* 4.0 (max control 0.00001)))
        (concat "a tail call must cost one pass over its arguments; its fastest run took "
                (string (us tail)) " us against the non-tail "
                (string (us control)) " us for the same work"))
