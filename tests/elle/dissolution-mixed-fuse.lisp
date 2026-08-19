(elle/epoch 12)
# Mixed map/filter loop fusion — value preservation + realization
# (docs/impl/dissolution.md § "Mixed chains — one loop").
#
# A chain need not be homogeneous. `(map f (filter p xs))` and `(filter q (map g
# xs))` — any mix of `map` and `filter` over the same proven immutable array with
# inline non-capturing lambdas (a composition declines a capture) — fuse to a
# SINGLE index-walk loop through one
# unified transform/guard pipeline: a `map` stage transforms the threaded element,
# a `filter` stage guards it, and the intermediate array between the two ops never
# exists. This file is the behavioral gauge: the fused form must compute EXACTLY
# what the un-fused staged `map`/`filter` computes. The codegen gauge (both HOF
# dispatches gone, both body ops inline, one accumulator, one guard `if`) lives in
# `src/hir/typeinfer/fuse.rs`.
#
# The cross-check reference applies the same ops through named functions with a
# `match`-body: a top-level named fn with a PURE body now inlines, and a `let` body
# inlines too (docs § "Named same-unit functions"), so to keep a genuinely UN-fused
# oracle — one that still mints the intermediate array the realization gauge below
# weighs — these wrap the body in a `match`, a binding-introducing form the
# inline-clone whitelist declines, so they stay plain staged `map`/`filter` calls.
# Same value. Fused inline-lambda and the un-fused match-body form must agree.

(defn t10 [x]
  (match x
    _ (* x 10)))
(defn t3 [x]
  (match x
    _ (* x 3)))
(defn evp [x]
  (match x
    _ (even? x)))

# ── map-of-filter: filter first, then transform the survivors ──────────
(assert (= (map (fn [x] (* x 10)) (filter (fn [y] (even? y)) [1 2 3 4 5 6]))
           [20 40 60]) "mixed map-of-filter fuses to the same value")
(assert (= (map (fn [x] (* x 10)) (filter (fn [y] (even? y)) [1 2 3 4 5 6]))
           (map t10 (filter evp [1 2 3 4 5 6])))
        "fused map-of-filter agrees with the un-fused staged ops")

# ── filter-of-map: transform first, then guard the mapped values ───────
(assert (= (filter (fn [y] (even? y)) (map (fn [x] (* x 3)) [1 2 3 4])) [6 12])
        "mixed filter-of-map fuses to the same value")
(assert (= (filter (fn [y] (even? y)) (map (fn [x] (* x 3)) [1 2 3 4]))
           (filter evp (map t3 [1 2 3 4])))
        "fused filter-of-map agrees with the un-fused staged ops")

# The guard tests the TRANSFORMED value, not the original: `(* x 3)` of an odd `x`
# can be even or odd, so the survivor set depends on the map running first.
(assert (= (filter (fn [y] (even? y)) (map (fn [x] (+ x 1)) [1 2 3 4 5]))
           [2 4 6]) "filter-of-map guards the mapped value, not the input")

# ── boundary and extreme cases ─────────────────────────────────────────
(assert (= (map (fn [x] (* x 10)) (filter (fn [y] (even? y)) [])) [])
        "empty base fuses to empty")
(assert (= (map (fn [x] (* x 10)) (filter (fn [y] (even? y)) [1 3 5])) [])
        "no survivor fuses to empty")
(assert (= (map (fn [x] (* x 10)) (filter (fn [y] (even? y)) [2 4 6]))
           [20 40 60]) "every element survives")

# ── a three-stage mixed tower: both intermediates dissolve ─────────────
(assert (= (map (fn [z] (+ z 1))
                (filter (fn [y] (even? y)) (map (fn [x] (* x 3)) [1 2 3 4])))
           [7 13]) "three-stage map-filter-map tower fuses to the same value")

# ── the fused result is a normal immutable array ───────────────────────
(assert (= (length (map (fn [x] (* x 10)) (filter (fn [y] (even? y)) [1 2 3 4])))
           2) "fused mixed result has the right length")
(assert (= (mutable? (map (fn [x] (* x 10))
                          (filter (fn [y] (even? y)) [1 2 3 4]))) false)
        "fused mixed result is frozen")

# ── the reorder gate governs a mixed chain exactly as a homogeneous one ─
# A mixed chain is always length ≥ 2, so it always carries the reorder
# requirement. A variadic comparison like `>` routes through `apply` and is NOT
# reorder-safe, so it declines the whole composition; the chain then falls back to
# fusing only its inner reorder-safe run (the `filter` alone), leaving the outer
# `map` a plain call. The VALUE is unchanged either way — the fallback is a
# realization difference, not a semantic one.
(assert (= (map (fn [x] (* x 10)) (filter (fn [x] (> x 2)) [1 2 3 4])) [30 40])
        "non-reorder-safe mixed chain (inner-only fused) still computes correctly")
(assert (= (filter (fn [x] (> x 20)) (map (fn [x] (* x 10)) [1 2 3])) [30])
        "non-reorder-safe mixed chain, other direction, still correct")

# ── Realization: the intermediate array between the two ops is gone ────
# `arena/total-allocs` is a cumulative, monotonic count of objects ever minted
# (docs/impl/dissolution.md § "The gauge"). The un-fused reference (the match-body
# named fns, declined by the inline-clone whitelist) mints the intermediate array
# that the fused single loop never allocates; both compute the same value.
(defn allocs [thunk]
  (let [before (arena/total-allocs)]
    (thunk)
    (- (arena/total-allocs) before)))

(def base [0 1 2 3 4 5 6 7 8 9])

(def mf-fused
  (allocs (fn []
            (map (fn [x] (* x 10))
                 (filter (fn [y] (even? y)) [0 1 2 3 4 5 6 7 8 9])))))
(def mf-unfused (allocs (fn [] (map t10 (filter evp [0 1 2 3 4 5 6 7 8 9])))))
(assert (= (map (fn [x] (* x 10)) (filter (fn [y] (even? y)) base))
           (map t10 (filter evp base)))
        "fused and un-fused map-of-filter compute the same value")
(assert (< mf-fused mf-unfused)
        (string "fused map-of-filter must mint fewer (no intermediate array): "
                mf-fused " vs " mf-unfused))

(def fm-fused
  (allocs (fn []
            (filter (fn [y] (even? y))
                    (map (fn [x] (* x 3)) [0 1 2 3 4 5 6 7 8 9])))))
(def fm-unfused (allocs (fn [] (filter evp (map t3 [0 1 2 3 4 5 6 7 8 9])))))
(assert (< fm-fused fm-unfused)
        (string "fused filter-of-map must mint fewer (no intermediate array): "
                fm-fused " vs " fm-unfused))

# The saving is one intermediate array per fused layer: the three-stage tower
# (two intermediates) saves STRICTLY MORE than the two-stage chain (one). This is
# the intermediate-elimination signature — not a one-off constant a single removed
# alloc would also satisfy.
(def tower-fused
  (allocs (fn []
            (map (fn [z] (+ z 1))
                 (filter (fn [y] (even? y))
                         (map (fn [x] (* x 3)) [0 1 2 3 4 5 6 7 8 9]))))))
(defn add1 [z]
  (match z
    _ (+ z 1)))
(def tower-unfused
  (allocs (fn [] (map add1 (filter evp (map t3 [0 1 2 3 4 5 6 7 8 9]))))))
(assert (= (map (fn [z] (+ z 1))
                (filter (fn [y] (even? y)) (map (fn [x] (* x 3)) base)))
           (map add1 (filter evp (map t3 base))))
        "fused and un-fused three-stage tower compute the same value")
(assert (> (- tower-unfused tower-fused) (- mf-unfused mf-fused))
        (string "the saving scales with mixed-chain depth (one intermediate per "
                "layer): tower saved " (- tower-unfused tower-fused)
                ", 2-stage saved " (- mf-unfused mf-fused)))

(println "dissolution-mixed-fuse: ok")
