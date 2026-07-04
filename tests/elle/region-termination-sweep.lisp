(elle/epoch 12)
## region/termination-sweep — the region-level dual of region-eval-leak.
##
## docs/impl/region-model.md § "Constants lower as ordinary allocations" + the
## termination sweep invariant: after any eval completes and its result is
## dropped, every region it created is freed, so the live region count returns to
## its pre-run baseline. And a heap literal an eval RETURNS lives in a RECLAIMABLE
## region (id >= 2), reachable and freeable by RC.
##
## Two witnesses:
##  (a) attribution: the region of an eval'd string literal is a real, reclaimable
##      region (id >= 2), not an immediate (id 0).
##  (b) region sweep (guard): evaluating constant-bearing forms and dropping the
##      results leaves the live region count flat — every per-literal region is
##      reclaimed each eval. It catches a fix that allocates a literal's region but
##      forgets to free it (a region leak, as opposed to the object leak the
##      sibling test pins).

# ── (a) attribution witness ───────────────────────────────────────
# An eval'd string literal is a real reclaimable heap region.
(def reg (arena/region-of (eval "termination-sweep-xyz")))
(assert (%ge reg 2)
        "eval'd string literal must be an ordinary allocation in a real reclaimable region")

# ── (b) region-sweep guard ────────────────────────────────────────
# arena/region-count includes a constant offset (stable harness regions);
# measuring a DELTA across the loop cancels that offset, so a flat delta means
# every per-eval region was reclaimed.
(defn eval-str (n)
  (var i 0)
  (while (%lt i n)
    (eval "hello")
    (assign i (%add i 1))))

(eval-str 200)
# warm one-time effects
(def rc-before (arena/region-count))
(eval-str 2000)
(def rc-delta (%sub (arena/region-count) rc-before))

(println "region-termination-sweep: live region delta over 2000 evals = "
         rc-delta)
(assert (%lt rc-delta 50)
        (concat "eval leaks regions, delta=" (number->string rc-delta)))

(println "region-termination-sweep: ok")
