(elle/epoch 12)
# audited: 2026-09-29
# A release one arm of a branch holds is anchored where every arm reaches it, so no arm strands the region.
# docs/impl/region/window.md
#
# A region's `decref_point` is the structurally-latest of its uses. When several
# arms of a branch use it, "latest" resolves to a node inside ONE arm — and arms
# are mutually exclusive, so every execution taking a different arm emits no
# release at all and holds the whole region (plus every member its free cascade
# would reclaim) to fiber teardown. The window re-anchors such a `decref_point`
# to `last_use[branch]`, the point every arm reaches; the release is the same
# single release, moved later.
#
# The dominant shape is the polymorphic entry point `(match (type-of a) …)` whose
# owned parameter is handed to a different callee per arm: without the window
# it pays the argument's whole object graph on every call that does not take
# the last arm naming it. Where a call site proves the argument's type the
# dispatch prunes to a single arm (src/hir/typeinfer/prune.rs) and never
# reaches this at all.
#
# Moving a release later is a PLACEMENT argument, and placement is enough only
# where the frame holds the region's one reference — so the re-anchoring is
# admitted only for a region escape proves does not leave its activation. Every
# subject below is such a region — including the one the arms reach only through a
# locally-called closure's environment, whose hold the allocation funnel counted.
# A region that escapes keeps the in-arm release and the per-arm compensation
# routes, and that decline is what the store / return / escaping-closure witnesses
# of region-branch-arm-window-uaf.lisp drive. region-branch-arm-tailcall.lisp
# covers the branch one of whose arms leaves through a frame-replacing callee.
#
# The two boundaries the window keeps are a nested loop and a nested lambda, and
# each is the scope's BODY rather than the scope's own node: the lowerer emits a
# node's releases after it, so a release anchored at the loop node already runs
# once per execution of the loop. That is where the loop-node extension puts every
# read of a live-in binding, so the distinction decides an ordinary class — the
# `arm-loop-read*` rows — while `bound-loop`, whose value is born in the loop body,
# must keep its release inside.
#
# The live-in premise is about the ALLOCATION, since that is both what "born in
# an arm" means and what the release's route follows: `region_to_slot` is keyed on
# a region's allocation site, so a binding whose init merely names another one
# records no slot and can never be the route. The `arm-alias-inside` row is the
# alias an arm introduces; `bound-loop` is the birth the premise keeps out.
#
# The same keying decides whose MUTATION matters. One binding owns the route, so a
# second name bound from the value — a cursor an arm walks with — repoints its own
# slot and leaves the allocating binding's alone. The `arm-cursor` and `each-list`
# rows are that shape, `each`'s list arm being where it is reached in production.
#
# An arm is a conditional POSITION, not a syntactic arm body. A `cond`'s clause
# tests are conditional positions exactly as its bodies are, and an `and`/`or` tail
# is one with no sibling body at all, so all three read their arms off the
# nested-`if` they are equivalent to. The `cond-*` and `*-short` rows drive that
# reading on the path that skips the position holding the release, with
# `ctl-cond-last-test` / `ctl-or-full` — the paths that do evaluate it — as the
# controls beside them.
#
# This file is the LEAK gauge — an `arena/count` delta over a fixed window, which
# must be BOUNDED for each placement, and for the two boundary shapes, whose
# releases must stay exactly where they are. The soundness complement is
# region-branch-arm-window-uaf.lisp; the per-op rates are the oracle's
# `param-used-arm`, `arm-alias-inside` and `arm-loop-read` rows
# (tests/impl/probe/branch.lisp), with `distinct`, `pipeline`, `wrap-map` and
# `push-accum` as the stdlib gauges of the `cond` reading.

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
# still drops only the frame's own. The closure is called OUTSIDE the branch,
# so every arm falls through to the merge and the anchor alone covers this row.
(defn used-captured (v t)
  (let [f (fn () (length v))]
    (%add (f)
          (match t
            :a (length v)
            :b (length v)
            _ (length v)))))

# (g) an arm whose LOOP reads the live-in parameter. A read of a loop-external
# binding is anchored at the loop NODE (docs/impl/region/anchors.md), and the
# lowerer emits a node's releases after it, so that release already runs once
# per execution of the loop — the same count with which the merge label is
# reached. Driving the arm that does NOT loop is what the anchor covers: without
# it the looping arm carries the branch's only release and every other arm
# strands the argument's whole object graph. The `bound-loop` boundary below is
# the contrast — its value is BORN in the loop body, so its release stays inside.
(defn arm-loop-read (v t)
  (match t
    :a (length v)
    _
      (begin
        (var i 0)
        (while (%lt i 3)
          (get v i)
          (assign i (%add i 1)))
        (%add i 100))))

# (h) the fn-LOCAL face of (g) — the same premise reached through the other route
# into `binding_source_regions`.
(defn arm-loop-read-local (t)
  (let [v (list 1 2 3)]
    (match t
      :a (length v)
      _
        (begin
          (var i 0)
          (while (%lt i 3)
            (get v i)
            (assign i (%add i 1)))
          (%add i 100)))))

# (i) the arm introduces an ALIAS of the live-in parameter — a binding whose init
# merely names it. Nothing is born in the arm, and the release's route is still
# the allocating binding's slot, since an init that names another binding
# records no slot of its own. Driven through the arm that does NOT alias, the
# one whose release is new.
(defn arm-alias-inside (v t)
  (match t
    :a (length v)
    _ (let [w v]
        (length w))))

# (j) the arm walks the live-in parameter with a reassigned CURSOR. The mutated
# refusal is about the release's ROUTE, and one binding owns it: `region_to_slot`
# is keyed on the allocation site, so the slot the release loads is `v`'s own,
# which no `assign` repoints — the cursor's init merely names `v` and records no
# slot at all. This is the `each` macro's list arm, whose `(def @cur seq)`
# would otherwise hold the whole cons chain per call. Driven through the arm
# that does NOT walk, the one whose release is new, and through the walking
# arm, whose release moved.
(defn arm-cursor (v t)
  (match t
    :a (length v)
    _
      (begin
        (def @cur v)
        (def @n 0)
        (while (pair? cur)
          (assign n (%add n 1))
          (assign cur (rest cur)))
        n)))

# (k) the `each` macro itself over a list — the production shape (j) isolates.
(defn each-list (v)
  (def @n 0)
  (each x in v
    (assign n (%add n 1)))
  n)

# (l) a `cond` whose LATER CLAUSE TEST names the live-in parameter. A clause test
# is a conditional position exactly as a clause body is — test k runs only where
# tests 0..k-1 all failed — so the arms are the nested-`if` the form is equivalent
# to: the clause body, and the rest of the chain from the next test on. Driven
# through the FIRST body, the path that never evaluates the test holding the
# release.
(defn cond-later-test (v t)
  (cond
    (%eq t 0) 1
    (%lt 0 (length v)) 2
    true 0))

# (m) the body half of the same decomposition, driven through the ELSE branch: the
# release sits in the first clause's body and no clause matches.
(defn cond-else-path (v t)
  (cond
    (%eq t 0) (length v)
    (%eq t 1) 2
    3))

# (n) the production dispatch `distinct` takes: a type `cond` naming the argument
# in every test, whose last test is the one no call reaches, so every call taking
# an earlier body would strand the argument's whole object graph. Reached through
# a binding holding the function as a VALUE, so the call site cannot prove the
# argument's type and every clause survives the dispatch prune.
(defn cond-dispatch (v)
  (cond
    (string? v) 0
    (array? v) (length v)
    (pair? v) 2
    0))
(def @cond-dispatch-ref cond-dispatch)

# (o) `or` short-circuits: the second element runs only where the first is falsy,
# so it is a conditional position with no sibling body — a one-armed branch.
# Driven through the short-circuiting path.
(defn or-short (v t)
  (if (or t (%lt 0 (length v))) 1 2))

# (p) the `and` face of the same rule: the tail runs only where the head is truthy.
(defn and-short (v t)
  (if (and t (%lt 0 (length v))) 1 2))

# boundaries ───────────────────────────────────────────────────────────────────
# Each drives the arm whose release must stay where it is. A hoist across a
# boundary would leave one release covering many allocations (the loop), or a
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
# Shapes bounded without the window: taking the arm that HOLDS the
# `decref_point`, and a single-arm dispatch with nothing to strand. A red subject
# above is the window and not the surrounding shape.
(defn ctl-last-arm (v t)
  (match t
    :a (length v)
    :b (length v)
    _ (length v)))
(defn ctl-one-arm (v t)
  (match t
    :a (length v)
    _ 0))

# The short-circuiting controls: the path that DOES evaluate the position holding
# the release is bounded without the window, so a red row above is the window and
# not the form.
(defn ctl-cond-last-test (v t)
  (cond
    (%eq t 0) 1
    (%lt 0 (length v)) 2
    true 0))
(defn ctl-or-full (v t)
  (if (or t (%lt 0 (length v))) 1 2))

(def used-param-d (measure (fn () (used-param (list 1 2 3) :a)) 200 window))
(def used-param-if-d
  (measure (fn () (used-param-if (list 1 2 3) true)) 200 window))
(def type-dispatch-d
  (measure (fn () (dispatch-ref (list 1 2 3) "x")) 200 window))
(def used-two-d (measure (fn () (used-two (list 1 2) (list 3 4) :a)) 200 window))
(def used-local-d (measure (fn () (used-local :a)) 200 window))
(def used-captured-d
  (measure (fn () (used-captured (list 1 2 3) :a)) 200 window))
(def arm-loop-read-d
  (measure (fn () (arm-loop-read (list 1 2 3) :a)) 200 window))
(def arm-loop-read-exit-d
  (measure (fn () (arm-loop-read (list 1 2 3) :z)) 200 window))
(def arm-loop-read-local-d (measure (fn () (arm-loop-read-local :a)) 200 window))
(def arm-alias-inside-d
  (measure (fn () (arm-alias-inside (list 1 2 3) :a)) 200 window))
(def arm-cursor-d (measure (fn () (arm-cursor (list 1 2 3) :a)) 200 window))
(def arm-cursor-walk-d (measure (fn () (arm-cursor (list 1 2 3) :z)) 200 window))
(def each-list-d (measure (fn () (each-list (list 1 2 3))) 200 window))
(def bound-loop-d (measure (fn () (bound-loop :a)) 200 window))
(def bound-lambda-d (measure (fn () (bound-lambda :a)) 200 window))
(def ctl-last-arm-d (measure (fn () (ctl-last-arm (list 1 2 3) :z)) 200 window))
(def ctl-one-arm-d (measure (fn () (ctl-one-arm (list 1 2 3) :a)) 200 window))
(def cond-later-test-d
  (measure (fn () (cond-later-test (list 1 2 3) 0)) 200 window))
(def cond-else-path-d
  (measure (fn () (cond-else-path (list 1 2 3) 9)) 200 window))
(def cond-dispatch-d
  (measure (fn () (cond-dispatch-ref (@array 1 2 3))) 200 window))
(def or-short-d (measure (fn () (or-short (list 1 2 3) true)) 200 window))
(def and-short-d (measure (fn () (and-short (list 1 2 3) false)) 200 window))
(def ctl-cond-last-test-d
  (measure (fn () (ctl-cond-last-test (list 1 2 3) 1)) 200 window))
(def ctl-or-full-d (measure (fn () (ctl-or-full (list 1 2 3) false)) 200 window))

(println "region-branch-arm-window deltas over " window " iters:")
(println "  param " used-param-d "  if " used-param-if-d "  type-dispatch "
         type-dispatch-d)
(println "  two " used-two-d "  local " used-local-d "  captured "
         used-captured-d)
(println "  arm loop reads live-in: param " arm-loop-read-d "  looping arm "
         arm-loop-read-exit-d "  local " arm-loop-read-local-d)
(println "  arm introduces an alias: " arm-alias-inside-d)
(println "  arm walks with a cursor: non-walking " arm-cursor-d "  walking "
         arm-cursor-walk-d "  each-list " each-list-d)
(println "  boundaries: loop " bound-loop-d "  lambda " bound-lambda-d)
(println "  cond: later test " cond-later-test-d "  else path " cond-else-path-d
         "  type dispatch " cond-dispatch-d)
(println "  short-circuit: or " or-short-d "  and " and-short-d)
(println "  controls: last-arm " ctl-last-arm-d "  one-arm " ctl-one-arm-d
         "  cond-last-test " ctl-cond-last-test-d "  or-full " ctl-or-full-d)

# Every leak in this class is at least one whole region per call, so a surviving
# strand reads ≥2000 over the window. 100 is slack for the one-time intercept.
(defn bounded? (d label)
  (assert (%lt d 100) (concat label " leaks, delta=" (number->string d))))

(bounded? ctl-last-arm-d "control: the arm holding the decref_point")
(bounded? ctl-one-arm-d "control: single-arm dispatch")
(bounded? ctl-cond-last-test-d "control: the cond path that runs the last test")
(bounded? ctl-or-full-d "control: the `or` path that runs both elements")

(bounded? cond-later-test-d "a cond clause body skipping a later clause's test")
(bounded? cond-else-path-d
          "a cond else branch skipping an earlier clause's body")
(bounded? cond-dispatch-d "a type cond naming its argument in every test")
(bounded? or-short-d "an `or` whose second element is short-circuited")
(bounded? and-short-d "an `and` whose second element is short-circuited")

(bounded? used-param-d "owned parameter used by an earlier arm")
(bounded? used-param-if-d "owned parameter used by an earlier `if` arm")
(bounded? type-dispatch-d "type dispatch over an unproven argument")
(bounded? used-two-d "two parameters stranded by one arm")
(bounded? used-local-d "fn-local live-in to the branch")
(bounded? used-captured-d
          "parameter the arms reach through a locally-called closure's env")

(bounded? arm-loop-read-d
          "arm whose loop reads the live-in parameter: non-looping arm")
(bounded? arm-loop-read-exit-d
          "arm whose loop reads the live-in parameter: the looping arm")
(bounded? arm-loop-read-local-d "arm whose loop reads a live-in fn-local")

(bounded? arm-alias-inside-d
          "an arm that introduces an alias of the live-in param")

(bounded? arm-cursor-d
          "an arm that walks the live-in param with a cursor: non-walking arm")
(bounded? arm-cursor-walk-d
          "an arm that walks the live-in param with a cursor: the walking arm")
(bounded? each-list-d "`each` over a list")

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
(assert (= (arm-loop-read (list 1 2 3) :a) 3) "arm-loop-read short arm lost")
(assert (= (arm-loop-read (list 1 2 3) :z) 103) "arm-loop-read looping arm lost")
(assert (= (arm-loop-read-local :z) 103) "arm-loop-read-local looping arm lost")
(assert (= (arm-alias-inside (list 1 2 3) :z) 3) "arm-alias-inside arm lost")
(assert (= (arm-cursor (list 1 2 3) :a) 3) "arm-cursor short arm lost")
(assert (= (arm-cursor (list 1 2 3) :z) 3) "arm-cursor walking arm lost")
(assert (= (each-list (list 1 2 3)) 3) "each-list result lost")
(assert (= (cond-later-test (list 1 2 3) 0) 1) "cond first-body result lost")
(assert (= (cond-later-test (list 1 2 3) 1) 2) "cond later-clause result lost")
(assert (= (cond-else-path (list 1 2 3) 9) 3) "cond else result lost")
(assert (= (cond-dispatch-ref (list 1 2 3)) 2) "cond dispatch pair arm lost")
(assert (= (cond-dispatch-ref [1 2 3]) 3) "cond dispatch array arm lost")
(assert (= (or-short (list 1 2 3) true) 1) "or short-circuit result lost")
(assert (= (or-short (list 1 2 3) false) 1) "or full-evaluation result lost")
(assert (= (and-short (list 1 2 3) false) 2) "and short-circuit result lost")
(assert (= (and-short (list 1 2 3) true) 1) "and full-evaluation result lost")
(assert (= (bound-loop :a) 0) "boundary loop body diverged")
(assert (= (bound-lambda :a) 0) "boundary lambda body diverged")

(println "region-branch-arm-window: ok")
