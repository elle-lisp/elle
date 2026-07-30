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
# drops.
#
# A value the callee hands BACK is that same counted edge carrying one step
# further. The caller's owning reference is minted by the CALLEE's `Return`, after
# the relocated release has run, so the region must not reach zero in between —
# and the env edge, dropped only with the closure region at the callee's
# completion, is what stops it. So a region whose only escape facet is the return
# one is admitted at a relocation point whose callee captures one of its holders,
# and nowhere else.
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

# (d6) the same walker HANDING THE ACCUMULATOR BACK — the stdlib `push-all` shape.
# `dst` crosses the return frontier, so the caller's owning reference is minted by
# `go`'s `Return`, after the relocated release has already run. What holds the
# region off zero in between is the counted edge the funnel took when `go`'s
# environment was built, and that edge falls away only with the closure region, at
# the callee's completion (docs/impl/region/mechanism.md § "The callee's return
# mint, and the edge that funds the gap").
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

# (d7) the same hand-back where this frame holds the ONLY other reference: the
# accumulator is minted at the call site and MOVED into the walker by a tail call,
# so the captured edge is the single thing standing between the relocated release
# and the callee's mint. Bounded here and faulting in the uaf complement are the
# two halves of one claim about that edge.
(defn drive-walk-moved (src)
  (walk-all (@array) src))

# (d8) the hand-back reached through a branch ARM, so the release is the merge's
# replica rather than an in-block move, and the funding edge is read at the arm's
# own relocation point.
(defn arm-handback (v t)
  (let [g (fn () v)]
    (if t (g) 0)))

# (d9) a captured local's ENV CELL. `populate_env` mints the cell box once per
# activation, and its `DecrefCellRegion` lands in the same dead block — so a frame
# that ends in a closure tail call strands one box per call unless the release
# relocates too. The reassigned face is the one the sole-holder admission has to
# read correctly: a mutated holder refuses a release routed through its SLOT, and
# this release names the BOX, which no `assign` repoints
# (docs/impl/region/mechanism.md § "A mutated holder poisons its value route, not
# its cell box").
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

# (d10) the same box with a HEAP init the caller owns, reassigned away inside the
# callee: the cell's content accounting is the caller's, so the box is the only
# per-call region and the row reads it alone.
(defn cell-heap (s)
  (def @c s)
  (let [g (fn ()
            (assign c (length c))
            c)]
    (g)))

# (d11) the reassigned cell where the tail call sits in a branch ARM, so the box's
# release is relocated at that arm's own point.
(defn arm-cell (n t)
  (def @c n)
  (let [g (fn ()
            (assign c (%add c 1))
            c)]
    (if t (g) 0)))

# (d12) a sibling's compiled FORWARD CELL. A one-way sibling capture is not a
# cycle — `go` calls `helper` and `helper` does not call back — so no SCC forms and
# the closure-cycle merge never sees the cell; per-region RC is what reclaims both.
# The cell's own release is the binding-scope `DecrefRegion` the lowerer emits after
# the letrec body, which this body's frame-replacing tail call leaves dead. Its
# count argument cannot come from a holder binding, because a binding names the
# closure region its cell points AT, never the cell's own — so the cell rides its
# binding's verdict, its holders being that binding's holders one indirection out
# (docs/impl/region/mechanism.md § "A compiled capture cell is frame-held exactly as
# its binding is"). Stranding the cell strands the closure with it: the cell's
# reference is what holds that closure's region off zero.
(defn fwd-cell-plain (n)
  (letrec [helper (fn (x) (%sub x 1))
           go (fn (m) (helper m))]
    (go n)))

# (d13) the same shape with a SELF-RECURSIVE capturer, a closed control that bounds
# what the projection is responsible for. Here the ownership forest's capture adopt
# claims the cell into `go`'s closure region and suppresses its own decref, so `go`'s
# stranded-self deferral reclaims the pair and the relocation never has to reach the
# cell at all. It must stay bounded either way.
(defn fwd-cell (n)
  (letrec [helper (fn (x) (%sub x 1))
           go (fn (m) (if (%lt m 1) :done (go (helper m))))]
    (go n)))

# (d14) the RETURN half: `go` is handed back, so escape's capture facet marks
# `helper` escaping and the sole-held admission refuses. What admits the cell is the
# tail callee's own counted edge — and a `needs_capture` binding is captured THROUGH
# its cell, so the funding edge is `closure ⊇ cell` and must name the cell region as
# well as the closure it points at.
(defn fwd-cell-ret (n)
  (letrec [helper (fn (x)
                    (when (%not (%int? x)) (error :x))
                    (%sub x 1))
           go (fn (m)
                (when (%not (%int? m)) (error :m))
                (if (%lt m 1) go (go (helper m))))]
    (go n)))

# (d15) the same, returning the SIBLING rather than the capturer: the cell's own
# content is what leaves, so the projection must carry the verdict either way round.
(defn fwd-cell-ret-sib (n)
  (letrec [helper (fn (x)
                    (when (%not (%int? x)) (error :x))
                    (%sub x 1))
           go (fn (m)
                (when (%not (%int? m)) (error :m))
                (if (%lt m 1) helper (go (helper m))))]
    (go n)))

# (d15b) the SIBLING captures the self-recursive member, and the letrec body
# tail-calls the sibling. One `TailCall` carries BOTH deferred channels: the
# merged arena's `deferred_release_slot` (`go`'s closure, its env, and the forward
# cell the single-closure self-edge admission collapsed into it) and the sibling's
# own `defer_callee_release`. They name different regions and each drops a
# different reference the frame owns, so the runtime runs both — and reading them
# as alternatives reclaims nothing at all, because the sibling's counted
# `closure ⊇ cell` edge holds the arena off zero until the sibling's own region
# goes (docs/impl/region/letrec.md § "The arena channel and the callee channel are
# independent").
(defn fwd-cell-sib (n)
  (letrec [go (fn (m) (if (%lt m 1) :done (go (%sub m 1))))
           outer (fn (m) (go m))]
    (outer n)))

# (d16) the exemption reads an operand's VALUE, not its syntax
# (docs/impl/region/mechanism.md § "What an operand names is its VALUE, not its
# syntax"). Here the letrec body's tail call names `go` nowhere — its ARGUMENT is a
# call to `go`, so what the callee is handed is that call's RESULT, and `go`'s own
# closure region was read and finished with before the tail call was made. Its
# release sits at the letrec's scope end, past the frame-replacing `TailCall`, and
# the relocation is what carries it back: a self-recursive member is the tail
# callee's own region only when the body tail-calls IT (docs/impl/selfrec.md § the
# placement table). Two faces of the same reading: a sibling callee and a
# top-level callee.
(defn arg-call-selfrec (n)
  (letrec [helper (fn (x) (%sub x 1))
           go (fn (m) (if (%lt m 1) 0 (go (%sub m 1))))]
    (helper (go n))))
(defn top-sub (x)
  (%sub x 1))
(defn arg-call-toplevel (n)
  (letrec [go (fn (m) (if (%lt m 1) 0 (go (%sub m 1))))]
    (top-sub (go n))))

# (d18) the `def` face of the same three bodies. A `def` has no scope NODE, so the
# analysis leaves its closure region's demise where the binding chain put it — the
# binding's last use — and a use as a CALLEE resolves through `last_use` to the node
# that CONSUMES it. So the release is emitted where that call has returned and the
# recursion has completed, needing no relocation at all; only when the consuming
# call is itself the frame-replacing tail call is it dead, and there the deferral
# supplies it (`def-tail`, the closed control). What keeps the live rows off the
# `MakeClosure` itself is that a `def` evaluates to what it bound, so the
# unused-binding narrowing floors the demise at the `def` rather than at its
# initializer (docs/impl/region/mechanism.md § "A binder's init release lands after
# the slot store").
(defn def-arg-call (n)
  (def go (fn (m) (if (%lt m 1) 0 (go (%sub m 1)))))
  (top-sub (go n)))
(defn def-nontail (n)
  (def go (fn (m) (if (%lt m 1) 0 (go (%sub m 1)))))
  (%add (go n) 0))
(defn def-stmt (n)
  (def go (fn (m) (if (%lt m 1) 0 (go (%sub m 1)))))
  (go n)
  0)
(defn def-tail (n)
  (def go (fn (m) (if (%lt m 1) 0 (go (%sub m 1)))))
  (go n))

# (d17) the same reading's over-free face, in the leak direction: the operand's
# value-producing leaf IS an allocation, so its region stays exempt. A fresh lambda
# handed to the tail call is the callee's owned parameter, and the closure region
# the argument's `%pair` builds is the moved value itself — hoisting either would
# drop the reference the callee now owns. Bounded here and correct-valued in the
# uaf complement are the two halves of one claim.
(defn call-thunk (g)
  (g))
(defn lambda-arg (n)
  (call-thunk (fn () n)))
(defn aggregate-arg (n)
  (let [xs (list n n)]
    (top-sub (length (%pair xs nil)))))

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

# The same for a reassigned capture: the escaping closure carries the CELL, so the
# holder is refused and the box stays in the dead block. Driven for its VALUE — it
# strands by design, and what must hold is that the escaped closure can still read
# and rewrite the cell it captured.
(defn escaping-cell (n t)
  (def @c n)
  (let [g (fn ()
            (assign c (%add c 1))
            c)]
    (if t g (tail-sink))))

# residual ─────────────────────────────────────────────────────────────────────
# A returned holder the tail callee does NOT capture. `v` reaches a return through
# the OTHER arm, so no environment edge stands at this arm's relocation point to
# fund the release, and it keeps its place in the dead block. Driven for its
# VALUE, not its delta: it strands by design, and what must hold is that both arms
# still compute correctly.

(defn handback-unfunded (v t)
  (if t v (tail-sink)))

# residual: an env cell whose branch has a FALLING-THROUGH arm ─────────────────
# `arm-cell` above relocates its box at the tail-calling arm's own point, which is
# where the box's only `DecrefCellRegion` sits — so the sibling arm, which reaches
# the merge instead, releases nothing. The branch-arm release window is what
# anchors a single release where every arm reaches it, and it excludes cell
# regions: re-anchoring the box to the merge would take it back out of the arm it
# relocates in, and the merge's replica placement needs a self-cancelling run,
# which `LoadCaptureRaw` + `DecrefCellRegion` is not (it leaves the holder as it
# was, so a second copy would count twice). The strand is per env cell, not per
# reassignment — `arm-cell-ro` is the immutable face of the same shape. Driven for
# VALUE: both arms must still compute correctly.

(defn arm-cell-ro (n t)
  (def @c n)
  (let [g (fn () c)]
    (if t (g) 0)))

# residual: the letrec member the body tail-calls ─────────────────────────────
# `helper` is captured by its sibling, so it is allocated per call rather than
# seeded as a constant, and the letrec's body tail-calls it. Its own region is
# exempt from the relocation by design — moving that release ahead of the call
# would free the closure the call is about to enter — and the deferral does not
# claim it either, because `tail_callee_defers_release` reads a demise landing at
# the CALL node while the lowerer placed this one at the letrec's scope end.
# Neither channel a letrec body's tail callee can ride fits the shape: a one-way
# sibling capture is neither self-recursion (`stranded_self_bindings`) nor an SCC
# (`stranded_cycle_bindings`).
#
# Driven for its DELTA, printed rather than asserted, so a future session reads the
# measured rate off a test instead of prose.
(defn callee-letrec-member (n)
  (letrec [helper (fn (x) (%sub x 1))
           go (fn (m) (helper m))]
    (helper (go n))))

(def walk-d (measure (fn () (drive-walk [1 2 3])) 200 window))
(def walk-moved-d
  (measure (fn () (length (drive-walk-moved [1 2 3]))) 200 window))
(def arm-handback-d
  (measure (fn () (length (arm-handback [1 2 3] true))) 200 window))
(def cell-src [1 2 3])
(def cell-immutable-d (measure (fn () (cell-immutable 1)) 200 window))
(def cell-reassigned-d (measure (fn () (cell-reassigned 1)) 200 window))
(def cell-heap-d (measure (fn () (cell-heap cell-src)) 200 window))
(def arm-cell-t-d (measure (fn () (arm-cell 1 true)) 200 window))
(def fwd-cell-d (measure (fn () (fwd-cell 3)) 200 window))
(def fwd-cell-plain-d (measure (fn () (fwd-cell-plain 3)) 200 window))
(def fwd-cell-ret-d (measure (fn () (fwd-cell-ret 3)) 200 window))
(def fwd-cell-ret-sib-d (measure (fn () (fwd-cell-ret-sib 3)) 200 window))
(def fwd-cell-sib-d (measure (fn () (fwd-cell-sib 3)) 200 window))
(def def-arg-call-d (measure (fn () (def-arg-call 3)) 200 window))
(def def-nontail-d (measure (fn () (def-nontail 3)) 200 window))
(def def-stmt-d (measure (fn () (def-stmt 3)) 200 window))
(def def-tail-d (measure (fn () (def-tail 3)) 200 window))
(def arg-call-selfrec-d (measure (fn () (arg-call-selfrec 3)) 200 window))
(def arg-call-toplevel-d (measure (fn () (arg-call-toplevel 3)) 200 window))
(def callee-letrec-member-d
  (measure (fn () (callee-letrec-member 3)) 200 window))
(def lambda-arg-d (measure (fn () (lambda-arg 3)) 200 window))
(def aggregate-arg-d (measure (fn () (aggregate-arg 3)) 200 window))
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
(println "  walk-fill " walk-fill-d "  walk-moved " walk-moved-d
         "  arm-handback " arm-handback-d)
(println "  arms: unused " arm-unused-t-d "/" arm-unused-f-d "  two " arm-two-d
         "  cond " arm-cond-0-d "/" arm-cond-2-d "  match " arm-match-a-d "/"
         arm-match-z-d)
(println "  arms: nested " arm-nested-t-d "/" arm-nested-f-d "  partial "
         arm-partial-t-d "/" arm-partial-f-d)
(println "  exemptions: moved " moved-arg-d "  moved+stranded "
         moved-and-stranded-d "  callee-local " callee-local-d "  arm-moved "
         arm-moved-t-d "/" arm-moved-f-d)
(println "  cells: immutable " cell-immutable-d "  reassigned "
         cell-reassigned-d "  heap-init " cell-heap-d "  arm " arm-cell-t-d)
(println "  fwd cells: plain " fwd-cell-plain-d "  selfrec-control " fwd-cell-d
         "  returned " fwd-cell-ret-d "/" fwd-cell-ret-sib-d
         "  sibling-captures-member " fwd-cell-sib-d)
(println "  operand value: selfrec " arg-call-selfrec-d "  toplevel "
         arg-call-toplevel-d "  lambda " lambda-arg-d "  aggregate "
         aggregate-arg-d)
(println "  def binder: arg-call " def-arg-call-d "  nontail " def-nontail-d
         "  stmt " def-stmt-d "  tail " def-tail-d)
(println "  residual: callee-letrec-member " callee-letrec-member-d)
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
(bounded? walk-d "the accumulator a captured walker hands back")
(bounded? walk-moved-d
          "the hand-back where the captured edge is the only other reference")
(bounded? arm-handback-d "the hand-back reached through a branch arm's callee")

(bounded? cell-immutable-d "the env cell of a captured local")
(bounded? cell-reassigned-d "the env cell of a REASSIGNED captured local")
(bounded? cell-heap-d "the env cell of a reassigned capture with a heap init")
(bounded? arm-cell-t-d
          "the reassigned cell relocated at a branch arm's tail call")

(bounded? fwd-cell-plain-d
          "a sibling's forward cell past a frame-replacing body")
(bounded? fwd-cell-d
          "control: the same cell where the capture adopt already claims it")
(bounded? fwd-cell-ret-d "the forward cell of a capturer the frame hands back")
(bounded? fwd-cell-ret-sib-d "the forward cell whose own content is handed back")
(bounded? fwd-cell-sib-d
          "the arena and the sibling callee stranded by one tail call")

(bounded? arg-call-selfrec-d
          "a self-recursive member the tail call's ARGUMENT calls")
(bounded? arg-call-toplevel-d "the same under a top-level tail callee")
(bounded? lambda-arg-d "a fresh lambda handed to the tail call as its argument")
(bounded? aggregate-arg-d "the aggregate an argument builds around a local")

(bounded? def-arg-call-d
          "a `def`-bound self-recursive closure the tail call's ARGUMENT calls")
(bounded? def-nontail-d "the same `def` under a non-tail consumer")
(bounded? def-stmt-d "the same `def` called for effect")
(bounded? def-tail-d "control: the `def` whose body tail-calls the binding")

(assert (= (drive-walk [1 2 3]) 3) "walker result lost")
(assert (= (length (drive-walk-moved [1 2 3])) 3) "moved-in walker result lost")
(assert (= (length (arm-handback [1 2 3] true)) 3) "arm hand-back result lost")
(assert (= (arm-handback [1 2 3] false) 0) "arm hand-back sibling arm lost")
(assert (= (length (handback-unfunded [1 2 3] true)) 3)
        "unfunded hand-back result lost")
(assert (= (handback-unfunded [1 2 3] false) 0)
        "unfunded hand-back sibling arm lost")
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
(assert (= (arm-cell-ro 7 true) 7) "residual: arm immutable cell result lost")
(assert (= (arm-cell-ro 7 false) 0)
        "residual: arm immutable cell sibling arm lost")
(assert (= (fwd-cell 3) :done) "forward-cell walker result lost")
(assert (= (fwd-cell-sib 3) :done) "sibling-captures-member result lost")
(assert (= (def-arg-call 3) -1) "def-binder arg-call result lost")
(assert (= (def-nontail 3) 0) "def-binder nontail result lost")
(assert (= (def-stmt 3) 0) "def-binder statement result lost")
(assert (= (def-tail 3) 0) "def-binder tail result lost")
(assert (= (arg-call-selfrec 3) -1) "operand-value selfrec result lost")
(assert (= (arg-call-toplevel 3) -1) "operand-value toplevel result lost")
(assert (= (callee-letrec-member 3) 1)
        "residual: callee-letrec-member result lost")
(assert (= (lambda-arg 3) 3) "lambda argument result lost")
(assert (= (aggregate-arg 3) 0) "aggregate argument result lost")
(assert (= (fwd-cell-plain 3) 2) "plain forward-cell result lost")
# The returned capturer's base case hands back `go` itself, so driving it re-enters
# the recursion — every step derefs the cell to reach `helper`, after the defining
# frame is gone.
(assert (not (nil? ((fwd-cell-ret 3) 3)))
        "returned capturer must still be callable after its cell's release")
(assert (= ((fwd-cell-ret-sib 3) 9) 8)
        "returned sibling must still be callable after its cell's release")
(let [g (escaping-cell 7 true)]
  (assert (= (g) 8) "escaping cell first read lost")
  (assert (= (g) 9) "escaping cell rewrite lost"))
(assert (= (escaping-cell 7 false) 0) "escaping-cell sibling arm lost")

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
