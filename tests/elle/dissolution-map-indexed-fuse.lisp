(elle/epoch 12)
# Map-indexed loop fusion — value preservation + realization
# (docs/impl/dissolution.md § "Map-indexed — the stage that carries the position").
#
# `(map-indexed f coll)` transforms each element as a `map` does, but hands its
# function the element's POSITION beside it — `(f i elem)`, index first. It produces
# a collection, so it is a pipeline stage rather than a terminal: ops chain over it.
# Its fused form is a `map`'s with one extra binding, the loop's own induction
# variable. That variable IS the position because every stage inner to a map-indexed
# preserves the walk's length — the emptiness rule refuses each of the three that can
# shorten it — so no survivor count is owed. This file is the behavioral gauge; the
# codegen gauge (the dispatch gone, the body inline, no sentinel, one `+`) lives in
# `src/hir/typeinfer/fuse.rs`.
#
# The cross-check reference applies the same ops through named functions with a
# `match` body — a binding-introducing form the inline-clone whitelist declines — so
# they stay plain staged calls and mint what the fused form does not. Same value.

(defn mi [i x]
  (match x
    _ (* i x)))
(defn t1 [x]
  (match x
    _ (+ x 1)))

# ── the position is the element's index ────────────────────────────────
(assert (= (->list (map-indexed (fn [i x] (* i x)) [10 20 30])) (list 0 20 60))
        "map-indexed hands its function the element's position first")
(assert (= (->list (map-indexed (fn [i x] i) [:a :b :c])) (list 0 1 2))
        "the positions run 0..len-1 in walk order")
(assert (= (->list (map-indexed (fn [i x] (* i x)) [10 20 30]))
           (->list (map-indexed mi [10 20 30])))
        "the fused map-indexed agrees with the un-fused named-fn form")
(assert (= (->list (map-indexed (fn [i x] i) [])) ())
        "an empty base runs the function nowhere")

# A named same-unit function inlines by cloning, and the arity is the op's: a
# one-parameter function is a `map`'s, so the chain declines rather than splicing a
# body with an unbound parameter.
(defn scale [i x]
  (* i x))
(assert (= (->list (map-indexed scale [10 20 30])) (list 0 20 60))
        "a named same-unit function inlines")
(assert (= :arity-error (get (try
                               (map-indexed (fn [x] (* x 2)) [10 20])
                               (catch e e)) :error))
        "a one-parameter function declines to the stdlib op, which calls it with two")

# ── the two facts map-indexed's own array arm decides ──────────────────
# Its array arm returns the accumulator with no `(if (mutable? coll) …)` test, so the
# result is MUTABLE even over an immutable base — and its `(empty? coll)` clause
# precedes that arm, so an empty input answers with the empty LIST. Fusion reproduces
# both: a rewrite may not change a value.
(assert (mutable? (map-indexed (fn [i x] (* i x)) [10 20 30]))
        "the fused result is unfrozen, as the stdlib array arm's is")
(assert (= (type-of (map-indexed (fn [i x] x) [])) :list)
        "an empty base answers with `()`, as the stdlib empty? clause does")
(assert (= (type-of (map-indexed mi []))
           (type-of (map-indexed (fn [i x] (* i x)) [])))
        "fused and un-fused agree on the empty base")

# A chain holding a map-indexed is unfrozen throughout, because `map` and `filter`
# are type-preserving over the mutable array it hands on.
(assert (mutable? (map (fn [y] (+ y 1)) (map-indexed (fn [i x] (* i x)) [10 20])))
        "a map over a map-indexed is unfrozen too")
(assert (= (type-of (map (fn [y] (+ y 1)) (map-indexed (fn [i x] (* i x)) [])))
           :list) "an empty base still answers `()` through the whole chain")

# ── composition: one loop, no intermediate array ───────────────────────
(assert (= (->list (map (fn [y] (+ y 1))
                        (map-indexed (fn [i x] (* i x)) [10 20 30])))
           (list 1 21 61)) "map-over-map-indexed fuses to the same value")
(assert (= (->list (map (fn [y] (+ y 1))
                        (map-indexed (fn [i x] (* i x)) [10 20 30])))
           (->list (map t1 (map-indexed mi [10 20 30]))))
        "the fused composition agrees with the un-fused staged ops")
(assert (= (->list (map-indexed (fn [i y] (* i y))
                                (map (fn [x] (+ x 1)) [10 20 30])))
           (list 0 21 62))
        "map-indexed over a map prefix sees the TRANSFORMED values")
(assert (= (->list (map-indexed (fn [i y] (* i y))
                                (map (fn [x] (+ x 1)) [10 20 30])))
           (->list (map-indexed mi (map t1 [10 20 30]))))
        "the fused prefix composition agrees with the un-fused staged ops")
(assert (= (->list (map-indexed (fn [j y] (+ j y))
                                (map-indexed (fn [i x] (* i x)) [10 20 30])))
           (list 0 21 62))
        "a map-indexed inner to another preserves the numbering for both")

# A scalar terminal over a map-indexed collapses to one loop with no array at all.
(assert (= (fold (fn (a x) (+ a x)) 0
                 (map-indexed (fn [i x] (* i x)) [10 20 30])) 80)
        "fold over a map-indexed folds the transformed values")
(assert (= (count (fn [y] (odd? y)) (map-indexed (fn [i x] (+ i x)) [10 21 30]))
           0) "count over a map-indexed counts the transformed values")
(assert (= (find-index (fn [y] (> y 30))
                       (map-indexed (fn [i x] (* i x)) [10 20 30])) 2)
        "a map-indexed preserves the numbering a find-index answers with")

# A shortening op OUTER to a map-indexed is free: the positions were handed out
# before it dropped anything.
(assert (= (->list (filter (fn [y] (> y 20))
                           (map-indexed (fn [i x] (* i x)) [10 20 30])))
           (list 60)) "a filter outer to a map-indexed sees the base numbering")
(assert (= (->list (take-while (fn [y] (< y 50))
                               (map-indexed (fn [i x] (* i x)) [10 20 30])))
           (list 0 20)) "a take-while outer to a map-indexed fuses whole")
(assert (= (->list (drop-while (fn [y] (< y 30))
                               (map-indexed (fn [i x] (* i x)) [10 20 30])))
           (list 60)) "a drop-while outer to a map-indexed fuses whole")

# ── the declines ───────────────────────────────────────────────────────
# Every stage that could RENUMBER what reaches a map-indexed is one that SHORTENS the
# walk, and the emptiness rule already refuses each such stage inner to an untyped
# array arm. So the chain declines whole and the stdlib value stands — which is what
# leaves the fused position read off the base index.
(assert (= (->list (map-indexed (fn [i y] (* i y))
                                (filter (fn [x] (odd? x)) [1 2 3]))) (list 0 3))
        "a filter inner to a map-indexed declines and renumbers")
(assert (= (->list (map-indexed (fn [i y] (* i y))
                                (take-while (fn [x] (odd? x)) [1 3 4 5])))
           (list 0 3))
        "a take-while inner to a map-indexed declines and renumbers")
(assert (= (->list (map-indexed (fn [i y] (* i y))
                                (drop-while (fn [x] (odd? x)) [1 3 4 5])))
           (list 0 5))
        "a drop-while inner to a map-indexed declines and renumbers")

# `map-indexed`'s array arm re-reads `(length coll)` every iteration where the fused
# loop captures `len` once, so a mutable base stays a plain call.
(def @mut @[10 20 30])
(assert (= (->list (map-indexed (fn [i x] (* i x)) mut)) (list 0 20 60))
        "a mutable @array base runs through the un-fused stdlib op")
(assert (= (length mut) 3) "the walk does not disturb the mutable base")

# A capturing function is left alone; the value is unchanged.
(def bump 5)
(assert (= (->list (map-indexed (fn [i x] (+ i x bump)) [10 20])) (list 15 26))
        "a capturing function declines and still computes the stdlib value")

# ── the base survives the walk ─────────────────────────────────────────
(def base [7 8 9])
(assert (= (->list (map-indexed (fn [i x] (+ i x)) base)) (list 7 9 11))
        "a Var-bound base fuses")
(assert (= (get base 0) 7) "the base Var survives the fused walk")

# ── Realization: the walker closure, its cell, and the intermediate ────
# `arena/total-allocs` is a cumulative, monotonic count of objects ever minted
# (docs/impl/dissolution.md § "The gauge"). `map-indexed`'s array arm walks with a
# `letrec`-bound self-recursive closure, so the un-fused form mints that closure and
# its forward cell per call on top of the function closure; the fused loop mints
# none.
(defn allocs [thunk]
  (let [before (arena/total-allocs)]
    (thunk)
    (- (arena/total-allocs) before)))

(def nums [0 1 2 3 4 5 6 7 8 9])
(def lone-fused (allocs (fn [] (map-indexed (fn [i x] (* i x)) nums))))
(def lone-unfused (allocs (fn [] (map-indexed mi nums))))
(assert (= (->list (map-indexed (fn [i x] (* i x)) nums))
           (->list (map-indexed mi nums)))
        "fused and un-fused lone map-indexed compute the same value")
(assert (< lone-fused lone-unfused)
        (string "a fused lone map-indexed must mint fewer (no walker closure): "
                lone-fused " vs " lone-unfused))

# Over a composition the intermediate array goes too, so the saving is strictly
# larger than the lone case's — the intermediate-elimination signature.
(def mm-fused
  (allocs (fn [] (map (fn [y] (+ y 1)) (map-indexed (fn [i x] (* i x)) nums)))))
(def mm-unfused (allocs (fn [] (map t1 (map-indexed mi nums)))))
(assert (= (->list (map (fn [y] (+ y 1)) (map-indexed (fn [i x] (* i x)) nums)))
           (->list (map t1 (map-indexed mi nums))))
        "fused and un-fused map-over-map-indexed compute the same value")
(assert (> (- mm-unfused mm-fused) (- lone-unfused lone-fused))
        (string "the saving grows with the composition (one intermediate array): "
                "map-over-map-indexed saved " (- mm-unfused mm-fused)
                ", lone saved " (- lone-unfused lone-fused)))

(println "dissolution-map-indexed-fuse: ok (lone saved "
         (- lone-unfused lone-fused) ", map-over-map-indexed saved "
         (- mm-unfused mm-fused) ")")
