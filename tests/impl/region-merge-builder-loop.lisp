(elle/epoch 15)
# audited: 2026-11-14
# A nested `%pair` literal built and discarded in a loop merges its two regions and frees both each iteration.
# docs/impl/region/merging.md
#
# A fresh nested `%pair` literal — `(%pair (%pair i i) i)` — built and DISCARDED
# each iteration is the canonical builder idiom: the inner pair is stored as the
# car of the outer, both die together at the same decref_point, so the inner's
# region merges into the outer's — they allocate against one slot, freed by one
# `DecrefRegion`. The hazard is the per-execution slot model — two
# alloc instructions sharing one merged slot would each mint a fresh physical
# region and overwrite the activation mapping, ORPHANING the child (the shared-slot
# leak). `runtime_region_for_alloc_slot`'s mint-or-reuse (child mints, parent
# reuses) resolves it, and the single `DecrefRegion` clears the slot each iteration
# so per-iteration physical uniqueness holds. `%pair` lowers as the inline
# intrinsic (the emit_alloc/MERGE-seed op), so the merge fires on every compile.
#
# What this pin guarantees:
#  - CORRECTNESS (guardfree): the nested literal reads back its own values. A
#    mis-merge (a slot resolving to a wrong/dead physical region) frees a live
#    region; the junk allocation between build and read reuses it, so the
#    wrong-value asserts catch the stale read deterministically and
#    `--trace=guardfree` detonates on the freed page.
#  - BOUNDED region count: a discarded builder idiom frees its regions each
#    iteration, so the live-region count does not grow with the iteration
#    count. Were the shared-slot leak to escape mint-or-reuse, the
#    orphaned child region would accumulate one per iteration and this delta would
#    blow up to ~window.

(def window 2000)

(include-file "gauge/window.lisp")

# ── Correctness + survival ────────────────────────────────────────────────
# Build the nested literal, read both levels back, and allocate junk before the
# read so an early-freed (mis-merged) region would have been reused.
(var i 0)
(while (%lt i 300)
  (let [p (%pair (%pair i (%add i 1)) (%add i 2))
        _junk (%pair (%pair 0 0) 0)]
    # Read the nested car with the `first` wrapper: `%first`'s result type is
    # Top (pair element types are untracked), so nesting raw `%first` cannot
    # prove the inner operand is a pair. The reads only verify the values; the
    # `%pair` MERGE seed under test is the construction above and the region
    # measurement below, both still using the bare intrinsic.
    (assert (= (first (first p)) i)
            (string "builder inner car corrupted at i=" i))
    (assert (= (%rest p) (%add i 2))
            (string "builder outer cdr corrupted at i=" i)))
  (assign i (%add i 1)))

# ── Bounded region count: the shared-slot-leak guard ──
(def delta
  (measure-regions (fn ()
                     (begin
                       (%pair (%pair 1 2) 3)
                       nil)) 200 window))
(println "region-merge-builder-loop delta over " window " iters: " delta)
(assert (%lt delta 100)
        (string "builder-idiom loop leaks regions (shared-slot leak?), delta="
                (number->string delta)))

(println "region-merge-builder-loop: ok")
