(elle/epoch 12)
# audited: 2026-09-29
# A top-level binding reassigned in a branch keeps its prior value on every path that does not overwrite it.
# docs/impl/region/bindings.md
#
# The sidecar arms guardfree, so a read of a freed prior faults.
#
# A reassigned binding is a 1-slot container, and drop-on-overwrite releases
# the displaced value at the assign that runs. So the release follows the path
# taken:
#   - MUST-kill: when EVERY arm reassigns the binding, the prior value is dead
#     at the join and the read sees the taken arm's value.
#   - MAY-survive: when only SOME arm reassigns it, the prior value is still
#     live on the not-taken path.
#
# The counter-factual: a release of the prior value keyed to the `assign`
# node rather than to its execution runs on the not-taken path too, and frees
# the surviving prior before its read.
#
# Branches are kept non-foldable by driving the condition from a mutable
# IMMEDIATE flag (`@flag`) — an immediate reassign is regionless and correct,
# so it is a safe way to make a runtime condition the optimizer can't fold,
# preserving both arms through region inference. Latent UAFs are manifested via
# alias+recycle (see region-mutable-reassign-flow.lisp for the rationale).

# ── 1. must-kill, TRUE path: both arms reassign; prior (0 0) is dead ────
(def @flag1 0)
(assign flag1 1)
(def @r1 (pair 0 0))
(if (> flag1 0) (assign r1 (pair 1 1)) (assign r1 (pair 2 2)))
(assert (= r1 (pair 1 1)) "both-arms reassign, true path takes arm value")
(println "branch-1 ok")

# ── 2. must-kill, FALSE path ────────────────────────────────────────────
(def @flag2 0)
(def @r2 (pair 0 0))
(if (> flag2 0) (assign r2 (pair 1 1)) (assign r2 (pair 2 2)))
(assert (= r2 (pair 2 2)) "both-arms reassign, false path takes arm value")
(println "branch-2 ok")

# ── 3. may-survive, branch TAKEN: one-arm assign fires, new value read ──
(def @flag3 0)
(assign flag3 1)
(def @r3 (pair 7 7))
(if (> flag3 0) (assign r3 (pair 1 1)))
(assert (= r3 (pair 1 1)) "one-arm reassign, taken path uses new value")
(println "branch-3 ok")

# ── 4. may-survive, branch NOT taken: prior MUST stay live — the kill-
#       soundness corner. Aliased + recycled so an early free shows up ──
(def @flag4 0)
(def @r4 (pair 7 7))
(if (> flag4 0) (assign r4 (pair 1 1)))
(def keep4 (list r4 r4 r4))
(def junk4
  (list (pair 8 8) (pair 9 9) (pair 8 8) (pair 9 9) (pair 8 8) (pair 9 9)))
(assert (= (first keep4) (pair 7 7))
        "one-arm reassign NOT taken: the not-overwritten prior must survive")
(println "branch-4 ok")

(println "region-mutable-reassign-branch: OK")
