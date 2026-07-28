(elle/epoch 12)
# A release past a frame-replacing tail call is not a release
# (docs/impl/region/mechanism.md § "A release past a frame-replacing tail call is
# not a release").
#
# A tail call whose callee is a closure replaces the frame, so everything the
# lowerer emits after the `TailCall` runs only on the NATIVE fall-through. For a
# region the call's arguments name that is the ownership move, and for the
# callee's own region the new activation takes the release over. Every OTHER
# release there — a parameter whose only use is inside a closure the body builds,
# a parameter used nowhere, a scope region the body allocated — is emitted where
# control never arrives, and the frame's reference is stranded once per call.
#
# The close moves that one release to just BEFORE the `TailCall`. Relocating an
# instruction is not by itself free of obligation: on the closure path the
# release now fires where none fired before, so it owes the same count argument
# any such mechanism owes, and escape supplies it — the frame must be the
# region's SOLE holder. That is why the walker rows below are residual and not
# subjects: a value the tail callee reaches through its CAPTURED environment is
# named by no argument and by no callee region, yet the call reads it, so a
# captured holder keeps the baseline.
#
# This file is the LEAK gauge — an `arena/count` delta over a fixed window, which
# must be BOUNDED for each subject, and for the exemptions and the boundary,
# whose releases must stay exactly where they are. The soundness complement is
# region-tail-frame-exit-uaf.lisp; the per-op rates are the `tail-frame-exit`
# probes in tests/elle/oracle.lisp.

(def window 2000)

(defn measure (thunk warm window)
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

# (b) a parameter used NOWHERE, so its release is the unused-param fallback the
# lowerer emits after the body — the same dead block. Escape clears it: nothing
# names the value but this frame's slot.
(defn tail-sink ()
  0)
(defn unused-param (x)
  (tail-sink))

# (c) two of them: the strand is per region, not per call.
(defn unused-two (x y)
  (tail-sink))

# exemptions ───────────────────────────────────────────────────────────────────
# The releases that must STAY in the dead fall-through. Each is already bounded;
# hoisting one would release a reference the callee now owns, so these rows are
# the over-free face of the gate.

# (e) the argument is MOVED into the tail call: the release it never runs is the
# reference the callee's owned-param release consumes.
(defn take-one (a)
  (length a))
(defn moved-arg (x)
  (take-one x))

# (f) a moved argument beside a stranded one — the exemption is per region.
(defn moved-and-stranded (x y)
  (take-one x))

# (g) the callee is a per-call local closure: the new activation takes over its
# release, so the frame must not also drop it here.
(defn callee-local (x)
  (let [g (fn (a) (length a))]
    (g x)))

# controls ─────────────────────────────────────────────────────────────────────
# Shapes with no dead block at all: a native tail call keeps the frame and falls
# through, and a non-tail call returns to the live scope exit.

(defn native-tail (x y)
  (length x))
(defn non-tail (x y)
  (tail-sink)
  0)

# boundary ─────────────────────────────────────────────────────────────────────
# The tail call sits inside a branch arm, so the enclosing scope's releases are
# emitted after the merge — a different block, reached by paths this tail call is
# not on. The hoist declines and the baseline stands: this row must not
# over-free, and must not regress the arm that already released.

(defn branch-tail (x t)
  (if t (tail-sink) (length x)))

# residual ─────────────────────────────────────────────────────────────────────
# A CAPTURED holder keeps the baseline, because the tail callee reaches it
# through its environment and no reading of the call's arguments can see that.
# These two still strand, and they are driven here so the residual is measured
# rather than asserted away: what must hold is that they do not FAULT, and the
# values they compute are correct.

(defn walk-all (dst src)
  (let [n (length src)]
    (letrec [go (fn [i]
                  (if (%lt i n)
                    (begin
                      (push dst (get src i))
                      (go (%add i 1)))
                    dst))]
      (go 0))))
(defn drive-walk (src)
  (let [acc (@array)]
    (walk-all acc src)
    (length acc)))
(defn captured-param (x)
  (let [g (fn () (length x))]
    (g)))

(def walk-d (measure (fn () (drive-walk [1 2 3])) 200 window))
(def unused-param-d (measure (fn () (unused-param [1 2])) 200 window))
(def unused-two-d (measure (fn () (unused-two [1 2] [3 4])) 200 window))
(def captured-param-d (measure (fn () (captured-param [1 2])) 200 window))
(def moved-arg-d (measure (fn () (moved-arg [1 2])) 200 window))
(def moved-and-stranded-d
  (measure (fn () (moved-and-stranded [1 2] [3 4])) 200 window))
(def callee-local-d (measure (fn () (callee-local [1 2])) 200 window))
(def native-tail-d (measure (fn () (native-tail [1 2] [3 4])) 200 window))
(def non-tail-d (measure (fn () (non-tail [1 2] [3 4])) 200 window))
(def branch-false-d (measure (fn () (branch-tail [1 2] false)) 200 window))

(println "region-tail-frame-exit deltas over " window " iters:")
(println "  walk " walk-d "  unused " unused-param-d "  unused-two "
         unused-two-d "  captured " captured-param-d)
(println "  exemptions: moved " moved-arg-d "  moved+stranded "
         moved-and-stranded-d "  callee-local " callee-local-d)
(println "  controls: native " native-tail-d "  non-tail " non-tail-d)
(println "  boundary: branch-false " branch-false-d)

# Every leak in this class is at least one whole region per call, so a surviving
# strand reads >=2000 over the window. 100 is slack for the one-time intercept.
(defn bounded? (d label)
  (assert (%lt d 100) (concat label " leaks, delta=" (number->string d))))

(bounded? native-tail-d "control: native tail call falls through")
(bounded? non-tail-d "control: non-tail call returns to the live scope exit")
(bounded? moved-arg-d "exemption: the moved argument's release is the transfer")
(bounded? callee-local-d "exemption: the callee's release is the activation's")
(bounded? branch-false-d "boundary: the arm that released must still release")

(bounded? unused-param-d "unused parameter past a frame-replacing tail call")
(bounded? unused-two-d "two unused parameters past one tail call")
(bounded? moved-and-stranded-d "stranded parameter beside a moved one")

# The residual rows are NOT asserted bounded — a captured holder keeps the
# baseline by design. What is asserted is that they still compute correctly:
# a strand is an over-keep, and must never become a mis-free.
(assert (= (drive-walk [1 2 3]) 3) "walker result lost")
(assert (= (captured-param [1 2]) 2) "captured-param result lost")

# Value preservation: relocating a release must not change what runs.
(assert (= (unused-param [1 2]) 0) "unused-param result lost")
(assert (= (moved-arg [1 2]) 2) "moved-arg result lost")
(assert (= (callee-local [1 2]) 2) "callee-local result lost")
(assert (= (native-tail [1 2] [3 4]) 2) "native-tail result lost")
(assert (= (branch-tail [1 2] false) 2) "branch else-arm result lost")
(assert (= (branch-tail [1 2] true) 0) "branch then-arm result lost")

(println "region-tail-frame-exit: ok")
