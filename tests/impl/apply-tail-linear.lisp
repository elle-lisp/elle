(elle/epoch 14)
# audited: 2026-10-06
# A tail call to a variadic callee costs one pass over its arguments, read against the same call in non-tail position.
# docs/regions/performance.md
#
# The counter-factual: counting each value's occurrences once per rest
# argument, rescanning the whole list each time, reads about 40 times the
# control at 4000 arguments, and the ratio grows with the count. One pass
# reads about 1. A spliced call such as `(apply sink xs)` never reaches the
# count, because it moves nothing, so the macro below writes the call out.
#
# The trap: one timing of each call on a shared runner read a ratio of 7 for
# the same work, when a stall slowed the tail call alone. A stall slows a
# timing and never speeds one up, so each call is timed several times,
# alternating with the other, and the fastest timing of each is compared.

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

(defn timed [f]
  (let [x (bytes 1 2 3 4)
        t0 (clock/monotonic)
        r (f x)]
    (assert (= r n) "the callee received every argument")
    (- (clock/monotonic) t0)))

# Warm up both paths so neither measurement pays first-call compilation.
(timed call-tail)
(timed call-nontail)

(def @control (timed call-nontail))
(def @tail (timed call-tail))
(repeat (- samples 1) (assign control (min control (timed call-nontail)))
        (assign tail (min tail (timed call-tail))))

(defn us [seconds]
  (round (* seconds 1000000.0)))

(println "apply-tail-linear: n=" n ", the fastest of " samples ": non-tail "
         (us control) " us, tail " (us tail) " us")

(assert (< tail (* 4.0 (max control 0.00001)))
        (concat "a tail call must cost one pass over its arguments; its fastest run took "
                (string (us tail)) " us against the non-tail "
                (string (us control)) " us for the same work"))
