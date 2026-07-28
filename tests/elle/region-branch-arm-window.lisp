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
# This file is the LEAK gauge — an `arena/count` delta over a fixed window, which
# must be BOUNDED for each placement, and for the three boundary shapes, whose
# releases must stay exactly where they are. The soundness complement is
# region-branch-arm-window-uaf.lisp; the per-op rates are the `param-used-arm`
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
# branch, so no arm leaves through a frame-replacing callee and the window's third
# boundary does not fire.
(defn used-captured (v t)
  (let [f (fn () (length v))]
    (%add (f)
          (match t
            :a (length v)
            :b (length v)
            _ (length v)))))

# boundaries ───────────────────────────────────────────────────────────────────
# Each drives the arm whose release must stay where it is. A hoist across a
# boundary would leave one release covering many allocations (the loop), a
# release emitted against another frame's slots (the lambda), or a release
# stranded past a frame replacement (the closure tail call) — all read as growth.

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

# A frame-replacing tail call in an arm: that arm leaves through the callee and
# never reaches the merge label, so the branch declines the window whole. Driven
# on the frame-exiting arm itself — the one whose own release a hoist to the
# merge would strand.
(defn bound-callee (v)
  (length v))
(defn bound-tailcall (v t)
  (match t
    :a (length v)
    :b (bound-callee v)
    _ 0))

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
(def bound-loop-d (measure (fn () (bound-loop :a)) 200 window))
(def bound-lambda-d (measure (fn () (bound-lambda :a)) 200 window))
(def bound-tailcall-d
  (measure (fn () (bound-tailcall (list 1 2 3) :b)) 200 window))
(def ctl-last-arm-d (measure (fn () (ctl-last-arm (list 1 2 3) :z)) 200 window))
(def ctl-one-arm-d (measure (fn () (ctl-one-arm (list 1 2 3) :a)) 200 window))

(println "region-branch-arm-window deltas over " window " iters:")
(println "  param " used-param-d "  if " used-param-if-d "  type-dispatch "
         type-dispatch-d)
(println "  two " used-two-d "  local " used-local-d "  captured "
         used-captured-d)
(println "  boundaries: loop " bound-loop-d "  lambda " bound-lambda-d
         "  tailcall " bound-tailcall-d)
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

(bounded? bound-loop-d "loop nested in an arm: per-iteration release")
(bounded? bound-lambda-d "lambda nested in an arm: per-activation release")
(bounded? bound-tailcall-d "closure tail call in an arm: the merge is unreached")

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
(assert (= (bound-loop :a) 0) "boundary loop body diverged")
(assert (= (bound-lambda :a) 0) "boundary lambda body diverged")
(assert (= (bound-tailcall (list 1 2 3) :b) 3)
        "boundary tail call body diverged")

(println "region-branch-arm-window: ok")
