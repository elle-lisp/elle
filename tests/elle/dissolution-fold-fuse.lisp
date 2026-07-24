(elle/epoch 12)
# Fold/reduce loop fusion — value preservation + realization
# (docs/impl/dissolution.md § "Fold — the scalar terminal").
#
# `(fold f init xs)` — f called `(f acc element)`, the left-fold core.lisp runs —
# dissolves to an index-walk loop with a SCALAR accumulator: seeded by `init`,
# updated one left-fold step per element, result is the accumulator's final value
# (no @array, no freeze). The real prize is COMPOSITION with a map/filter prefix:
# `(fold f init (map g xs))` / `(fold f init (filter p xs))` fuse to ONE loop with
# NO intermediate array — map-reduce. This file is the behavioral gauge: the fused
# form must compute EXACTLY what the un-fused staged ops compute. The codegen gauge
# (the HOF dispatch gone, body ops inline, scalar accumulator, one loop) lives in
# `src/hir/typeinfer/fuse.rs`.
#
# The cross-check reference applies the same ops through NAMED functions (Vars, not
# lambda literals), which the gate leaves as plain staged `fold`/`map`/`filter`
# calls. Fused inline-lambda and un-fused named-fn must agree.

(defn addf [a x]
  (+ a x))
(defn subf [a x]
  (- a x))
(defn t2 [x]
  (* x 2))
(defn t3 [x]
  (* x 3))
(defn evp [x]
  (even? x))

# ── single fold: the scalar accumulator ────────────────────────────────
(assert (= (fold (fn [a x] (+ a x)) 0 [1 2 3 4]) 10)
        "single fold sums to the same value")
(assert (= (fold (fn [a x] (+ a x)) 100 [1 2 3 4]) 110)
        "the seed `init` is threaded in")
(assert (= (fold (fn [a x] (+ a x)) 0 []) 0)
        "an empty collection folds to `init` unchanged")

# `reduce` is `(def reduce fold)` — the same op, recognized by its own name.
(assert (= (reduce (fn [a x] (+ a x)) 0 [1 2 3 4]) 10)
        "reduce dissolves like fold")

# Order sensitivity: the fold is LEFT-associative. A non-commutative combinator
# (subtraction) proves the fused loop threads the accumulator in element order —
# ((((100-1)-2)-3)-4) = 90, not any reordering.
(assert (= (fold (fn [a x] (- a x)) 100 [1 2 3 4]) 90)
        "left-fold order is preserved (non-commutative combinator)")
(assert (= (fold (fn [a x] (- a x)) 100 [1 2 3 4]) (fold subf 100 [1 2 3 4]))
        "fused subtraction fold agrees with the un-fused named-fn form")

# ── fold-of-map: map-reduce, one loop, no intermediate array ────────────
(assert (= (fold (fn [a x] (+ a x)) 0 (map (fn [x] (* x 2)) [1 2 3 4])) 20)
        "fold-of-map fuses to the same value")
(assert (= (fold (fn [a x] (+ a x)) 0 (map (fn [x] (* x 2)) [1 2 3 4]))
           (fold addf 0 (map t2 [1 2 3 4])))
        "fused fold-of-map agrees with the un-fused staged ops")
# Order preserved through the map stage too: ((((100-2)-4)-6)-8) = 80.
(assert (= (fold (fn [a x] (- a x)) 100 (map (fn [x] (* x 2)) [1 2 3 4])) 80)
        "left-fold order is preserved across a fused map prefix")

# ── fold-of-filter: only survivors reach the fold step ─────────────────
(assert (= (fold (fn [a x] (+ a x)) 0 (filter (fn [y] (even? y)) [1 2 3 4 5 6]))
           12) "fold-of-filter sums only the survivors")
(assert (= (fold (fn [a x] (+ a x)) 0 (filter (fn [y] (even? y)) [1 2 3 4 5 6]))
           (fold addf 0 (filter evp [1 2 3 4 5 6])))
        "fused fold-of-filter agrees with the un-fused staged ops")

# ── a fold over a map/filter tower: both intermediates dissolve ─────────
(assert (= (fold (fn [a x] (+ a x)) 0
                 (filter (fn [y] (even? y)) (map (fn [x] (* x 3)) [1 2 3 4])))
           18) "fold over a filter-of-map tower fuses to the same value")

# ── boundary and extreme cases ─────────────────────────────────────────
(assert (= (fold (fn [a x] (+ a x)) 7 (map (fn [x] (* x 2)) [])) 7)
        "empty base folds to init (fused prefix produces nothing)")
(assert (= (fold (fn [a x] (+ a x)) 0 (filter (fn [y] (even? y)) [1 3 5])) 0)
        "no survivor folds to init")

# ── the reorder gate governs a fold composition exactly as a mixed one ──
# A fold WITH an inner map/filter is length ≥ 2, so it carries the reorder
# requirement. A variadic comparison like `>` routes through `apply` and is NOT
# reorder-safe, so it declines the whole composition; the chain falls back to
# fusing only its inner reorder-safe run (the `filter`), leaving the outer `fold`
# a plain call. The VALUE is unchanged either way.
(assert (= (fold (fn [a x] (+ a x)) 0 (filter (fn [x] (> x 2)) [1 2 3 4])) 7)
        "non-reorder-safe fold composition (inner-only fused) still correct")
# A LONE fold has no reorder gate — it threads the accumulator in order — so a
# non-reorder-safe body still fuses and computes correctly.
(assert (= (fold (fn [a x] (if (> a x) a x)) 0 [3 1 4 1 5]) 5)
        "a lone fold with a non-reorder-safe body fuses and still computes right")

# ── Realization: the intermediate array between the prefix and the fold ─
# `arena/total-allocs` is a cumulative, monotonic count of objects ever minted
# (docs/impl/dissolution.md § "The gauge"). The un-fused reference (named fns —
# declined by the gate) mints the intermediate array that the map prefix would
# hand the fold; the fused single loop never allocates it.
(defn allocs [thunk]
  (let [before (arena/total-allocs)]
    (thunk)
    (- (arena/total-allocs) before)))

(def base [0 1 2 3 4 5 6 7 8 9])

(def fm-fused
  (allocs (fn []
            (fold (fn [a x] (+ a x)) 0
                  (map (fn [x] (* x 2)) [0 1 2 3 4 5 6 7 8 9])))))
(def fm-unfused (allocs (fn [] (fold addf 0 (map t2 [0 1 2 3 4 5 6 7 8 9])))))
(assert (= (fold (fn [a x] (+ a x)) 0 (map (fn [x] (* x 2)) base))
           (fold addf 0 (map t2 base)))
        "fused and un-fused fold-of-map compute the same value")
(assert (< fm-fused fm-unfused)
        (string "fused fold-of-map must mint fewer (no intermediate array): "
                fm-fused " vs " fm-unfused))

# The saving scales with the prefix depth: a fold over a two-op prefix (two
# intermediates the un-fused form materializes) saves STRICTLY MORE than over a
# one-op prefix — the intermediate-elimination signature, not a one-off constant.
(def tower-fused
  (allocs (fn []
            (fold (fn [a x] (+ a x)) 0
                  (filter (fn [y] (even? y))
                          (map (fn [x] (* x 3)) [0 1 2 3 4 5 6 7 8 9]))))))
(def tower-unfused
  (allocs (fn [] (fold addf 0 (filter evp (map t3 [0 1 2 3 4 5 6 7 8 9]))))))
(assert (= (fold (fn [a x] (+ a x)) 0
                 (filter (fn [y] (even? y)) (map (fn [x] (* x 3)) base)))
           (fold addf 0 (filter evp (map t3 base))))
        "fused and un-fused fold-over-tower compute the same value")
(assert (> (- tower-unfused tower-fused) (- fm-unfused fm-fused))
        (string "the saving scales with prefix depth (one intermediate per layer): "
                "tower saved " (- tower-unfused tower-fused) ", 1-stage saved "
                (- fm-unfused fm-fused)))

# A lone fold produces a scalar — no result array to save. Its allocation profile
# is NOT necessarily below the stdlib fold's: `core-fold-step` is a hand-tuned raw-
# intrinsic (`%add`/`%lt`) index walk, while the fused loop uses the same generic
# `+`/`<` scaffold map/filter fusion emits. The realization win of fold fusion is
# specifically the intermediate array a map/filter prefix would materialize (gauged
# above), not beating a raw scalar loop — so the lone fold is gauged by value only.
(assert (= (fold (fn [a x] (+ a x)) 0 base) 45) "the lone fold computes the sum")

(println "dissolution-fold-fuse: ok (fm saved " (- fm-unfused fm-fused)
         ", tower saved " (- tower-unfused tower-fused) ")")
