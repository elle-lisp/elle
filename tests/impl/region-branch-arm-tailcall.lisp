(elle/epoch 13)
# audited: 2026-09-29
# A branch whose arm leaves through a frame-replacing tail call strands nothing on either kind of arm.
# docs/impl/region/window.md
# docs/impl/region/relocate.md
#
# region-branch-arm-window.lisp anchors a release that one arm holds at the
# merge every arm reaches. An arm that leaves through a frame-replacing callee
# never reaches the merge, so the anchor alone does not cover it. The frame-exit
# relocation does: a merge owns the relocation points its arms sealed, so the
# anchored release is replicated ahead of each arm's `TailCall`, and an arm
# whose call NAMES the region keeps its copy in the dead block instead — that
# release is the ownership move the callee consumes. Each branch here is driven
# on BOTH kinds of arm, since the two paths are covered by different halves of
# the composition.
#
# A replica has to be a release that names a VALUE, so the branch is narrowed
# to the regions a value route can name rather than to a class of region:
# releasing by id is the lowerer's default, and a region a binder allocated has
# a slot naming its value from the binder to the release. The `binder-routed`
# rows are that reading, driven through an arm that USES the subject — an arm
# that does not is already covered by the per-arm head compensation.
#
# One escape facet is admitted rather than refused. A RETURNED region costs the
# merge no funding edge — the arm that hands it over ran its mint before
# jumping there — and neither does a replica ahead of a `TailCall`, which runs
# before the callee's mint: a callee reaches a value this frame owns as an
# operand or through its captured environment and by no other route, so it
# either holds a counted edge or cannot mint against the region at all. Both
# ends are rows below: the captured one is `push-all`'s shape, and the
# unreached one is a walker the arm hands a fresh value instead.
#
# This file is the LEAK gauge — an `arena/count` delta over a fixed window,
# which must be BOUNDED on every path. The soundness complement is
# region-branch-arm-window-uaf.lisp; the per-op rates are the oracle's
# `branch-arm-tailcall-sibling` and `branch-arm-return-captured` rows
# (tests/impl/probe/branch.lisp).

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

# (a) a sibling arm leaves through a frame-replacing CLOSURE tail call that
# names the same parameter — the shape `append`/`concat` take, where the list
# arm hands the argument to `append-list`. Driving the OTHER arm reads the
# anchor: `v`'s one release sits in the tail-calling arm, so a missing anchor
# strands the argument's whole object graph on every call that dispatches
# elsewhere. Driving the tail-calling arm is the complement — its reference is
# the ownership move, and the callee's owned parameter release is what must
# still fire.
(defn tc-callee (v)
  (length v))
(defn tailcall-sibling (v t)
  (match t
    :a (length v)
    :b (tc-callee v)
    _ 0))

# (b) the tail-calling arm names NOTHING the other arms hold: the region's
# release is anchored at the merge and REPLICATED ahead of that arm's call,
# since no exemption applies to it. Driven on both the falling-through and the
# frame-exiting arm, which the two halves of the composition cover separately.
(defn tc-bare ()
  0)
(defn tailcall-elsewhere (v t)
  (match t
    :a (length v)
    :b (length v)
    :c (tc-bare)
    _ 0))

# (c) the subject is a fn-local the BINDER allocated with an inline `%`-opcode,
# so it is no call result and the lowerer's default release names it by region
# id. A sibling arm still leaves through a frame-replacing callee, so the
# branch is admitted only where the window asks the question the emitter asks
# rather than reading the region's class: `region_to_slot` is keyed on the
# allocation site, so the `let` binder's slot names this value from the binder
# to the release and the relocation can replicate it. Driven through an arm
# that USES the subject — an arm that does not is already covered by the
# per-arm head compensation, so a used sibling is the path a class reading
# strands.
(defn binder-routed (t)
  (let [v (%pair 1 nil)]
    (match t
      :a (length v)
      :b (length v)
      :c (tc-bare)
      _ 0)))

# (d) the parameter is RETURNED by the arm that runs, while the sibling arm —
# the one that holds the `decref_point` — hands it to a local walker it
# tail-calls. The return facet costs the merge nothing: this arm's own return
# mint has already raised the count when the anchored release drops the
# frame's reference. The sibling's replica is funded by the walker's
# captured-holder edge, so the branch is admitted for the class. This is
# `push-all` over a byte-family source, and with it every `append`/`concat`
# that takes one.
(defn returned-captured (dst src)
  (if (%eq (type-of src) :string)
    (begin
      (push dst src)
      dst)
    (let [n (length src)]
      (letrec [go (fn (i)
                    (if (%lt i n)
                      (begin
                        (push dst (get src i))
                        (go (%add i 1)))
                      dst))]
        (go 0)))))

# (e) the other end of the same enumeration: the sibling arm tail-calls a callee
# that neither names the accumulator nor captures it — a self-recursive walker
# whose next `acc` is a fresh value built from this one. That callee cannot
# reach the region, so its `Return` mints nothing against it and the replica
# ahead of the `TailCall` is the region's last release. Both arms are driven:
# the recursive one runs the replica, the base one the anchor.
(def acc-walk (fn (i acc) (if (%lt i 0) acc (acc-walk (%sub i 1) (pair i acc)))))

# boundary ─────────────────────────────────────────────────────────────────────

# The CALLEE an exiting arm tail-calls. Its own closure region is what that call
# names, so the frame-exit relocation exempts it and replicates nothing into the
# arm — the deferred callee channel runs that release from where it sits
# instead. Anchoring it at the merge would take it out of that channel's reach
# and leave the tail-calling path with no release at all, so it stays in the
# arm. Driven on both paths; the leak this boundary prevents is one closure
# region per call, and it compounds with the depth of a tower of stdlib HOF
# compositions.
(defn bound-callee (t)
  (letrec [go (fn (a b) a)]
    (if (%eq t 0) 0 (go t 1))))

(def tailcall-sibling-fallthrough-d
  (measure (fn () (tailcall-sibling (list 1 2 3) :a)) 200 window))
(def tailcall-sibling-exit-d
  (measure (fn () (tailcall-sibling (list 1 2 3) :b)) 200 window))
(def tailcall-elsewhere-fallthrough-d
  (measure (fn () (tailcall-elsewhere (list 1 2 3) :a)) 200 window))
(def tailcall-elsewhere-exit-d
  (measure (fn () (tailcall-elsewhere (list 1 2 3) :c)) 200 window))
(def binder-routed-used-d (measure (fn () (binder-routed :a)) 200 window))
(def binder-routed-last-d (measure (fn () (binder-routed :b)) 200 window))
(def binder-routed-exit-d (measure (fn () (binder-routed :c)) 200 window))
(def returned-captured-fallthrough-d
  (measure (fn () (returned-captured (@string) "xy")) 200 window))
(def returned-captured-exit-d
  (measure (fn () (returned-captured (@array) [1 2])) 200 window))
(def acc-walk-d (measure (fn () (acc-walk 3 ())) 200 window))
(def bound-callee-d (measure (fn () (bound-callee 1)) 200 window))
(def bound-callee-fallthrough-d (measure (fn () (bound-callee 0)) 200 window))

(println "region-branch-arm-tailcall deltas over " window " iters:")
(println "  tail-calling sibling: names-arg fallthrough "
         tailcall-sibling-fallthrough-d "  exit " tailcall-sibling-exit-d)
(println "  tail-calling sibling: names-none fallthrough "
         tailcall-elsewhere-fallthrough-d "  exit " tailcall-elsewhere-exit-d)
(println "  binder-routed local, tail-calling sibling: used arm "
         binder-routed-used-d "  last-use arm " binder-routed-last-d "  exit "
         binder-routed-exit-d)
(println "  returned + captured sibling: fallthrough "
         returned-captured-fallthrough-d "  exit " returned-captured-exit-d)
(println "  returned + unreached sibling: acc-walk " acc-walk-d)
(println "  boundary: tail callee " bound-callee-d "  callee fall-through "
         bound-callee-fallthrough-d)

# Every leak in this class is at least one whole region per call, so a surviving
# strand reads ≥2000 over the window. 100 is slack for the one-time intercept.
(defn bounded? (d label)
  (assert (%lt d 100) (concat label " leaks, delta=" (number->string d))))

(bounded? tailcall-sibling-fallthrough-d
          "arm falling through while a sibling tail-calls with the parameter")
(bounded? tailcall-sibling-exit-d
          "arm tail-calling with the parameter: the ownership move")
(bounded? tailcall-elsewhere-fallthrough-d
          "arm falling through while a sibling tail-calls naming nothing")
(bounded? tailcall-elsewhere-exit-d
          "arm tail-calling naming nothing: the replicated release")
(bounded? binder-routed-used-d
          "binder-routed local used by an arm while a sibling tail-calls")
(bounded? binder-routed-last-d
          "binder-routed local: the arm holding the decref_point")
(bounded? binder-routed-exit-d
          "binder-routed local: the frame-exiting arm that names nothing")
(bounded? returned-captured-fallthrough-d
          "arm returning the parameter while a capturing sibling tail-calls")
(bounded? returned-captured-exit-d
          "capturing sibling arm: the walker's own return of the parameter")
(bounded? acc-walk-d
          "returned accumulator the sibling arm's callee cannot reach")
(bounded? bound-callee-d
          "the callee an exiting arm tail-calls: the deferred callee channel")
(bounded? bound-callee-fallthrough-d
          "the callee an exiting arm tail-calls: the fall-through path")

# Value preservation: re-anchoring a release must not change what runs.
(assert (= (tailcall-sibling (list 1 2 3) :a) 3)
        "tail-calling sibling: fall-through arm result lost")
(assert (= (tailcall-sibling (list 1 2 3) :b) 3)
        "tail-calling sibling: frame-exiting arm result lost")
(assert (= (tailcall-elsewhere (list 1 2 3) :a) 3)
        "bare tail-calling sibling: fall-through arm result lost")
(assert (= (tailcall-elsewhere (list 1 2 3) :c) 0)
        "bare tail-calling sibling: frame-exiting arm result lost")
(assert (= (binder-routed :a) 1) "binder-routed used arm result lost")
(assert (= (binder-routed :c) 0) "binder-routed frame-exiting arm result lost")
(assert (= (returned-captured (@string) "xy") "xy")
        "returned-captured bulk arm result lost")
(assert (= (length (returned-captured (@array) [1 2])) 2)
        "returned-captured walk arm result lost")
(assert (= (length (acc-walk 3 ())) 4) "acc-walk result lost")
(assert (= (bound-callee 1) 1) "boundary tail-callee arm result lost")
(assert (= (bound-callee 0) 0) "boundary tail-callee fall-through result lost")

(println "region-branch-arm-tailcall: ok")
