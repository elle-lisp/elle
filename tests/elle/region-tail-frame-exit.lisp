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
# region's SOLE holder. A value the tail callee reaches through its CAPTURED
# environment is named by no argument and by no callee region, yet the call reads
# it — and it is admitted anyway, because the funnel counted the closure's hold
# when the env was built, so the frame's release is still the only reference it
# drops. What keeps the walker row residual is the other facet: it hands its
# accumulator BACK, so the return frontier holds that release to the caller's
# mint.
#
# A release emitted once the block has CLOSED is placed the other way (§ "The
# relocation point outlives the block"): a branch merge inherits the relocation
# points of the arms that reach it, so the release is emitted at the merge AND
# replicated ahead of each arm's `TailCall`. That counts once per path because a
# value-routed release nil-stamps the slot it read, so the copy a path reaches
# second loads `nil` and no-ops.
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

# (d) the tail call sits in a branch ARM, so the release lands past the merge
# label — a block the arm's closure path never reaches. A branch merge inherits
# its arms' relocation points, so the release is emitted there AND replicated
# ahead of each arm's `TailCall`; a value-routed release nil-stamps the slot it
# read, so whichever copy a path reaches first does the work and any later one
# no-ops. Both arms, both nestings, and every branch kind that merges through
# arms alone.
(defn tail-sink2 ()
  1)
(defn arm-unused (x t)
  (if t (tail-sink) (tail-sink2)))
(defn arm-two (x y t)
  (if t (tail-sink) (tail-sink2)))
(defn arm-cond (x t)
  (cond
    (%eq t 0) (tail-sink)
    (%eq t 1) (tail-sink2)
    (tail-sink)))
(defn arm-match (x t)
  (match t
    :a (tail-sink)
    _ (tail-sink2)))
(defn arm-nested (x a t)
  (if a (if t (tail-sink) (tail-sink2)) (tail-sink)))

# (d2) only ONE arm leaves through a frame-replacing tail call; the other falls
# through to the merge and needs the release that is still emitted there. No arm
# has to be proven to tail-call for the accounting to hold — the nil-stamp is
# what makes the two copies act once.
(defn arm-partial (x t)
  (if t (tail-sink) 5))

# (d3) the tail callee reaches the parameter through its CAPTURED environment.
# No argument names it, so the exemption cannot see it — and it does not need to:
# building `g`'s env took a counted reference through the allocation funnel, so
# the frame's own release is the only one it drops.
(defn captured-param (x)
  (let [g (fn () (length x))]
    (g)))

# (d4) the same, one block further out: the capturing closure is the callee of a
# branch ARM, so the release is the merge's replica rather than an in-block move.
(defn arm-captured (x t)
  (let [g (fn () (length x))]
    (if t (g) (tail-sink2))))

# (d5) a walker that fills its captured accumulator in place and returns
# something else. Both parameters are reached only through `go`'s environment and
# neither leaves the activation, so both reclaim — the walker shape minus the
# hand-back.
(defn walk-fill (dst src)
  (let [n (length src)]
    (letrec [go (fn [i]
                  (if (%lt i n)
                    (begin
                      (push dst (get src i))
                      (go (%add i 1)))
                    n))]
      (go 0))))
(defn drive-fill (src)
  (let [acc (@array)]
    (walk-fill acc src)
    (length acc)))

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

# (h) the argument is moved into ONE arm's tail call. The exemption is read per
# relocation point, so that arm keeps its release in the dead block while the
# sibling arm is free to take a copy.
(defn arm-moved (x t)
  (if t (take-one x) (tail-sink2)))

# controls ─────────────────────────────────────────────────────────────────────
# Shapes with no dead block at all: a native tail call keeps the frame and falls
# through, and a non-tail call returns to the live scope exit.

(defn native-tail (x y)
  (length x))
(defn non-tail (x y)
  (tail-sink)
  0)

# boundary ─────────────────────────────────────────────────────────────────────
# The arm that already released must keep releasing exactly once. `x`'s own
# `decref_point` sits in the else arm here, so the then arm's release is the
# dead-arm compensation at its head — emitted before the tail call, and never
# doubled by a replica.

(defn branch-tail (x t)
  (if t (tail-sink) (length x)))

# boundary: the capturing closure ESCAPES ──────────────────────────────────────
# A closure that leaves the activation carries its captures with it, and escape's
# capture facet says so — so the holder is refused and the release stays in the
# dead block beside the sibling arm's tail call. Driven for its VALUE, not its
# delta: it strands by design, and what must hold is that the escaped closure can
# still read what it captured.

(defn escaping-capture (x t)
  (let [g (fn () (length x))]
    (if t g (tail-sink))))

# residual ─────────────────────────────────────────────────────────────────────
# The walker hands its accumulator BACK through the tail callee, so `dst` crosses
# the return frontier: the caller's owning reference is minted after the relocated
# release would have run, and the admission refuses it. That one region still
# strands per call, and the row is driven here so the residual is measured rather
# than asserted away — what must hold is that it does not FAULT and computes
# correctly.

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

(def walk-d (measure (fn () (drive-walk [1 2 3])) 200 window))
(def unused-param-d (measure (fn () (unused-param [1 2])) 200 window))
(def unused-two-d (measure (fn () (unused-two [1 2] [3 4])) 200 window))
(def captured-param-d (measure (fn () (captured-param [1 2])) 200 window))
(def arm-captured-d (measure (fn () (arm-captured [1 2] true)) 200 window))
(def walk-fill-d (measure (fn () (drive-fill [1 2 3])) 200 window))
(def arm-unused-t-d (measure (fn () (arm-unused [1 2] true)) 200 window))
(def arm-unused-f-d (measure (fn () (arm-unused [1 2] false)) 200 window))
(def arm-two-d (measure (fn () (arm-two [1 2] [3 4] true)) 200 window))
(def arm-cond-0-d (measure (fn () (arm-cond [1 2] 0)) 200 window))
(def arm-cond-2-d (measure (fn () (arm-cond [1 2] 2)) 200 window))
(def arm-match-a-d (measure (fn () (arm-match [1 2] :a)) 200 window))
(def arm-match-z-d (measure (fn () (arm-match [1 2] :z)) 200 window))
(def arm-nested-t-d (measure (fn () (arm-nested [1 2] true true)) 200 window))
(def arm-nested-f-d (measure (fn () (arm-nested [1 2] false true)) 200 window))
(def arm-partial-t-d (measure (fn () (arm-partial [1 2] true)) 200 window))
(def arm-partial-f-d (measure (fn () (arm-partial [1 2] false)) 200 window))
(def arm-moved-t-d (measure (fn () (arm-moved [1 2] true)) 200 window))
(def arm-moved-f-d (measure (fn () (arm-moved [1 2] false)) 200 window))
(def moved-arg-d (measure (fn () (moved-arg [1 2])) 200 window))
(def moved-and-stranded-d
  (measure (fn () (moved-and-stranded [1 2] [3 4])) 200 window))
(def callee-local-d (measure (fn () (callee-local [1 2])) 200 window))
(def native-tail-d (measure (fn () (native-tail [1 2] [3 4])) 200 window))
(def non-tail-d (measure (fn () (non-tail [1 2] [3 4])) 200 window))
(def branch-false-d (measure (fn () (branch-tail [1 2] false)) 200 window))
(def branch-true-d (measure (fn () (branch-tail [1 2] true)) 200 window))

(println "region-tail-frame-exit deltas over " window " iters:")
(println "  walk " walk-d "  unused " unused-param-d "  unused-two "
         unused-two-d "  captured " captured-param-d "  arm-captured "
         arm-captured-d)
(println "  walk-fill " walk-fill-d)
(println "  arms: unused " arm-unused-t-d "/" arm-unused-f-d "  two " arm-two-d
         "  cond " arm-cond-0-d "/" arm-cond-2-d "  match " arm-match-a-d "/"
         arm-match-z-d)
(println "  arms: nested " arm-nested-t-d "/" arm-nested-f-d "  partial "
         arm-partial-t-d "/" arm-partial-f-d)
(println "  exemptions: moved " moved-arg-d "  moved+stranded "
         moved-and-stranded-d "  callee-local " callee-local-d "  arm-moved "
         arm-moved-t-d "/" arm-moved-f-d)
(println "  controls: native " native-tail-d "  non-tail " non-tail-d)
(println "  boundary: branch " branch-true-d "/" branch-false-d)

# Every leak in this class is at least one whole region per call, so a surviving
# strand reads >=2000 over the window. 100 is slack for the one-time intercept.
(defn bounded? (d label)
  (assert (%lt d 100) (concat label " leaks, delta=" (number->string d))))

(bounded? native-tail-d "control: native tail call falls through")
(bounded? non-tail-d "control: non-tail call returns to the live scope exit")
(bounded? moved-arg-d "exemption: the moved argument's release is the transfer")
(bounded? callee-local-d "exemption: the callee's release is the activation's")
(bounded? arm-moved-t-d "exemption: the arm that moved its argument")
(bounded? arm-moved-f-d "exemption: the sibling of the arm that moved")
(bounded? branch-false-d "boundary: the arm that released must still release")
(bounded? branch-true-d "boundary: the compensated arm must not double-release")

(bounded? unused-param-d "unused parameter past a frame-replacing tail call")
(bounded? unused-two-d "two unused parameters past one tail call")
(bounded? moved-and-stranded-d "stranded parameter beside a moved one")

(bounded? arm-unused-t-d
          "release past a merge both arms leave through a tail call")
(bounded? arm-unused-f-d "the sibling arm of the same merge")
# Two parameters strand two regions per call, so the surviving-strand floor is
# 2x the window; `bounded?`'s slack covers the one-time intercept either way.
(bounded? arm-two-d "two parameters past a merge both arms leave through")
(bounded? arm-cond-0-d "a cond clause body leaving through a tail call")
(bounded? arm-cond-2-d "a cond else body leaving through a tail call")
(bounded? arm-match-a-d "a match arm leaving through a tail call")
(bounded? arm-match-z-d "the match catch-all arm leaving through a tail call")
(bounded? arm-nested-t-d "an inner branch's arm inside an outer arm")
(bounded? arm-nested-f-d "the outer arm beside a branch that inherited points")
(bounded? arm-partial-t-d
          "the tail-calling arm of a partly falling-through branch")
(bounded? arm-partial-f-d "the falling-through arm keeps the merge release")

(bounded? captured-param-d
          "parameter the tail callee reaches through its captured environment")
(bounded? arm-captured-d
          "the same capture reached through a branch arm's callee")
(bounded? walk-fill-d
          "a walker's captured parameters, neither of which it hands back")

# The residual row is NOT asserted bounded — the walker hands its accumulator back
# through the tail callee, so the return frontier refuses that one release by
# design. What is asserted is that it still computes correctly: a strand is an
# over-keep, and must never become a mis-free.
(assert (= (drive-walk [1 2 3]) 3) "walker result lost")
(assert (= (captured-param [1 2]) 2) "captured-param result lost")
(assert (= (arm-captured [1 2] true) 2) "arm-captured result lost")
(assert (= (drive-fill [1 2 3]) 3) "walk-fill result lost")
(assert (= ((escaping-capture [1 2] true)) 2) "escaping-capture result lost")
(assert (= (escaping-capture [1 2] false) 0) "escaping-capture sibling arm lost")

# Value preservation: relocating a release must not change what runs.
(assert (= (unused-param [1 2]) 0) "unused-param result lost")
(assert (= (moved-arg [1 2]) 2) "moved-arg result lost")
(assert (= (callee-local [1 2]) 2) "callee-local result lost")
(assert (= (native-tail [1 2] [3 4]) 2) "native-tail result lost")
(assert (= (branch-tail [1 2] false) 2) "branch else-arm result lost")
(assert (= (branch-tail [1 2] true) 0) "branch then-arm result lost")
(assert (= (arm-unused [1 2] true) 0) "arm-unused then result lost")
(assert (= (arm-unused [1 2] false) 1) "arm-unused else result lost")
(assert (= (arm-cond [1 2] 2) 0) "arm-cond else result lost")
(assert (= (arm-match [1 2] :z) 1) "arm-match catch-all result lost")
(assert (= (arm-nested [1 2] true true) 0) "arm-nested inner result lost")
(assert (= (arm-partial [1 2] false) 5) "arm-partial fall-through result lost")
(assert (= (arm-moved [1 2] true) 2) "arm-moved result lost")

(println "region-tail-frame-exit: ok")
