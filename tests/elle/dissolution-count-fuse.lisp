(elle/epoch 12)
# Count loop fusion — value preservation + realization
# (docs/impl/dissolution.md § "Count — the terminal that is a guard plus a tally").
#
# `(count pred coll)` takes a `filter`'s two-argument shape and produces a NUMBER,
# so it is a terminal exactly as `fold` is. Its fused form is the pipeline already
# built: the predicate becomes the LAST guard stage, and the base case tallies a
# scalar accumulator instead of pushing (no @array, no freeze). Over a map/filter
# prefix the whole chain collapses to ONE loop with no intermediate array. This
# file is the behavioral gauge: the fused form must compute EXACTLY what the
# un-fused staged ops compute. The codegen gauge (the dispatch gone, the predicate
# inline, a scalar tally, one loop) lives in `src/hir/typeinfer/fuse.rs`.
#
# The cross-check reference applies the same ops through named functions with a
# `match` body — a binding-introducing form the inline-clone whitelist declines —
# so they stay plain staged `count`/`map`/`filter` calls and mint what the fused
# form does not. Same value.

(defn evp [x]
  (match x
    _ (even? x)))
(defn nump [x]
  (match x
    _ (number? x)))
(defn t3 [x]
  (match x
    _ (* x 3)))

# ── single count: the scalar tally ─────────────────────────────────────
(assert (= (count (fn [x] (even? x)) [1 2 3 4 5 6]) 3)
        "single count tallies the survivors")
(assert (= (count (fn [x] (even? x)) [1 3 5]) 0) "no survivor tallies to 0")
(assert (= (count (fn [x] (even? x)) []) 0) "an empty collection tallies to 0")
(assert (= (count (fn [x] true) [1 2 3 4]) 4)
        "an always-true predicate tallies every element")
(assert (= (count (fn [x] (even? x)) [1 2 3 4 5 6]) (count evp [1 2 3 4 5 6]))
        "fused count agrees with the un-fused named-fn form")

# Only `nil` and `false` are falsy, so a predicate returning 0 or "" still counts.
(assert (= (count (fn [x] 0) [1 2 3]) 3)
        "0 is truthy — the guard follows Elle truthiness, not the value's shape")
(assert (= (count (fn [x] nil) [1 2 3]) 0) "nil is falsy")

# A named same-unit predicate inlines by cloning, and a stdlib `defn` carried
# across the compile-unit boundary does too — the tally terminal puts no new
# requirement on how the function is resolved. `inc` is truthy for every number,
# so it counts them all.
(defn pos? [x]
  (> x 0))
(assert (= (count pos? [1 -2 3 -4 5]) 3) "a named same-unit predicate inlines")
(assert (= (count inc [1 2 3]) 3) "a cross-unit stdlib predicate inlines")

# ── count-of-map: one loop, no intermediate array ──────────────────────
(assert (= (count (fn [y] (even? y)) (map (fn [x] (* x 3)) [1 2 3 4])) 2)
        "count-of-map fuses to the same value")
(assert (= (count (fn [y] (even? y)) (map (fn [x] (* x 3)) [1 2 3 4]))
           (count evp (map t3 [1 2 3 4])))
        "fused count-of-map agrees with the un-fused staged ops")

# ── count-of-filter: two nested guards, no intermediate array ──────────
(assert (= (count (fn [y] (even? y)) (filter (fn [x] (number? x)) [1 "a" 2 3 4]))
           2) "count-of-filter counts only what survives both guards")
(assert (= (count (fn [y] (even? y)) (filter (fn [x] (number? x)) [1 "a" 2 3 4]))
           (count evp (filter nump [1 "a" 2 3 4])))
        "fused count-of-filter agrees with the un-fused staged ops")

# ── a count over a filter-of-map tower: both intermediates dissolve ────
(assert (= (count (fn [z] (number? z))
                  (filter (fn [y] (even? y)) (map (fn [x] (* x 3)) [1 2 3 4])))
           2) "count over a filter-of-map tower fuses to the same value")

# ── the reorder gate governs a count composition exactly as a fold's ───
# A count WITH an inner map/filter is length >= 2, so it carries the reorder
# requirement. A variadic comparison like `>` routes through `apply` and is NOT
# reorder-safe, so it declines the whole composition; the chain falls back to
# fusing only its inner reorder-safe run (the `filter`), leaving the outer `count`
# a plain call. The VALUE is unchanged either way.
(assert (= (count (fn [y] (even? y)) (filter (fn [w] (> w 1)) [1 2 3 4])) 2)
        "non-reorder-safe count composition (inner-only fused) still correct")
# A LONE count has no reorder gate, so a non-reorder-safe predicate still fuses.
(assert (= (count (fn [x] (> x 2)) [1 2 3 4 5]) 3)
        "a lone count with a non-reorder-safe predicate fuses and still counts")

# ── the mutable-array base declines ────────────────────────────────────
# `count`'s own array arm re-reads `(length coll)` every iteration where the fused
# loop captures `len` once, so a mutable base stays a plain call. The value is the
# stdlib op's, and the base is untouched by the count.
(def @mut @[1 2 3 4 5 6])
(assert (= (count (fn [x] (even? x)) mut) 3)
        "a mutable @array base counts through the un-fused stdlib op")
(assert (= (length mut) 6) "counting does not disturb the mutable base")

# ── the base survives the walk ─────────────────────────────────────────
(def base [1 2 3 4 5 6 7 8])
(assert (= (count (fn [x] (even? x)) base) 4) "Var-bound base counts")
(assert (= (get base 0) 1) "the base Var survives the fused count")

# ── Realization: the closures and the intermediate array ───────────────
# `arena/total-allocs` is a cumulative, monotonic count of objects ever minted
# (docs/impl/dissolution.md § "The gauge"). Unlike a lone fold, a lone count has a
# realization win of its own: `count`'s array arm walks with a `letrec`-bound
# self-recursive closure, so the un-fused form mints that closure and its forward
# cell per call on top of the predicate closure. The fused loop mints none.
(defn allocs [thunk]
  (let [before (arena/total-allocs)]
    (thunk)
    (- (arena/total-allocs) before)))

(def lone-fused
  (allocs (fn [] (count (fn [x] (even? x)) [0 1 2 3 4 5 6 7 8 9]))))
(def lone-unfused (allocs (fn [] (count evp [0 1 2 3 4 5 6 7 8 9]))))
(assert (= (count (fn [x] (even? x)) base) (count evp base))
        "fused and un-fused lone count compute the same value")
(assert (< lone-fused lone-unfused)
        (string "a fused lone count must mint fewer (no walker closure): "
                lone-fused " vs " lone-unfused))

# Over a map prefix the intermediate array goes too, so the saving is strictly
# larger than the lone case's — the intermediate-elimination signature.
(def cm-fused
  (allocs (fn []
            (count (fn [y] (even? y))
                   (map (fn [x] (* x 3)) [0 1 2 3 4 5 6 7 8 9])))))
(def cm-unfused (allocs (fn [] (count evp (map t3 [0 1 2 3 4 5 6 7 8 9])))))
(assert (= (count (fn [y] (even? y)) (map (fn [x] (* x 3)) base))
           (count evp (map t3 base)))
        "fused and un-fused count-of-map compute the same value")
(assert (> (- cm-unfused cm-fused) (- lone-unfused lone-fused))
        (string "the saving grows with the prefix (one intermediate array): "
                "count-of-map saved " (- cm-unfused cm-fused) ", lone saved "
                (- lone-unfused lone-fused)))

(println "dissolution-count-fuse: ok (lone saved " (- lone-unfused lone-fused)
         ", count-of-map saved " (- cm-unfused cm-fused) ")")
