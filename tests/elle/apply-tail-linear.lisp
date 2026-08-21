(elle/epoch 12)
# Argument passing must cost one pass over the arguments, whatever the
# calling convention. The convention that has to work hardest for that is
# a tail call to a variadic callee: the caller's reference to each
# argument moves to the callee, the rest arguments land in a collected
# list holding its own reference, and the moved-in reference is surplus.
# Releasing it needs each value's occurrence count across the whole
# argument list — a value appearing twice shares one moved reference, so
# a second release would free it under a live use. See
# `docs/regions/performance.md` § "Passing arguments costs one pass over
# them".
#
# The two calls below differ in exactly one thing: tail position. They
# spread arrays of the same length into the same callee, so they build
# the same rest list and allocate the same amount. Only the tail call
# takes the move path with its release step.
#
# The measurement is a ratio against the non-tail call, not a clock
# bound, so it reads the same on a fast machine and a loaded one. Taking
# the occurrence counts pairwise costs 1.6e9 steps here against 4e4 —
# about 20x the control, and it grows with n. Anything near 1x is the
# one-pass count.

(def n 40000)

(defn sink [& xs]
  (length xs))

(defn fill []
  (let [@c @[]]
    (each _ in (range 0 n)
      (push c (bytes 1 2 3 4)))
    c))

(defn call-tail [c]
  (apply sink c))

# `+` forces the result into an argument position, so the `apply` is not
# in tail position and the callee takes the owned-parameter path.
(defn call-nontail [c]
  (+ 0 (apply sink c)))

(defn timed [f c]
  (let [t0 (clock/monotonic)
        r (f c)]
    (assert (= r n) "the callee received every argument")
    (- (clock/monotonic) t0)))

# Warm up both paths so neither measurement pays first-call compilation.
(assert (= (call-tail [1 2]) 2) "tail warm-up")
(assert (= (call-nontail [1 2]) 2) "non-tail warm-up")

# Each call gets its own array: the tail call moves the caller's
# references, so reusing one array across both would not measure the same
# thing twice.
(def control (timed call-nontail (fill)))
(def tail (timed call-tail (fill)))

(println "apply-tail-linear: n=" n " non-tail " (round (* control 1000.0)) " ms"
         " tail " (round (* tail 1000.0)) " ms")

(assert (< tail (* 4.0 (max control 0.001)))
        (concat "a tail `apply` must cost one pass over its arguments; it took "
                (string (round (* tail 1000.0))) " ms against the non-tail "
                (string (round (* control 1000.0))) " ms for the same work"))
