(elle/epoch 12)
# Per-execution region uniqueness — unoptimized Tofte-Talpin: every value gets
# its own region, period (merging is the only thing that may collapse regions,
# and merging is off). docs/impl/region/model.md "The per-execution region model".
#
# Counterfactual for the tail-loop region co-location bug: a `%pair` allocated as
# a tail-call argument has a DEAD `DecrefRegion` (the compiler emits it past the
# `TailCall`, where it is unreachable), so `take_runtime_region_for_drop_slot`
# never clears the alloc slot. A tail-recursive body runs inside one shared
# `activation_region_map` (TCO reuses the frame), so `runtime_region_for_alloc_slot`
# kept handing back the SAME cached physical region every iteration — piling every
# iteration's cons into one region (witnessed: 3 pairs in region 26). That is a
# Rule 6 commingling violation and the seed of the tail-call leaks.
#
# Each cons built in a distinct tail iteration must instead live in its own
# physical region. Pre-fix these assertions FAIL (region-of equal across
# iterations); after minting a fresh region per allocation execution they pass.

(defn build (n acc)
  (if (%le n 0) acc (build (%sub n 1) (%pair n acc))))

(def lst (build 6 ()))

# lst, (rest lst), (rest (rest lst)) are conses from three distinct tail
# iterations of `build`.
(def r0 (arena/region-of lst))
(def r1 (arena/region-of (rest lst)))
(def r2 (arena/region-of (rest (rest lst))))

# Sanity: all three are heap conses, so each has a real (non-zero) region.
(assert (not (= r0 0)) "lst must be a heap value with a region")
(assert (not (= r1 0)) "(rest lst) must be a heap value with a region")
(assert (not (= r2 0)) "(rest (rest lst)) must be a heap value with a region")

# The invariant: distinct values occupy distinct regions.
(assert (not (= r0 r1))
        (concat "co-location: lst and (rest lst) share region "
                (number->string r0)))
(assert (not (= r1 r2))
        (concat "co-location: (rest lst) and (rest (rest lst)) share region "
                (number->string r1)))
(assert (not (= r0 r2))
        "co-location: first and third tail-built conses share a region")

(println "region-tailloop-uniqueness: ok")
