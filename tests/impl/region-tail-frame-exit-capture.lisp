(elle/epoch 13)
# audited: 2026-09-29
# A value a tail callee reaches through its captured environment, or hands back, is released once before the call.
# docs/impl/region/relocate.md
# docs/impl/region/compensate.md
#
# region-tail-frame-exit.lisp moves a release the lowerer emits past a
# frame-replacing `TailCall` to just before it. A value the tail callee reaches
# through its CAPTURED environment is named by no argument and by no callee
# region, yet the call reads it — and it is admitted anyway, because the funnel
# counted the closure's hold when the env was built, so the frame's release is
# still the only reference it drops.
#
# A value the callee hands BACK reaches zero at no point either. The caller's
# owning reference is minted by the CALLEE's `Return`, after the relocated
# release has run — and the callee either holds the region through its env,
# whose counted edge is dropped only with the closure region at the callee's
# completion, or cannot name the region at all, in which case its `Return`
# mints nothing against it. So a region whose only escape facet is the return
# one is admitted at every relocation point.
#
# An ENV CELL's release leaves no nil-stamp, so it takes another route: the
# relocation keeps it in the arm whose tail call it was placed in, and the
# sibling arm takes branch compensation's per-arm release — the HEAD one where
# it names the cell's binding nowhere, the TAIL one, after its last use of that
# binding, where it reads it. Exactly one of the two runs per path, because the
# arms are mutually exclusive.
#
# This file is a LEAK gauge — an `arena/count` delta over a fixed window, which
# must be BOUNDED for each subject. The soundness complement is
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

# (a) the tail callee reaches the parameter through its CAPTURED environment.
# No argument names it, so the exemption cannot see it — and it does not need to:
# building `g`'s env took a counted reference through the allocation funnel, so
# the frame's own release is the only one it drops.
(defn captured-param (x)
  (let [g (fn () (length x))]
    (g)))

# (b) the same, one block further out: the capturing closure is the callee of a
# branch ARM, so the release is the merge's replica rather than an in-block move.
(defn arm-captured (x t)
  (let [g (fn () (length x))]
    (if t (g) (tail-sink2))))

# (c) a walker that fills its captured accumulator in place and returns
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

# (d) the same walker HANDING THE ACCUMULATOR BACK — the stdlib `push-all` shape.
# `dst` crosses the return frontier, so the caller's owning reference is minted by
# `go`'s `Return`, after the relocated release has already run. What holds the
# region off zero in between is the counted edge the funnel took when `go`'s
# environment was built, and that edge falls away only with the closure region, at
# the callee's completion.
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

# (e) the same hand-back where this frame holds the ONLY other reference: the
# accumulator is minted at the call site and MOVED into the walker by a tail call,
# so the captured edge is the single thing standing between the relocated release
# and the callee's mint. Bounded here and faulting in the uaf complement are the
# two halves of one claim about that edge.
(defn drive-walk-moved (src)
  (walk-all (@array) src))

# (f) the hand-back reached through a branch ARM, so the release is the merge's
# replica ahead of that arm's `TailCall` rather than an in-block move, and the
# callee's counted edge is what stands between it and the mint.
(defn arm-handback (v t)
  (let [g (fn () v)]
    (if t (g) 0)))

# (g) a returned holder the tail callee neither names nor captures. `v` reaches a
# return through the OTHER arm, so the arm that leaves through the callee releases a
# return-frontier region ahead of that call — and it owes no funding edge, because a
# callee reaches a value this frame owns as an operand or through its captured
# environment and by no other route. This one is reached by neither, so it cannot
# mint against `v`'s region at all and the replica is the last release. Both arms
# are driven, and the claim is that exactly one release runs on each.
(defn handback-unreached (v t)
  (if t v (tail-sink)))

# (h) the everyday shape of the same reading — an index-walk fold driver. The base
# arm returns the accumulator; the recursive arm hands the tail callee the
# COMBINER's result rather than `acc` itself, so nothing at that point reaches `acc`
# and each displaced accumulator is the frame's alone to free. This is what `fold`,
# `reduce` and `concat` walk with, so a strand here is one region per element.
(defn fold-step (f n i acc)
  (if (%lt i n) (fold-step f n (%add i 1) (f acc i)) acc))
(defn drive-fold (n)
  (length (fold-step (fn (a b) (@array)) n 0 (@array))))

# (i) a captured local's ENV CELL. `populate_env` mints the cell box once per
# activation, and its `DecrefCellRegion` lands in the same dead block — so a frame
# that ends in a closure tail call strands one box per call unless the release
# relocates too. The reassigned face is the one the frame-held admission has to
# read correctly: a mutated holder refuses a release routed through its SLOT, and
# this release names the BOX, which no `assign` repoints
# (docs/impl/region/window.md).
(defn cell-immutable (n)
  (def @c n)
  (let [g (fn () c)]
    (g)))
(defn cell-reassigned (n)
  (def @c n)
  (let [g (fn ()
            (assign c (%add c 1))
            c)]
    (g)))

# (j) the same box with a HEAP init the caller owns, reassigned away inside the
# callee: the cell's content accounting is the caller's, so the box is the only
# per-call region and the row reads it alone.
(defn cell-heap (s)
  (def @c s)
  (let [g (fn ()
            (assign c (length c))
            c)]
    (g)))

# (k) the reassigned cell where the tail call sits in a branch ARM, so the box's
# release is relocated at that arm's own point. The SIBLING arm reaches the merge
# instead and finds no release there — the box's one `DecrefCellRegion` went into
# the arm the relocation moved it into. The sibling names the cell's binding
# nowhere, so it is a dead sibling arm and compensation's HEAD route covers it: the
# release names the cell BOX, which no `assign` repoints and no capturer aliases
# uncounted. Both arms are driven, since the whole claim is that exactly one
# release runs on each.
(defn arm-cell (n t)
  (def @c n)
  (let [g (fn ()
            (assign c (%add c 1))
            c)]
    (if t (g) 0)))

# (k2) the immutable face of the same shape: the strand is per env cell, not per
# reassignment, so a capture the body never rewrites takes the same head release.
(defn arm-cell-ro (n t)
  (def @c n)
  (let [g (fn () c)]
    (if t (g) 0)))

# (k3) the sibling arm READS the cell's binding, so it is a USED sibling arm and
# takes compensation's tail route — the same box release, after that arm's last use
# instead of at its head. The route's same-node retain is a claim about the value
# the holder names; this release names the box, whose holders are the frame's env
# slot and one counted `closure ⊇ cell` edge per capturer, so no use of the binding
# can add one. Both arms are driven: the reading arm runs the tail release, the
# sibling runs the relocated one.
(defn arm-cell-read (n t)
  (def @c n)
  (let [g (fn () c)]
    (if t c (g))))

# (k4) the reassigned face of the reading arm: the box the reader's arm releases
# is the one an `assign` writes through, which repoints its content and never the
# box itself.
(defn arm-cell-read-rw (n t)
  (def @c n)
  (let [g (fn ()
            (assign c (%add c 1))
            c)]
    (if t c (g))))

# the capturing closure LEAVES BY RETURN ───────────────────────────────────────
# A closure the frame hands back carries its captures with it, and it carries them
# on the funnel's counted edge — which is why escape's capture facet propagates
# nothing beyond the return one here and the holder is admitted like any other. The
# release relocates ahead of the sibling arm's tail call, and the counted edge is
# what keeps the capture alive for the caller that drives the returned closure.
# Driven for its VALUE: what must hold is that the escaped closure can still read
# what it captured. The genuine refusal is a closure that crosses the FIBER
# frontier, whose holder no edge at the point replaces.

(defn escaping-capture (x t)
  (let [g (fn () (length x))]
    (if t g (tail-sink))))

# The same for a reassigned capture: the returned closure carries the CELL on the
# same counted `closure ⊇ cell` edge. Driven for its VALUE — what must hold is that
# the escaped closure can still read and rewrite the cell it captured.
(defn escaping-cell (n t)
  (def @c n)
  (let [g (fn ()
            (assign c (%add c 1))
            c)]
    (if t g (tail-sink))))

(def captured-param-d (measure (fn () (captured-param [1 2])) 200 window))
(def arm-captured-d (measure (fn () (arm-captured [1 2] true)) 200 window))
(def walk-fill-d (measure (fn () (drive-fill [1 2 3])) 200 window))
(def walk-d (measure (fn () (drive-walk [1 2 3])) 200 window))
(def walk-moved-d
  (measure (fn () (length (drive-walk-moved [1 2 3]))) 200 window))
(def arm-handback-d
  (measure (fn () (length (arm-handback [1 2 3] true))) 200 window))
(def handback-unreached-t-d
  (measure (fn () (length (handback-unreached (list 1 2 3) true))) 200 window))
(def handback-unreached-f-d
  (measure (fn () (handback-unreached (list 1 2 3) false)) 200 window))
(def drive-fold-d (measure (fn () (drive-fold 3)) 200 window))
(def cell-src [1 2 3])
(def cell-immutable-d (measure (fn () (cell-immutable 1)) 200 window))
(def cell-reassigned-d (measure (fn () (cell-reassigned 1)) 200 window))
(def cell-heap-d (measure (fn () (cell-heap cell-src)) 200 window))
(def arm-cell-t-d (measure (fn () (arm-cell 1 true)) 200 window))
(def arm-cell-f-d (measure (fn () (arm-cell 1 false)) 200 window))
(def arm-cell-ro-t-d (measure (fn () (arm-cell-ro 1 true)) 200 window))
(def arm-cell-ro-f-d (measure (fn () (arm-cell-ro 1 false)) 200 window))
(def arm-cell-read-t-d (measure (fn () (arm-cell-read 1 true)) 200 window))
(def arm-cell-read-f-d (measure (fn () (arm-cell-read 1 false)) 200 window))
(def arm-cell-read-rw-t-d (measure (fn () (arm-cell-read-rw 1 true)) 200 window))
(def arm-cell-read-rw-f-d
  (measure (fn () (arm-cell-read-rw 1 false)) 200 window))

(println "region-tail-frame-exit-capture deltas over " window " iters:")
(println "  captured " captured-param-d "  arm-captured " arm-captured-d
         "  walk-fill " walk-fill-d)
(println "  walk " walk-d "  walk-moved " walk-moved-d "  arm-handback "
         arm-handback-d)
(println "  unreached hand-back " handback-unreached-t-d "/"
         handback-unreached-f-d "  fold driver " drive-fold-d)
(println "  cells: immutable " cell-immutable-d "  reassigned "
         cell-reassigned-d "  heap-init " cell-heap-d "  arm " arm-cell-t-d "/"
         arm-cell-f-d "  arm-ro " arm-cell-ro-t-d "/" arm-cell-ro-f-d)
(println "  cells read by a sibling arm: ro " arm-cell-read-t-d "/"
         arm-cell-read-f-d "  rw " arm-cell-read-rw-t-d "/" arm-cell-read-rw-f-d)

# Every leak in this class is at least one whole region per call, so a surviving
# strand reads >=2000 over the window. 100 is slack for the one-time intercept.
(defn bounded? (d label)
  (assert (%lt d 100) (concat label " leaks, delta=" (number->string d))))

(bounded? captured-param-d
          "parameter the tail callee reaches through its captured environment")
(bounded? arm-captured-d
          "the same capture reached through a branch arm's callee")
(bounded? walk-fill-d
          "a walker's captured parameters, neither of which it hands back")
(bounded? walk-d "the accumulator a captured walker hands back")
(bounded? walk-moved-d
          "the hand-back where the captured edge is the only other reference")
(bounded? arm-handback-d "the hand-back reached through a branch arm's callee")
(bounded? handback-unreached-t-d
          "the arm that returns a hand-back the sibling's callee cannot reach")
(bounded? handback-unreached-f-d
          "the arm whose callee cannot reach the returned hand-back")
# One accumulator per step, so the surviving-strand floor is a multiple of the
# window; `bounded?`'s slack covers the one-time intercept either way.
(bounded? drive-fold-d "an index-walk fold driver's displaced accumulators")

(bounded? cell-immutable-d "the env cell of a captured local")
(bounded? cell-reassigned-d "the env cell of a REASSIGNED captured local")
(bounded? cell-heap-d "the env cell of a reassigned capture with a heap init")
(bounded? arm-cell-t-d
          "the reassigned cell relocated at a branch arm's tail call")
(bounded? arm-cell-f-d "the falling-through sibling of that arm")
(bounded? arm-cell-ro-t-d "the immutable cell relocated at the same arm")
(bounded? arm-cell-ro-f-d "the falling-through sibling of the immutable arm")
(bounded? arm-cell-read-t-d "the arm that READS the cell its sibling relocated")
(bounded? arm-cell-read-f-d "the relocating sibling of the reading arm")
(bounded? arm-cell-read-rw-t-d "the reading arm of a REASSIGNED cell")
(bounded? arm-cell-read-rw-f-d "the relocating sibling of the reassigned reader")

# Value preservation: relocating a release must not change what runs.
(assert (= (drive-walk [1 2 3]) 3) "walker result lost")
(assert (= (length (drive-walk-moved [1 2 3])) 3) "moved-in walker result lost")
(assert (= (length (arm-handback [1 2 3] true)) 3) "arm hand-back result lost")
(assert (= (arm-handback [1 2 3] false) 0) "arm hand-back sibling arm lost")
(assert (= (length (handback-unreached [1 2 3] true)) 3)
        "unreached hand-back result lost")
(assert (= (handback-unreached [1 2 3] false) 0)
        "unreached hand-back sibling arm lost")
(assert (= (drive-fold 3) 0) "fold driver result lost")
(assert (= (captured-param [1 2]) 2) "captured-param result lost")
(assert (= (arm-captured [1 2] true) 2) "arm-captured result lost")
(assert (= (drive-fill [1 2 3]) 3) "walk-fill result lost")
(assert (= ((escaping-capture [1 2] true)) 2) "escaping-capture result lost")
(assert (= (escaping-capture [1 2] false) 0) "escaping-capture sibling arm lost")
(assert (= (cell-immutable 7) 7) "immutable cell result lost")
(assert (= (cell-reassigned 7) 8) "reassigned cell result lost")
(assert (= (cell-heap cell-src) 3) "heap-init cell result lost")
(assert (= (arm-cell 7 true) 8) "arm reassigned cell result lost")
(assert (= (arm-cell 7 false) 0) "arm reassigned cell sibling arm lost")
(assert (= (arm-cell-ro 7 true) 7) "arm immutable cell result lost")
(assert (= (arm-cell-ro 7 false) 0) "arm immutable cell sibling arm lost")
(assert (= (arm-cell-read 7 true) 7) "arm cell read result lost")
(assert (= (arm-cell-read 7 false) 7) "arm cell read sibling arm lost")
(assert (= (arm-cell-read-rw 7 true) 7) "reassigned arm cell read result lost")
(assert (= (arm-cell-read-rw 7 false) 8)
        "reassigned arm cell read sibling arm lost")
(let [g (escaping-cell 7 true)]
  (assert (= (g) 8) "escaping cell first read lost")
  (assert (= (g) 9) "escaping cell rewrite lost"))
(assert (= (escaping-cell 7 false) 0) "escaping-cell sibling arm lost")

(println "region-tail-frame-exit-capture: ok")
