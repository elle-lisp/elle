(elle/epoch 12)
# A release inside one arm is not a release on the other arms
# (docs/impl/region/mechanism.md § "A release inside one arm is not a release on
# the other arms").
#
# A region's `decref_point` is the structurally-latest of its uses. When several
# arms of a branch use it, "latest" resolves to a node inside ONE arm — and arms
# are mutually exclusive, so every execution taking a different arm emits no
# release at all and holds the whole region (plus every member its free cascade
# would reclaim) to fiber teardown. The close re-anchors such a `decref_point` to
# `last_use[branch]`, the point every arm reaches; the release is the same single
# release, moved later.
#
# The dominant shape is the polymorphic entry point `(match (type-of a) …)` whose
# owned parameter is handed to a different callee per arm: it pays the argument's
# whole object graph on every call that does not take the last arm naming it.
# Where a call site proves the argument's type the dispatch prunes to a single
# arm (typeinfer/prune.rs) and never reaches this at all.
#
# Moving a release later is a PLACEMENT argument, and placement is enough only
# where the frame holds the region's one reference — so the re-anchoring is
# admitted only for a region escape proves does not leave its activation. Every
# subject below is such a region — including the one the arms reach only through a
# locally-called closure's environment, whose hold the allocation funnel counted.
# A region that escapes keeps the in-arm release and the per-arm compensation
# routes, and that decline is what the store / return / escaping-closure witnesses
# of region-branch-arm-window-uaf.lisp drive.
#
# An arm that leaves through a frame-replacing callee never reaches the merge, so
# the anchor alone does not cover it. The frame-exit relocation does: a merge owns
# the relocation points its arms sealed, so the anchored release is replicated
# ahead of each arm's `TailCall`, and an arm whose call NAMES the region keeps its
# copy in the dead block instead — that release is the ownership move the callee
# consumes (docs/impl/region/mechanism.md § "An arm that leaves through a callee
# takes a replica, not the anchor"). Such a branch is driven on BOTH kinds of arm
# below, since the two paths are covered by different halves of the composition.
#
# This file is the LEAK gauge — an `arena/count` delta over a fixed window, which
# must be BOUNDED for each placement, and for the two boundary shapes, whose
# releases must stay exactly where they are. The soundness complement is
# region-branch-arm-window-uaf.lisp; the per-op rates are the `param-used-arm`
# and `branch-arm-tailcall-sibling` probes in tests/elle/oracle.lisp.

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

# (a) an owned PARAMETER used by the taken arm while a later sibling holds the
# `decref_point`. The scrutinee is a tag, so no dispatch prune applies and the
# arms stay as written.
(defn used-param (v t)
  (match t
    :a (length v)
    :b (length v)
    _ (length v)))

# (b) the `If` face of the same shape — the window reads arm structure, not the
# branch's kind or arity.
(defn used-param-if (v c)
  (if c (length v) (%add 1 (length v))))

# (c) the production shape: a type dispatch over an owned parameter. Reached
# through a binding that holds the function as a VALUE, so the call site cannot
# prove the argument's type and every arm survives.
(defn type-dispatch (a b)
  (match (type-of a)
    :list (length a)
    :array (length a)
    :string (string a b)
    :struct (length (keys a))
    _ 0))
(def @dispatch-ref type-dispatch)

# (d) TWO parameters stranded by one arm: the window is per region, not per
# branch.
(defn used-two (x y t)
  (match t
    :a (%add (length x) (length y))
    :b (%add (length x) (length y))
    _ 0))

# (e) a fn-LOCAL (not a parameter) live-in to the branch — the same premise,
# reached through the other route into `binding_source_regions`.
(defn used-local (t)
  (let [v (list 1 2 3)]
    (match t
      :a (length v)
      :b (length v)
      _ (length v))))

# (f) the parameter the arms strand is also held by a locally-called CLOSURE's
# environment. That second holder is not one to fear: building the env took a
# counted reference through the allocation funnel, so the re-anchored release
# still drops only the frame's own (docs/impl/region/mechanism.md § "Lexical
# capture is not a second holder to fear"). The closure is called OUTSIDE the
# branch, so every arm falls through to the merge and the anchor alone covers this
# row — the replicated placement is rows (g) and (h) below.
(defn used-captured (v t)
  (let [f (fn () (length v))]
    (%add (f)
          (match t
            :a (length v)
            :b (length v)
            _ (length v)))))

# (g) a sibling arm leaves through a frame-replacing CLOSURE tail call that names
# the same parameter — the shape `append`/`concat` take, where the list arm hands
# the argument to `append-list`. Driving the OTHER arm is the leak this closes:
# `v`'s one release sits in the tail-calling arm, so every call that dispatches
# elsewhere strands the argument's whole object graph. Driving the tail-calling arm
# is the complement — its reference is the ownership move, and the callee's owned
# parameter release is what must still fire.
(defn tc-callee (v)
  (length v))
(defn tailcall-sibling (v t)
  (match t
    :a (length v)
    :b (tc-callee v)
    _ 0))

# (h) the tail-calling arm names NOTHING the other arms hold: the region's release
# is anchored at the merge and REPLICATED ahead of that arm's call, since no
# exemption applies to it. Driven on both the falling-through and the frame-exiting
# arm, which the two halves of the composition cover separately.
(defn tc-bare ()
  0)
(defn tailcall-elsewhere (v t)
  (match t
    :a (length v)
    :b (length v)
    :c (tc-bare)
    _ 0))

# boundaries ───────────────────────────────────────────────────────────────────
# Each drives the arm whose release must stay where it is. A hoist across a
# boundary would leave one release covering many allocations (the loop) or a
# release emitted against another frame's slots (the lambda) — both read as
# growth.

# A nested loop holding the `decref_point`: the loop body re-allocates per
# iteration, so `s`'s release must fire per iteration, not once after the branch.
(defn bound-loop (t)
  (match t
    :a
      (begin
        (var i 0)
        (while (%lt i 8)
          (let [s (list i i)]
            (length s))
          (assign i (%add i 1)))
        0)
    :b 1
    _ 2))

# A nested lambda holding it: its body's releases run in its own activation.
(defn bound-lambda (t)
  (match t
    :a
      (let [f (fn ()
                (let [s (list 1 2)]
                  (length s)))]
        (f)
        (f)
        0)
    :b 1
    _ 2))

# controls ─────────────────────────────────────────────────────────────────────
# Already-bounded shapes: taking the arm that HOLDS the `decref_point`, and a
# single-arm dispatch with nothing to strand. A red subject above is the window
# and not the surrounding shape.
(defn ctl-last-arm (v t)
  (match t
    :a (length v)
    :b (length v)
    _ (length v)))
(defn ctl-one-arm (v t)
  (match t
    :a (length v)
    _ 0))

(def used-param-d (measure (fn () (used-param (list 1 2 3) :a)) 200 window))
(def used-param-if-d
  (measure (fn () (used-param-if (list 1 2 3) true)) 200 window))
(def type-dispatch-d
  (measure (fn () (dispatch-ref (list 1 2 3) "x")) 200 window))
(def used-two-d (measure (fn () (used-two (list 1 2) (list 3 4) :a)) 200 window))
(def used-local-d (measure (fn () (used-local :a)) 200 window))
(def used-captured-d
  (measure (fn () (used-captured (list 1 2 3) :a)) 200 window))
(def tailcall-sibling-fallthrough-d
  (measure (fn () (tailcall-sibling (list 1 2 3) :a)) 200 window))
(def tailcall-sibling-exit-d
  (measure (fn () (tailcall-sibling (list 1 2 3) :b)) 200 window))
(def tailcall-elsewhere-fallthrough-d
  (measure (fn () (tailcall-elsewhere (list 1 2 3) :a)) 200 window))
(def tailcall-elsewhere-exit-d
  (measure (fn () (tailcall-elsewhere (list 1 2 3) :c)) 200 window))
(def bound-loop-d (measure (fn () (bound-loop :a)) 200 window))
(def bound-lambda-d (measure (fn () (bound-lambda :a)) 200 window))
(def ctl-last-arm-d (measure (fn () (ctl-last-arm (list 1 2 3) :z)) 200 window))
(def ctl-one-arm-d (measure (fn () (ctl-one-arm (list 1 2 3) :a)) 200 window))

(println "region-branch-arm-window deltas over " window " iters:")
(println "  param " used-param-d "  if " used-param-if-d "  type-dispatch "
         type-dispatch-d)
(println "  two " used-two-d "  local " used-local-d "  captured "
         used-captured-d)
(println "  tail-calling sibling: names-arg fallthrough "
         tailcall-sibling-fallthrough-d "  exit " tailcall-sibling-exit-d)
(println "  tail-calling sibling: names-none fallthrough "
         tailcall-elsewhere-fallthrough-d "  exit " tailcall-elsewhere-exit-d)
(println "  boundaries: loop " bound-loop-d "  lambda " bound-lambda-d)
(println "  controls: last-arm " ctl-last-arm-d "  one-arm " ctl-one-arm-d)

# Every leak in this class is at least one whole region per call, so a surviving
# strand reads ≥2000 over the window. 100 is slack for the one-time intercept.
(defn bounded? (d label)
  (assert (%lt d 100) (concat label " leaks, delta=" (number->string d))))

(bounded? ctl-last-arm-d "control: the arm holding the decref_point")
(bounded? ctl-one-arm-d "control: single-arm dispatch")

(bounded? used-param-d "owned parameter used by an earlier arm")
(bounded? used-param-if-d "owned parameter used by an earlier `if` arm")
(bounded? type-dispatch-d "type dispatch over an unproven argument")
(bounded? used-two-d "two parameters stranded by one arm")
(bounded? used-local-d "fn-local live-in to the branch")
(bounded? used-captured-d
          "parameter the arms reach through a locally-called closure's env")

(bounded? tailcall-sibling-fallthrough-d
          "arm falling through while a sibling tail-calls with the parameter")
(bounded? tailcall-sibling-exit-d
          "arm tail-calling with the parameter: the ownership move")
(bounded? tailcall-elsewhere-fallthrough-d
          "arm falling through while a sibling tail-calls naming nothing")
(bounded? tailcall-elsewhere-exit-d
          "arm tail-calling naming nothing: the replicated release")

(bounded? bound-loop-d "loop nested in an arm: per-iteration release")
(bounded? bound-lambda-d "lambda nested in an arm: per-activation release")

# Value preservation: re-anchoring a release must not change what runs.
(assert (= (used-param (list 1 2 3) :a) 3) "param arm result lost")
(assert (= (used-param (list 1 2 3) :z) 3) "param wildcard arm result lost")
(assert (= (used-param-if (list 1 2 3) true) 3) "if then-arm result lost")
(assert (= (used-param-if (list 1 2 3) false) 4) "if else-arm result lost")
(assert (= (dispatch-ref (list 1 2 3) "x") 3) "type dispatch list arm lost")
(assert (= (dispatch-ref "ab" "c") "abc") "type dispatch string arm lost")
(assert (= (used-two (list 1 2) (list 3 4) :a) 4) "two-param arm result lost")
(assert (= (used-local :b) 3) "local arm result lost")
(assert (= (used-captured (list 1 2 3) :b) 6) "captured arm result lost")
(assert (= (tailcall-sibling (list 1 2 3) :a) 3)
        "tail-calling sibling: fall-through arm result lost")
(assert (= (tailcall-sibling (list 1 2 3) :b) 3)
        "tail-calling sibling: frame-exiting arm result lost")
(assert (= (tailcall-elsewhere (list 1 2 3) :a) 3)
        "bare tail-calling sibling: fall-through arm result lost")
(assert (= (tailcall-elsewhere (list 1 2 3) :c) 0)
        "bare tail-calling sibling: frame-exiting arm result lost")
(assert (= (bound-loop :a) 0) "boundary loop body diverged")
(assert (= (bound-lambda :a) 0) "boundary lambda body diverged")

(println "region-branch-arm-window: ok")
