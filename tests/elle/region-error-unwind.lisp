(elle/epoch 12)
# An abandoned frame runs the releases it still owes
# (docs/impl/region/mechanism.md § "An abandoned frame runs the releases it
# still owes").
#
# An error leaves through the signal machinery, so none of the frame's
# remaining instructions run — and every release the frame still owed is among
# them. The frame that called the raising native is holding the arguments it
# materialized for that call, and every binding whose last use lies past it;
# each of those is one region nobody releases.
#
# The rate is per unwound frame and per pending value, so it is what a `try` or
# a `protect` in a loop grows by — the shape a retry loop and a server request
# loop both are.
#
# The runtime runs those releases at the exit, off the two tables the emitter
# recorded. A value route is `LoadLocal s; DecrefValueRegion; StoreLocal s nil`,
# so a slot still holding a heap value is a release that did not run; a slot
# route is `DecrefRegion`, whose receipt is the activation-map entry it takes,
# so a slot still mapped is one that did not run either.
#
# This file is the LEAK gauge — an `arena/count` delta over a fixed window,
# BOUNDED for every subject. The soundness complement is
# region-error-unwind-uaf.lisp; the per-op rate is the `denied-discard` probe
# in tests/elle/oracle.lisp.

(def window 500)

(defn measure [thunk warm window]
  (var i 0)
  (while (%lt i warm)
    (thunk)
    (assign i (%add i 1)))
  (def before (arena/count))
  (var j 0)
  (while (%lt j window)
    (thunk)
    (assign j (%add j 1)))
  (%sub (arena/count) before))

# subjects ─────────────────────────────────────────────────────────────────────

# (a) ONE pending value: the string the frame built for the raising call. Its
# release sits past the call, which never returns.
(defn one-arg []
  (protect (get (string "x" 1) :k))
  nil)

# (b) TWO of them: the rate is per pending value, not per frame.
(defn two-args []
  (protect (get (string "x" 1) (string "y" 2)))
  nil)

# (c) a binding LIVE ACROSS the raising call — bound before it, released after.
(defn live-binding []
  (protect (live-across))
  nil)

(defn live-across []
  (let [a (string "a" 1)
        b (string "b" 2)]
    (get a b)))

# (d) an ENCLOSING frame's pending value: the raising frame is the callee, and
# the caller's own release is abandoned too.
(defn raiser []
  (get 5 :k))

(defn outer-frame []
  (protect (outer-holder))
  nil)

(defn outer-holder []
  (let [o (string "o" 1)]
    (begin
      (raiser)
      o)))

# controls ─────────────────────────────────────────────────────────────────────

# (e) a raise with NOTHING pending: no heap argument, no live binding. Bounded
# before this mechanism and after it, so a regression here is the walk running
# a release the frame never had.
(defn no-pending []
  (protect (get 5 :k))
  nil)

# (f) the same values with NO raise: the ordinary releases run, so this pins
# that the subjects measure the abandoned release and not their own scratch.
(defn no-raise []
  (protect (get (string "x" 1) 0))
  nil)

# measurement ──────────────────────────────────────────────────────────────────

(def d-one (measure one-arg 20 window))
(def d-two (measure two-args 20 window))
(def d-live (measure live-binding 20 window))
(def d-outer (measure outer-frame 20 window))
(def d-none (measure no-pending 20 window))
(def d-ok (measure no-raise 20 window))

(println "region-error-unwind over " window " iters (object deltas):")
(println "  one-arg      " d-one)
(println "  two-args     " d-two)
(println "  live-binding " d-live)
(println "  outer-frame  " d-outer)
(println "  no-pending   " d-none " (control)")
(println "  no-raise     " d-ok " (control)")

(assert (%lt d-none 50)
        (concat "control: a raise with nothing pending must release nothing, "
                "delta=" (number->string d-none)))
(assert (%lt d-ok 50)
        (concat "control: the same values with no raise reclaim normally, "
                "delta=" (number->string d-ok)))

(assert (%lt d-one 50)
        (concat "the raising call's own argument is a release the frame still "
                "owed, delta=" (number->string d-one)))
(assert (%lt d-two 50)
        (concat "two pending arguments are two owed releases, delta="
                (number->string d-two)))
(assert (%lt d-live 50)
        (concat "a binding live across the raising call is owed a release, "
                "delta=" (number->string d-live)))
(assert (%lt d-outer 50)
        (concat "an enclosing frame's pending release is abandoned too, delta="
                (number->string d-outer)))
(println "region-error-unwind: ok")
