(elle/epoch 12)
# A release the `break` jumps over is not a release
# (docs/impl/region/mechanism.md § "A release the break jumps over is not a
# release").
#
# `break` lowers to a store into the block's result slot plus a jump to the
# block's exit label. The value the break CARRIES is transferred to the block
# and dies where the block's value dies (region-break-transfer.lisp). Every
# OTHER region whose release sits between the break site and that exit label has
# no consumer to be handed to: its release is emitted into code the jump passes
# over, so it never executes and the region is held to fiber teardown — one
# region per break, per skipped release.
#
# The close re-anchors those regions to the same point the broken value takes,
# `last_use[block]`, which the break path and the fall-through path both reach.
# Three boundaries stop the hoist, and each is narrower than the scope it sits
# in: an iterative scope nested in the block, for a region that scope ALLOCATES
# (re-allocated per iteration, so one release cannot cover N — a region the loop
# only reads is one per activation and hoists); a lambda nested in the block,
# unconditionally (its releases run in another activation, against another
# frame's slots); and a frame-replacing tail call on the block's FALL-THROUGH
# (that path leaves through the callee, so the exit label is not a point every
# path reaches — one inside a break's own value is on a path that already jumps
# over these releases and refuses nothing).
#
# This file is the LEAK gauge — an `arena/region-count` delta over a fixed
# window, which must be BOUNDED for every placement of a skipped release, and
# for the three boundary shapes, whose releases must stay exactly where they are.
# The soundness complement is region-break-skip-uaf.lisp; the per-op rate is the
# `break-skipped` probe in tests/elle/oracle.lisp.

(def window 2000)

(defn measure (thunk warm window)
  (var i 0)
  (while (%lt i warm)
    (thunk)
    (assign i (%add i 1)))
  (def before (arena/region-count))
  (var j 0)
  (while (%lt j window)
    (thunk)
    (assign j (%add j 1)))
  (%sub (arena/region-count) before))

(defn mk ()
  {:a 1})

# A fresh heap value the CALLER allocates, so the callee's parameter names a
# region the caller owns and the callee releases.
(defn mk-string ()
  (concat "ab" "cd"))

# subjects ─────────────────────────────────────────────────────────────────────

# (a) a CALL RESULT used after the break site — released by the value route
# through its binding slot.
(defn skip-call (n)
  (block (let [x (mk)]
           (when (%gt n 0) (break 1))
           (%struct? x)))
  nil)

# (b) a heap LITERAL in the window — released by the static region route
# (`DecrefRegion` against the slot), the other half of the emit split.
(defn skip-literal (n)
  (block (let [x {:a 1}]
           (when (%gt n 0) (break 1))
           (%struct? x)))
  nil)

# (c) two live values in the window: the hoist is per region, not per block.
(defn skip-two (n)
  (block (let [x (mk)
               y (mk)]
           (when (%gt n 0) (break 1))
           (%add (length (keys x)) (length (keys y)))))
  nil)

# (d) the break is nested two branches deep — the window is read off the
# structural order, not off the break's syntactic depth.
(defn skip-deep-break (n)
  (block (let [x (mk)]
           (if (%gt n 0) (if (%gt n -1) (break 1) nil) nil)
           (%struct? x)))
  nil)

# (e) the break crosses a NESTED block on its way out: the skipped release sits
# in the inner block, the anchor is the outer one.
(defn skip-nested-block (n)
  (block :outer
    (block :inner
      (let [x (mk)]
        (when (%gt n 0) (break :outer 1))
        (%struct? x))))
  nil)

# (f) the block's result is CONSUMED, so the anchor is the consuming node rather
# than the block itself — the skipped region rides the same pin.
(defn skip-used (n)
  (%add 0
        (block (let [x (mk)]
                 (when (%gt n 0) (break 1))
                 (%add 2 (length (keys x)))))))

# (g) the block is the function's TAIL. Every subject above ends its body with a
# trailing `nil`, which keeps the block out of tail position and so keeps a
# `Return` out of the window; a lookup helper is not written that way. The tail
# value and the break's value are both `Return`-wrapped here, and a `Return` is
# the return MINT — control falls through it to the exit label — so neither is a
# frame-replacing exit and the window still applies.
(defn skip-tail-block (n)
  (block (let [x (mk)]
           (when (%gt n 0) (break 1))
           (%struct? x))))

# (h) the same, with the break carrying a FRESH value rather than an immediate.
# The carried value takes the return mint; the skipped `x` has no mint and must
# still be released at the anchor.
(defn skip-tail-heap-break (n)
  (block (let [x (mk)]
           (when (%gt n 0) (break {:b 2}))
           x)))

# (i) a tail block with SEVERAL breaks, each carrying a heap literal — the shape
# a lookup helper is written in, and the one where the frame-exit boundary has
# to read which path an exit is on. A break's value is walked as a tail value in
# a tail block, so each `{…}` here is a tail-marked call sitting in the window.
# An exit inside a break's own value is on a path that already jumps over these
# releases, so it does not refuse the window; only one on the fall-through does
# (`bound-tailcall`).
# Each guard names one value of `n`, so every break is reachable on its own and
# the value asserts below exercise all three exits rather than only the first.
(defn skip-tail-many-breaks (n)
  (block (let [x (mk)]
           (when (= n 1) (break {:b 1}))
           (when (= n 2) (break {:b 2}))
           (when (= n 3) (break {:b 3}))
           (%struct? x))))

# boundaries ───────────────────────────────────────────────────────────────────
# Each drives a break that never fires, so the guarded code RUNS and its releases
# must fire where they were placed. A hoist that crossed a boundary would leave
# one release covering many allocations (the loop), a release emitted in the
# wrong frame (the lambda), or a release stranded past a frame replacement (the
# tail call) — all three read as growth here.

(defn bound-loop (n)
  (block (when (%lt n 0) (break 1))
    (def @i 0)
    (while (%lt i 8)
      (let [x (mk)]
        (%struct? x))
      (assign i (%add i 1)))
    nil))

# The loop barrier is read off the ALLOCATION site, not off where the release
# sits. `s` is a parameter — allocated once by the caller — whose last use is
# inside the loop, so its release sits there while the value is still one per
# activation. One release at the anchor covers it exactly once. The break here
# DOES fire, so a refusal strands `s` on every call.
(defn skip-loop-read-param (n s)
  (block (when (%gt n 0) (break 1))
    (def @i 0)
    (while (%lt i 4)
      (when (%eq (length s) 999) (break 2))
      (assign i (%add i 1)))
    nil))

(defn bound-lambda (n)
  (block (when (%lt n 0) (break 1))
    (let [f (fn ()
              (let [x (mk)]
                (%struct? x)))]
      (f)
      (f))
    nil))

# The third boundary is the anchor itself: a frame-replacing tail call in the
# body means the fall-through path leaves through the callee instead of arriving
# at the exit label, so a release moved there would be dead on exactly the path
# that used to run it. The block declines the window and `x` keeps its own
# release, which the break-free call below must still run.
(defn bound-tailcall-callee (n)
  (%add n 1))
(defn bound-tailcall (n)
  (block (when (%lt n 0) (break 1))
    (let [x (mk)]
      (%struct? x))
    (bound-tailcall-callee n)))

# controls ─────────────────────────────────────────────────────────────────────
# The same body with the break unreachable, and with the value's last use BEFORE
# the break site — both already bounded, so a red subject above is the skip and
# not the surrounding shape.
(defn ctl-nobreak (n)
  (block (let [x (mk)]
           (when (%lt n 0) (break 1))
           (%struct? x)))
  nil)
(defn ctl-before-break (n)
  (block (let [x (mk)]
           (%struct? x))
    (when (%gt n 0) (break 1))
    0)
  nil)

(def skip-call-d (measure (fn () (skip-call 1)) 200 window))
(def skip-literal-d (measure (fn () (skip-literal 1)) 200 window))
(def skip-two-d (measure (fn () (skip-two 1)) 200 window))
(def skip-deep-break-d (measure (fn () (skip-deep-break 1)) 200 window))
(def skip-nested-block-d (measure (fn () (skip-nested-block 1)) 200 window))
(def skip-used-d (measure (fn () (skip-used 1)) 200 window))
(def skip-tail-block-d (measure (fn () (skip-tail-block 1)) 200 window))
(def skip-tail-heap-break-d
  (measure (fn () (skip-tail-heap-break 1)) 200 window))
(def skip-tail-many-breaks-d
  (measure (fn () (skip-tail-many-breaks 1)) 200 window))
(def skip-loop-read-param-d
  (measure (fn () (skip-loop-read-param 1 (mk-string))) 200 window))
(def bound-loop-d (measure (fn () (bound-loop 1)) 200 window))
(def bound-lambda-d (measure (fn () (bound-lambda 1)) 200 window))
(def bound-tailcall-d (measure (fn () (bound-tailcall 1)) 200 window))
(def ctl-nobreak-d (measure (fn () (ctl-nobreak 1)) 200 window))
(def ctl-before-break-d (measure (fn () (ctl-before-break 1)) 200 window))

(println "region-break-skip deltas over " window " iters:")
(println "  call " skip-call-d "  literal " skip-literal-d "  two " skip-two-d
         "  deep " skip-deep-break-d)
(println "  nested-block " skip-nested-block-d "  used " skip-used-d)
(println "  tail-block " skip-tail-block-d "  tail-heap-break "
         skip-tail-heap-break-d "  tail-many-breaks " skip-tail-many-breaks-d)
(println "  loop-read-param " skip-loop-read-param-d)
(println "  boundaries: loop " bound-loop-d "  lambda " bound-lambda-d
         "  tailcall " bound-tailcall-d)
(println "  controls: nobreak " ctl-nobreak-d "  before-break "
         ctl-before-break-d)

# Every leak in this class is at least one whole region per break, so a surviving
# over-keep reads ~2000 over the window. 100 is slack for the one-time intercept.
(defn bounded? (d label)
  (assert (%lt d 100) (concat label " leaks, delta=" (number->string d))))

(bounded? ctl-nobreak-d "control: break unreachable")
(bounded? ctl-before-break-d "control: last use before the break site")

(bounded? skip-call-d "skipped release of a call result")
(bounded? skip-literal-d "skipped release of a heap literal")
(bounded? skip-two-d "two skipped releases in one window")
(bounded? skip-deep-break-d "skipped release under a nested break")
(bounded? skip-nested-block-d "skipped release inside a nested block")
(bounded? skip-used-d "skipped release under a consumed block result")
(bounded? skip-tail-block-d "skipped release in a block in TAIL position")
(bounded? skip-tail-heap-break-d
          "skipped release beside a break carrying a fresh value")
(bounded? skip-tail-many-breaks-d
          "skipped release in a tail block with several breaks")
(bounded? skip-loop-read-param-d
          "skipped release of a param the loop only reads")

(bounded? bound-loop-d "loop nested in the window: per-iteration release")
(bounded? bound-lambda-d "lambda nested in the window: per-activation release")
(bounded? bound-tailcall-d
          "tail call in the window: the exit label is unreached")

# Value preservation: re-anchoring a release must not change what runs.
(assert (= (skip-used 1) 1) "breaking block result lost")
(assert (= (skip-used 0) 3) "fall-through block result lost")
(assert (= (bound-loop 1) nil) "boundary loop body diverged")
(assert (= (bound-lambda 1) nil) "boundary lambda body diverged")
(assert (= (bound-tailcall 1) 2) "boundary tail call body diverged")
(assert (= (skip-tail-block 1) 1) "tail block: breaking result lost")
(assert (= (skip-tail-block 0) true) "tail block: fall-through result lost")
(assert (= (skip-tail-heap-break 1) {:b 2})
        "tail block: the value the break carried did not survive")
(assert (= (skip-tail-heap-break 0) {:a 1})
        "tail block: the fall-through value did not survive")
(assert (= (skip-tail-many-breaks 1) {:b 1}) "tail block: first break lost")
(assert (= (skip-tail-many-breaks 2) {:b 2}) "tail block: second break lost")
(assert (= (skip-tail-many-breaks 3) {:b 3}) "tail block: third break lost")
(assert (= (skip-tail-many-breaks 0) true)
        "tail block: many-break fall-through lost")

(println "region-break-skip: ok")
