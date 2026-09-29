(elle/epoch 12)
# audited: 2026-09-29
# A fused `fold`/`reduce`, lone or over a map/filter prefix, computes exactly what
# the staged stdlib ops compute, and a prefixed one mints fewer objects.
#
# docs/impl/dissolution/terminals.md
#
# The codegen pins — the dispatch gone, body ops inline, a scalar accumulator,
# one loop — are in src/hir/typeinfer/fuse/tests/fold.rs.
#
# The un-fused oracles wrap their bodies in a `match`, a binding-introducing form
# a fragment cannot close over, so they stay plain staged `fold`/`map`/`filter`
# calls that still mint the intermediate array the realization check weighs. A
# pure body or a `let` body inlines, so `addf-let` is the fusing let-body
# counterpart, checked by value.

(defn addf [a x]
  (match a
    _ (+ a x)))
(defn subf [a x]
  (match a
    _ (- a x)))
(defn t2 [x]
  (match x
    _ (* x 2)))
(defn t3 [x]
  (match x
    _ (* x 3)))
(defn evp [x]
  (match x
    _ (even? x)))

# A fusing let-body combinator (a fragment closes over `let` bindings); value-checked
# against the un-fused `addf` below.
(defn addf-let [a x]
  (let [s (+ a x)]
    s))

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
# A fusing let-body combinator agrees with the un-fused match-body one.
(assert (= (fold addf-let 0 [1 2 3 4]) (fold addf 0 [1 2 3 4]))
        "fused let-body combinator agrees with the un-fused match-body form")

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
# `>` routes through `apply`, so it is not reorder-safe: the composition declines
# and only the inner `filter` fuses. The value is unchanged either way.
(assert (= (fold (fn [a x] (+ a x)) 0 (filter (fn [x] (> x 2)) [1 2 3 4])) 7)
        "non-reorder-safe fold composition (inner-only fused) still correct")
# A LONE fold has no reorder gate — it threads the accumulator in order — so a
# non-reorder-safe body still fuses and computes correctly.
(assert (= (fold (fn [a x] (if (> a x) a x)) 0 [3 1 4 1 5]) 5)
        "a lone fold with a non-reorder-safe body fuses and still computes right")

# ── Realization: the intermediate array between the prefix and the fold ─
# The un-fused reference mints the intermediate array the map prefix hands the
# fold; the fused single loop never allocates it. `arena/total-allocs` counts
# every object ever minted.
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

# A lone fold has nothing to save: it produces a scalar, and the stdlib fold walks
# with `core-fold-step`, a top-level self-recursive binding that mints no per-call
# closure. Weighing it would compare two loops that mint the same objects, so the
# lone fold is checked by value only.
(assert (= (fold (fn [a x] (+ a x)) 0 base) 45) "the lone fold computes the sum")

(println "dissolution-fold-fuse: ok (fm saved " (- fm-unfused fm-fused)
         ", tower saved " (- tower-unfused tower-fused) ")")
