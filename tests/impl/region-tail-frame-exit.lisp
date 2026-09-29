(elle/epoch 13)
# audited: 2026-09-29
# A release the lowerer emits past a frame-replacing tail call runs ahead of that call, and the moves it exempts stay.
# docs/impl/region/relocate.md
# docs/impl/region/replicate.md
#
# A tail call whose callee is a closure replaces the frame, so everything the
# lowerer emits after the `TailCall` runs only on the NATIVE fall-through. For a
# region the call's arguments name that is the ownership move, and for the
# callee's own region the new activation takes the release over. Every OTHER
# release there — a parameter whose only use is inside a closure the body builds,
# a parameter used nowhere, a scope region the body allocated — is emitted where
# control never arrives, and without the relocation the frame's reference would
# strand once per call.
#
# The relocation moves that one release to just BEFORE the `TailCall`. Relocating
# an instruction is not by itself free of obligation: on the closure path the
# release now fires where none fired before, so it owes the same count argument
# any such mechanism owes, and escape supplies it — the frame must hold the
# region ALONE.
#
# A release emitted once the block has CLOSED is placed the other way: a branch
# merge inherits the relocation points of the arms that reach it, so the release
# is emitted at the merge AND replicated ahead of each arm's `TailCall`. That
# counts once per path because a value-routed release nil-stamps the slot it
# read, so the copy a path reaches second loads `nil` and no-ops.
#
# The arms are one of two sources. A merge also inherits what covered the
# position the branch was ENTERED at, because the merge is reached only through
# the branch. That is what a branch following an earlier branch needs, and
# functionalization writes one for every mutable a branch arm reassigns — so a
# loop or an `assign` inside an arm puts the enclosing scope's releases past a
# merge the first branch's tail-calling arm never arrives at.
#
# region-tail-frame-exit-capture.lisp covers what the tail callee reaches
# through its environment, and region-tail-frame-exit-letrec.lisp the letrec
# closures and forward cells.
#
# This file is the LEAK gauge — an `arena/count` delta over a fixed window, which
# must be BOUNDED for each subject, and for the exemptions and the boundary,
# whose releases must stay exactly where they are. The soundness complement is
# region-tail-frame-exit-uaf.lisp; the per-op rates are the oracle's
# `tail-frame-exit-*` rows (tests/impl/probe/branch.lisp).

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

(defn tail-sink ()
  0)
(defn tail-sink2 ()
  1)

# subjects ─────────────────────────────────────────────────────────────────────

# (a) a parameter used NOWHERE, so its release is the unused-param fallback the
# lowerer emits after the body — the same dead block. Escape clears it: nothing
# names the value but this frame's slot.
(defn unused-param (x)
  (tail-sink))

# (b) two of them: the strand is per region, not per call.
(defn unused-two (x y)
  (tail-sink))

# (c) the tail call sits in a branch ARM, so the release lands past the merge
# label — a block the arm's closure path never reaches. A branch merge inherits
# its arms' relocation points, so the release is emitted there AND replicated
# ahead of each arm's `TailCall`; a value-routed release nil-stamps the slot it
# read, so whichever copy a path reaches first does the work and any later one
# no-ops. Both arms, both nestings, and every branch kind that merges through
# arms alone.
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

# (d) only ONE arm leaves through a frame-replacing tail call; the other falls
# through to the merge and needs the release that is still emitted there. No arm
# has to be proven to tail-call for the accounting to hold — the nil-stamp is
# what makes the two copies act once.
(defn arm-partial (x t)
  (if t (tail-sink) 5))

# (e) the release lands past a SECOND branch. A merge inherits the relocation
# points of the arms that reach it AND the points that already covered the
# position the branch was entered at — the merge is reached only through the
# branch, so the paths arriving at it are the paths that arrived at the entry.
# Read from the arms alone, a branch following an earlier branch starts life
# covering nothing, and every release after it is emitted where the earlier
# branch's tail-calling arm never arrives.
#
# The second branch is not written here: functionalization inserts one for every
# mutable a branch arm reassigns, an `If` on the same condition merging the two
# versions of the name after the arm that already carries the tail call. The
# scrutinee's own region is what these rows measure — the branch tests a value
# read out of it, so its release lands at the branch and, with the second one
# inserted, past that instead.
(defn mk-ok ()
  [true 7])
(defn mk-not ()
  [false 7])
(defn phi-when (p)
  (let [[ok? n] (p)]
    (when ok?
      (let [_ (begin
                (def @i 0)
                (assign i (%add i 1))
                i)]
        (tail-sink)))))

# (e2) both arms, the sibling FALLING THROUGH to the merge, where the release
# is still emitted. Exactly one release runs per path, the arm's replica or the
# merge's copy.
(defn phi-if (p)
  (let [[ok? n] (p)]
    (if ok?
      (let [_ (begin
                (def @i 0)
                (assign i (%add i 1))
                i)]
        (tail-sink))
      5)))

# (e3) the reassignment inside a LOOP, which is how ordinary code writes it:
# `while` and `each` both carry an `assign` over an induction variable, so a walk
# in a branch arm inserts the merge whether or not its body ever runs. This one's
# never does.
(defn phi-loop (p)
  (let [[ok? n] (p)]
    (when ok?
      (let [_ (begin
                (def @i 0)
                (while (%lt i 0) (assign i (%add i 1))))]
        (tail-sink)))))

# (e4) a `cond` clause body carrying the reassignment, with the else body
# leaving through its own callee. Both bodies are driven.
(defn phi-cond (p)
  (let [[ok? n] (p)]
    (cond
      ok?
        (let [_ (begin
                  (def @i 0)
                  (assign i (%add i 1))
                  i)]
          (tail-sink))
      (tail-sink2))))

# (e5) control: the same arm with no reassignment, so functionalization inserts
# no second branch and the arm's own point is the one the release reads.
(defn phi-none (p)
  (let [[ok? n] (p)]
    (when ok?
      (let [_ (begin
                (def i 0)
                (%add i 1))]
        (tail-sink)))))

# exemptions ───────────────────────────────────────────────────────────────────
# The releases that must STAY in the dead fall-through. Each is bounded in
# place; hoisting one would release a reference the callee now owns, so these
# rows are the over-free face of the gate.

# (f) the argument is MOVED into the tail call: the release it never runs is the
# reference the callee's owned-param release consumes.
(defn take-one (a)
  (length a))
(defn moved-arg (x)
  (take-one x))

# (g) a moved argument beside a stranded one — the exemption is per region.
(defn moved-and-stranded (x y)
  (take-one x))

# (h) the callee is a per-call local closure: the new activation takes over its
# release, so the frame must not also drop it here.
(defn callee-local (x)
  (let [g (fn (a) (length a))]
    (g x)))

# (i) the argument is moved into ONE arm's tail call. The exemption is read per
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

(def unused-param-d (measure (fn () (unused-param [1 2])) 200 window))
(def unused-two-d (measure (fn () (unused-two [1 2] [3 4])) 200 window))
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
(def phi-when-d (measure (fn () (phi-when mk-ok)) 200 window))
(def phi-if-t-d (measure (fn () (phi-if mk-ok)) 200 window))
(def phi-if-f-d (measure (fn () (phi-if mk-not)) 200 window))
(def phi-loop-d (measure (fn () (phi-loop mk-ok)) 200 window))
(def phi-cond-t-d (measure (fn () (phi-cond mk-ok)) 200 window))
(def phi-cond-f-d (measure (fn () (phi-cond mk-not)) 200 window))
(def phi-none-d (measure (fn () (phi-none mk-ok)) 200 window))
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
(println "  unused " unused-param-d "  unused-two " unused-two-d)
(println "  arms: unused " arm-unused-t-d "/" arm-unused-f-d "  two " arm-two-d
         "  cond " arm-cond-0-d "/" arm-cond-2-d "  match " arm-match-a-d "/"
         arm-match-z-d)
(println "  arms: nested " arm-nested-t-d "/" arm-nested-f-d "  partial "
         arm-partial-t-d "/" arm-partial-f-d)
(println "  merge inheriting the branch's entry: when " phi-when-d "  if "
         phi-if-t-d "/" phi-if-f-d "  loop " phi-loop-d "  cond " phi-cond-t-d
         "/" phi-cond-f-d "  control " phi-none-d)
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

(bounded? phi-when-d
          "a release past the merge functionalization inserts for a reassigned local")
(bounded? phi-if-t-d "the tail-calling arm of the same shape's two-armed face")
(bounded? phi-if-f-d "the sibling arm, which falls through to the merge")
(bounded? phi-loop-d "the same where a loop carries the reassignment")
(bounded? phi-cond-t-d "the same under a cond clause body")
(bounded? phi-cond-f-d "the cond else body, which leaves through its own callee")
(bounded? phi-none-d "control: the same arm with no reassignment to merge")

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
(assert (= (phi-when mk-ok) 0) "phi merge when result lost")
(assert (nil? (phi-when mk-not)) "phi merge when sibling arm lost")
(assert (= (phi-if mk-ok) 0) "phi merge if result lost")
(assert (= (phi-if mk-not) 5) "phi merge if fall-through result lost")
(assert (= (phi-loop mk-ok) 0) "phi merge loop result lost")
(assert (= (phi-cond mk-ok) 0) "phi merge cond clause result lost")
(assert (= (phi-cond mk-not) 1) "phi merge cond else result lost")
(assert (= (phi-none mk-ok) 0) "control: no-reassignment arm result lost")

(println "region-tail-frame-exit: ok")
