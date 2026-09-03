(elle/epoch 12)
# A squelch boundary abandons frames the same way an error does
# (docs/impl/region/mechanism.md § "A squelch boundary abandons frames the same
# way, so it runs the same walk").
#
# A `squelch`/`attune` boundary raises a `signal-violation` the abandoned
# activation never catches. So the frames between the emitting site and the
# boundary are abandoned exactly as an error's are: none of their remaining
# instructions run, and every release among them is one region nobody
# releases. The rate is per pending value, per abandoned frame — what a
# `squelch` in a retry loop or a request loop grows by.
#
# Two places carry the frames. The chain the boundary parked is abandoned at
# the discard chokepoint (`VM::discard_suspended_frames`), and the activation
# the boundary breaks out of is abandoned at the exit itself. Both run the two
# tables the emitter recorded, off each frame's own `Code`. Every subject below
# reaches the boundary through a park, so what it gauges is the chain; the
# chokepoint itself is driven directly by
# `runtime::tests::ownership::discard_runs_the_abandoned_frames_release_tables`.
#
# This file is the LEAK gauge — `arena/count` and `arena/region-count` deltas
# over a fixed window, BOUNDED for every subject. The soundness complement is
# region-squelch-unwind-uaf.lisp.

(def window 300)

(defn measure [thunk warm window]
  (var i 0)
  (while (%lt i warm)
    (thunk)
    (assign i (%add i 1)))
  (def objects (arena/count))
  (def regions (arena/region-count))
  (var j 0)
  (while (%lt j window)
    (thunk)
    (assign j (%add j 1)))
  [(%sub (arena/count) objects) (%sub (arena/region-count) regions)])

# subjects ─────────────────────────────────────────────────────────────────────

# (a) ONE pending value, held by the frame that emits. Its release sits past
# the emit, which the boundary never returns from.
(def one-body
  (squelch (fn []
             (let [s (string "x" 1)]
               (begin
                 (emit :yield 1)
                 s))) :yield))

(defn one-pending []
  (try
    (one-body)
    (catch e nil)))

# (b) TWO of them: the rate is per pending value, not per frame.
(def two-body
  (squelch (fn []
             (let [s (string "x" 1)
                   t (string "y" 2)]
               (begin
                 (emit :yield 1)
                 (get s 0)
                 (get t 0)
                 s))) :yield))

(defn two-pending []
  (try
    (two-body)
    (catch e nil)))

# (c) an ENCLOSING frame's pending value: the emitting frame is a callee, so
# the caller's own release is abandoned with it.
(defn yielder []
  (emit :yield 1))

(def enclosing-body
  (squelch (fn []
             (let [s (string "x" 1)]
               (begin
                 (yielder)
                 s))) :yield))

(defn enclosing []
  (try
    (enclosing-body)
    (catch e nil)))

# (d) an ATTUNE boundary rather than a squelch: the same enforcement, reached
# from the complementary mask.
(def attune-body
  (attune |:error|
          (fn []
            (let [s (string "x" 1)]
              (begin
                (emit :yield 1)
                s)))))

(defn attuned []
  (try
    (attune-body)
    (catch e nil)))

# controls ─────────────────────────────────────────────────────────────────────

# (e) a violation with NOTHING pending. Bounded before this mechanism and
# after it, so a regression here is the discard running a release no frame
# ever owed. It is also what isolates the boundary's own `signal-violation`
# error, which every subject above builds too.
(def none-body
  (squelch (fn []
             (begin
               (emit :yield 1)
               7)) :yield))

(defn nothing-pending []
  (try
    (none-body)
    (catch e nil)))

# (f) the same squelched body with NO violation: the ordinary releases run, so
# this pins that the subjects measure the abandoned release and not the body's
# own scratch.
(def clean-body
  (squelch (fn []
             (let [s (string "x" 1)]
               s)) :yield))

(defn no-violation []
  (try
    (clean-body)
    (catch e nil)))

# (g) the CATCHING frame's own value: it is not abandoned, so its release runs
# where it always did. A regression here is the walk reaching past the frames
# the boundary actually abandons.
(defn catching []
  (let [c (string "c" 1)]
    (begin
      (try
        (none-body)
        (catch e nil))
      c)))

# measurement ──────────────────────────────────────────────────────────────────

(def d-one (measure one-pending 20 window))
(def d-two (measure two-pending 20 window))
(def d-encl (measure enclosing 20 window))
(def d-attune (measure attuned 20 window))
(def d-none (measure nothing-pending 20 window))
(def d-clean (measure no-violation 20 window))
(def d-catch (measure catching 20 window))

(println "region-squelch-unwind over " window " iters [objects regions]:")
(println "  one-pending      " d-one)
(println "  two-pending      " d-two)
(println "  enclosing        " d-encl)
(println "  attuned          " d-attune)
(println "  nothing-pending  " d-none " (control)")
(println "  no-violation     " d-clean " (control)")
(println "  catching         " d-catch " (control)")

(def slack 50)

(defn bounded [label delta]
  (let [objects (get delta 0)
        regions (get delta 1)]
    (begin
      (assert (< objects slack)
              (concat label ": objects grew, delta=" (number->string objects)))
      (assert (< regions slack)
              (concat label ": regions grew, delta=" (number->string regions))))))

(bounded "control: a violation with nothing pending releases nothing" d-none)
(bounded "control: a squelched body that never violates reclaims normally"
         d-clean)
(bounded "control: the catching frame is not abandoned" d-catch)

(bounded "the emitting frame's pending value is a release it still owed" d-one)
(bounded "two pending values are two owed releases" d-two)
(bounded "an enclosing frame's pending release is abandoned too" d-encl)
(bounded "an attune boundary abandons its frames the same way" d-attune)

(println "region-squelch-unwind: ok")
