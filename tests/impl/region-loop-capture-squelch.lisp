(elle/epoch 12)
# ── Region: an outer binding captured by a lambda INSIDE a loop must ──
# ──         outlive the loop (not be freed per iteration) ─────────────
#
# `safe` is a top-level (outer) binding. The loop body builds a fresh
# closure `(fn () (safe))` each iteration (here via `protect`, which wraps
# its body in `(fiber/new (fn () …) 1)`), capturing `safe`. The capture is a
# use of `safe` recorded at the *lambda's* HirId, which sits inside the loop.
#
# The liveness iter-scope extension (src/hir/liveness.rs) hoists a binding's
# last-use out of a loop when it is bound outside and REFERENCED inside —
# but it only fired for `Var` nodes, not for lambda CAPTURES. So `safe`'s
# region demise (`decref-value-region`) landed inside the loop body and ran
# every iteration, freeing the global binding's region after ~2 iterations →
# the next iteration's `(safe)` tail-calls a recycled, torn closure
# (`as_closure` tag mismatch) — a use-after-free.
#
# `safe` is a SQUELCH closure on purpose: its env is shared from `outer`
# (region-backed, see region-squelch-nested.lisp), which is the rc shape that
# makes the per-iteration over-decref reach zero fastest — but the defect is
# general to any outer binding captured by an in-loop lambda.
#
# Counter-factual: pre-fix this aborts/segfaults by iteration 2 (a capture
# cell freed by `DecrefValueRegion`, then read on the next iteration). The
# distinguishing control: a loop-LOCAL closure (bound fresh inside the body)
# is correctly freed per iteration and never tripped this.

(def inner (fn () (yield 1)))
(def outer (fn () (inner)))
(def safe (squelch outer :yield))

# Loop the protect-over-squelch: each iteration captures the outer `safe`.
# Pre-fix `safe`'s region is freed mid-loop and a later `(safe)` reads garbage.
(def @n 0)
(while (< n 20)
  (let [[ok? err] (protect (safe))]
    (assert (not ok?) "looped squelch: protect catches the converted yield")
    (assert (= (get err :error) :signal-violation)
            "looped squelch: error readable — safe's region survived the loop"))
  (assign n (+ n 1)))

# Same shape, but the squelched body ERRORS rather than yields (the defect
# does not need a yield — it is the capture-in-loop over-decref).
(def boom (fn () (+ "x" 1)))
(def safe2 (squelch (fn () (boom)) :yield))
(def @m 0)
(while (< m 20)
  (let [[ok? err] (protect (safe2))]
    (assert (not ok?) "looped squelch-error: failure"))
  (assign m (+ m 1)))

(println "region-loop-capture-squelch: ok")
