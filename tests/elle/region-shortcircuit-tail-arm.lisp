(elle/epoch 12)
# audited: 2026-09-08
# A short-circuit operand is an arm
# (docs/impl/region/replicate.md § "A short-circuit operand is an arm").
#
# `and` and `or` carry no branch in the source and the lowerer gives them one:
# each operand stores its value into the result slot, and every operand but the
# last branches on that value — to the done block where the answer is settled, to
# the next operand otherwise. So the done block is a merge reached through every
# operand's block, and the operands are its arms.
#
# Only the LAST operand inherits tail position, so it is the only arm that can
# carry a frame-replacing tail call. That one arm is enough: a closure callee
# replaces the frame, so every release the enclosing scope emits past the merge is
# skipped on each call the earlier operands do not settle. A parameter's own
# owned-param release is one of those, so the caller's `CallArgument` reference is
# stranded once per such call and everything the argument's region holds strands
# behind it.
#
# THE TRAP the controls guard. The leak is not a fact about `and`/`or`; it is a
# fact about the CALLEE in the last operand. Rows (e) and (f) hold the shape fixed
# and vary only what the arm reaches — an intrinsic that pushes no frame, and a
# short-circuit that never enters the arm — so a reading that blamed the operator
# would move them too.
#
# THE COUNTER-FACTUAL row (c) catches. The close replicates the merge's release
# ahead of the arm's `TailCall`, and a reading that replicated every region would
# free the one the call is moving INTO the callee — whose owned-param release then
# drops a reference that is already gone. Row (c) is bounded before the close and
# must stay bounded after it, so it fails on an over-eager replica rather than on
# the leak.
#
# This file is the LEAK gauge — an `arena/region-count` delta over a fixed window,
# BOUNDED for every row. The soundness complement is
# region-shortcircuit-tail-arm-uaf.lisp.

(def window 400)

(defn measure [thunk warm window]
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

# callees ──────────────────────────────────────────────────────────────────────

# A closure the arm tail-calls: it replaces the frame, so nothing the lowerer
# emitted after the `TailCall` runs.
(defn sink []
  0)

# A closure that takes the caller's argument over as an owned parameter.
(defn sink-arg [a]
  (length a))

# A closure that hands its argument back, so the caller reads the region after
# the arm's call returns.
(defn sink-back [a]
  a)

# subjects ─────────────────────────────────────────────────────────────────────

# (a) `or` whose last operand replaces the frame. `x` is named nowhere in the
# body, so its release is the unused-parameter fallback the lowerer emits past
# the merge — the block this arm never reaches.
(defn a-or [x t]
  (or t (sink)))

# (b) the same through `and`, whose branch is `or`'s mirror: the first operand
# short-circuits on FALSE and falls through to the second on true.
(defn b-and [x t]
  (and t (sink)))

# (d) the arm's callee reaches `x` through its CAPTURED environment, which no
# argument names. The funnel counted that hold when the env was built, so the
# frame's release is still the only reference the replica drops.
(defn d-or-captured [x t]
  (let [g (fn [] (length x))]
    (or t (g))))

# exemption ────────────────────────────────────────────────────────────────────

# (c) `x` IS the arm's call argument, so its never-executed release is the
# ownership move the callee's owned-param release consumes. The arm takes no
# replica of it, and the row is bounded both before the close and after.
(defn c-or-moved [x t]
  (or t (sink-arg x)))

# controls ─────────────────────────────────────────────────────────────────────

# (e) the same shape with an INTRINSIC in the last operand. No frame is replaced,
# so the merge is a position every path reaches and the release runs where the
# lowerer put it.
(defn e-or-intrinsic [x t]
  (or t (%int? t)))

# (f) the same subject as (a), entered with the first operand truthy. The arm
# never runs, so the merge carries the release on its own — proof the rows above
# measure the arm rather than the operator.
(defn f-or-settled [x t]
  (or t (sink)))

# drivers ──────────────────────────────────────────────────────────────────────

(defn s-a []
  (a-or (string "shortcircuit-a") false))
(defn s-b []
  (b-and (string "shortcircuit-b") true))
(defn s-c []
  (c-or-moved (string "shortcircuit-c") false))
(defn s-d []
  (d-or-captured (string "shortcircuit-d") false))
(defn s-e []
  (e-or-intrinsic (string "shortcircuit-e") false))
(defn s-f []
  (f-or-settled (string "shortcircuit-f") true))

# measurement ──────────────────────────────────────────────────────────────────

(def d-a (measure s-a 20 window))
(def d-b (measure s-b 20 window))
(def d-c (measure s-c 20 window))
(def d-d (measure s-d 20 window))
(def d-e (measure s-e 20 window))
(def d-f (measure s-f 20 window))

(println "region-shortcircuit-tail-arm over " window " iters (region deltas):")
(println "  a or-closure-tail  " d-a)
(println "  b and-closure-tail " d-b)
(println "  c or-moved-arg     " d-c " (exemption)")
(println "  d or-captured      " d-d)
(println "  e or-intrinsic     " d-e " (control)")
(println "  f or-settled       " d-f " (control)")

(assert (%lt d-e 40)
        (concat "control: an intrinsic in the last operand replaces no frame, "
                "delta=" (number->string d-e)))
(assert (%lt d-f 40)
        (concat "control: a settled first operand never enters the arm, delta="
                (number->string d-f)))
(assert (%lt d-c 40)
        (concat "the moved argument's region was not reclaimed by the callee, "
                "delta=" (number->string d-c)))

(assert (%lt d-a 40)
        (concat "an `or` arm that replaces the frame strands the merge's "
                "release, delta=" (number->string d-a)))
(assert (%lt d-b 40)
        (concat "an `and` arm that replaces the frame strands the merge's "
                "release, delta=" (number->string d-b)))
(assert (%lt d-d 40)
        (concat "a holder the arm's callee captures strands the merge's "
                "release, delta=" (number->string d-d)))

(println "region-shortcircuit-tail-arm: ok")
