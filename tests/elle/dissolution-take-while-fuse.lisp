(elle/epoch 12)
# Take-while loop fusion — value preservation + realization
# (docs/impl/dissolution.md § "Take-while — the stage that ends the walk").
#
# `(take-while pred coll)` keeps the leading run its predicate admits and stops at
# the first element it rejects. It wears a `filter`'s two-argument shape and
# produces a COLLECTION, so it is a pipeline stage rather than a terminal: ops
# chain over it. Its fused form is a guard whose rejecting side clears the
# early-exit sentinel a search taught the loop to carry — read by the loop
# condition where the `take-while` is the chain's innermost op, and by the stage
# itself where something inner to it must still run per element. This file is the
# behavioral gauge; the codegen gauge (the dispatch gone, the predicate inline, the
# sentinel where it belongs) lives in `src/hir/typeinfer/fuse.rs`.
#
# The cross-check reference applies the same ops through named functions with a
# `match` body — a binding-introducing form the inline-clone whitelist declines —
# so they stay plain staged calls and mint what the fused form does not. Same
# value.

(defn evp [x]
  (match x
    _ (even? x)))
(defn nump [x]
  (match x
    _ (number? x)))
(defn t2 [x]
  (match x
    _ (* x 2)))

# ── the leading run ────────────────────────────────────────────────────
(assert (= (->list (take-while (fn [x] (even? x)) [2 4 5 6 8])) (list 2 4))
        "take-while keeps the leading run and stops at the first rejection")
(assert (= (->list (take-while (fn [x] (even? x)) [1 2 4])) ())
        "a first-element rejection keeps nothing")
(assert (= (->list (take-while (fn [x] (even? x)) [2 4 6])) (list 2 4 6))
        "an undecided walk keeps everything")
(assert (= (->list (take-while (fn [x] (even? x)) [2 4 5 6 8]))
           (->list (take-while evp [2 4 5 6 8])))
        "the fused take-while agrees with the un-fused named-fn form")

# Only `nil` and `false` are falsy, so a predicate returning 0 or "" still keeps.
(assert (= (length (take-while (fn [x] 0) [1 2 3])) 3)
        "0 is truthy — the guard follows Elle truthiness")
(assert (= (length (take-while (fn [x] nil) [1 2 3])) 0) "nil is falsy")

# A named same-unit predicate inlines by cloning, and a stdlib `defn` carried
# across the compile-unit boundary does too.
(defn small? [x]
  (even? x))
(assert (= (length (take-while small? [2 4 5])) 2)
        "a named same-unit predicate inlines")
(assert (= (length (take-while inc [1 2 3])) 3)
        "a cross-unit stdlib predicate inlines (every number is truthy)")

# ── the two facts take-while's own array arm decides ───────────────────
# Its array arm returns the accumulator with no `(if (mutable? coll) …)` test, so
# the result is MUTABLE even over an immutable base — and its `(empty? coll)`
# clause precedes that arm, so an empty input answers with the empty LIST. Fusion
# reproduces both: a rewrite may not change a value.
(assert (mutable? (take-while (fn [x] (even? x)) [2 4 5]))
        "the fused result is unfrozen, as the stdlib array arm's is")
(assert (= (type-of (take-while (fn [x] (even? x)) [])) :list)
        "an empty base answers with `()`, as the stdlib empty? clause does")
(assert (= (type-of (take-while evp []))
           (type-of (take-while (fn [x] (even? x)) [])))
        "fused and un-fused agree on the empty base")
(assert (= (type-of (take-while (fn [x] (even? x)) [1 2])) :@array)
        "a non-empty base answers with the accumulator even when nothing is kept")

# A chain holding a take-while is unfrozen throughout, because `map` and `filter`
# are type-preserving over the mutable array it hands on.
(assert (mutable? (map (fn [y] (* y 2)) (take-while (fn [x] (even? x)) [2 4 5])))
        "a map over a take-while is unfrozen too")
(assert (= (type-of (map (fn [y] (* y 2)) (take-while (fn [x] (even? x)) [])))
           :list) "an empty base still answers `()` through the whole chain")

# ── composition: one loop, no intermediate array ───────────────────────
(assert (= (->list (map (fn [y] (* y 2))
                        (take-while (fn [x] (even? x)) [2 4 5 6]))) (list 4 8))
        "map-over-take-while fuses to the same value")
(assert (= (->list (map (fn [y] (* y 2))
                        (take-while (fn [x] (even? x)) [2 4 5 6])))
           (->list (map t2 (take-while evp [2 4 5 6]))))
        "the fused composition agrees with the un-fused staged ops")
(assert (= (->list (take-while (fn [y] (even? y)) (map (fn [x] (* x 2)) [1 2 5])))
           (list 2 4 10))
        "take-while over a map prefix sees the TRANSFORMED values")
(assert (= (->list (take-while (fn [y] (even? y)) (map (fn [x] (* x 2)) [1 2 5])))
           (->list (take-while evp (map t2 [1 2 5]))))
        "the fused prefix composition agrees with the un-fused staged ops")

# A scalar terminal over a take-while collapses to one loop with no array at all.
(assert (= (count (fn [y] (number? y)) (take-while (fn [x] (even? x)) [2 4 5 6]))
           2) "count over a take-while counts only the leading run")
(assert (= (count (fn [y] (number? y)) (take-while (fn [x] (even? x)) [2 4 5 6]))
           (count nump (take-while evp [2 4 5 6])))
        "the fused count-over-take-while agrees with the un-fused form")
(assert (= (fold (fn (a x) (+ a x)) 0 (take-while (fn [x] (even? x)) [2 4 5 6]))
           6) "fold over a take-while folds only the leading run")

# Two early exits in one chain: the take-while is innermost, so it takes the
# walk-ending sentinel and the search rides a gate stage. Neither may swallow the
# other's elements — the search answers over the leading run alone.
(assert (= (find (fn [y] (odd? y)) (take-while (fn [x] (even? x)) [2 4 5 7]))
           nil) "a search over a take-while sees only the leading run")
(assert (= (find (fn [y] (odd? y)) (take-while (fn [x] (even? x)) [2 4 5 7]))
           (find (fn [y] (odd? y)) (take-while evp [2 4 5 7])))
        "the fused search-over-take-while agrees with the un-fused form")
(assert (any? (fn [y] (number? y)) (take-while (fn [x] (even? x)) [2 4 5]))
        "a search decides over the leading run's elements")

# A take-while keeps a LEADING run, so it preserves every survivor's position —
# only a `filter` in the chain renumbers what a `find-index` answers with.
(assert (= (find-index (fn [y] (= y 6))
                       (take-while (fn [x] (even? x)) [2 4 6 7])) 2)
        "find-index over a take-while answers the position the run preserved")
(assert (= (find-index (fn [y] (= y 6))
                       (take-while (fn [x] (even? x))
                                   (map (fn [z] (* z 2)) [1 2 3 5]))) 2)
        "a map prefix under a take-while preserves the position too")

# ── where the early exit applies ───────────────────────────────────────
# A LONE take-while is the chain's innermost op, so the loop condition reads its
# sentinel and the walk ends at the decision. `(/ 6 0)` on the second element is
# reached only if it does not.
(assert (= (length (take-while (fn [x] (even? (/ 6 x))) [4 0])) 0)
        "the walk ends at the decision — no element past it is fetched")

# With a PREFIX the walk must stay exhaustive: the staged form runs the transform
# over the whole input, so the fused loop must too. The transform raises on an
# element past the decision, which is reached only if the walk did not stop.
(assert (= :past-the-decision (try
                                (take-while (fn [y] (nil? y))
                                (map (fn [x]
                                       (if (zero? x)
                                         (error :past-the-decision)
                                         (* x 2))) [3 0]))
                                (catch e e)))
        "a prefix runs past the decision — its error still surfaces")

# The other half of the split: the take-while's own predicate stops even though
# the walk does not. `(/ 6 0)` in the predicate is reached only if the sentinel
# gate fails to hold it off the second element, which the first already decided.
(assert (= (length (take-while (fn [y] (even? (/ 6 y)))
                               (map (fn [x] (* x 1)) [4 0]))) 0)
        "the sentinel gate keeps the predicate off elements past the decision")

# ── the declines ───────────────────────────────────────────────────────
# A `filter` inner to a take-while can hand an empty collection on from a NON-empty
# base, where the staged op answers `()` and a fused loop its accumulator, so the
# chain declines whole. The value is the stdlib's either way.
(assert (= (type-of (take-while (fn [y] (even? y))
                                (filter (fn [x] (nil? x)) [1 2 3]))) :list)
        "a filter that empties a non-empty base still answers `()`")
(assert (= (->list (take-while (fn [y] (even? y))
                               (filter (fn [x] (number? x)) [2 "a" 4 5])))
           (list 2 4)) "the declined chain still computes the stdlib value")

# `take-while`'s array arm re-reads `(length coll)` every iteration where the fused
# loop captures `len` once, so a mutable base stays a plain call.
(def @mut @[2 4 5 6])
(assert (= (->list (take-while (fn [x] (even? x)) mut)) (list 2 4))
        "a mutable @array base runs through the un-fused stdlib op")
(assert (= (length mut) 4) "the walk does not disturb the mutable base")

# A predicate reading an enclosing local fuses — the splice is the call site, so the
# name is in scope (docs/impl/dissolution.md § "Captures").
(assert (= (->list (let [limit 5]
                     (take-while (fn [x] (even? (+ x limit))) [1 3 4])))
           (list 1 3)) "a capturing predicate fuses to the stdlib value")

# ── the base survives the walk ─────────────────────────────────────────
(def base [2 4 6 8 9 10])
(assert (= (->list (take-while (fn [x] (even? x)) base)) (list 2 4 6 8))
        "a Var-bound base fuses")
(assert (= (get base 4) 9) "the base Var survives the fused walk")

# ── Realization: the walker closure, its cell, and the intermediate ────
# `arena/total-allocs` is a cumulative, monotonic count of objects ever minted
# (docs/impl/dissolution.md § "The gauge"). `take-while`'s array arm walks with a
# `letrec`-bound self-recursive closure, so the un-fused form mints that closure
# and its forward cell per call on top of the predicate closure; the fused loop
# mints none. The walk is UNDECIDED (every element admitted) so both forms visit
# every element — a walk that decides early would weigh the fused loop's one extra
# iteration against the objects fusion removes.
(defn allocs [thunk]
  (let [before (arena/total-allocs)]
    (thunk)
    (- (arena/total-allocs) before)))

(def all-even [0 2 4 6 8 10 12 14 16 18])
(def lone-fused (allocs (fn [] (take-while (fn [x] (even? x)) all-even))))
(def lone-unfused (allocs (fn [] (take-while evp all-even))))
(assert (= (->list (take-while (fn [x] (even? x)) all-even))
           (->list (take-while evp all-even)))
        "fused and un-fused lone take-while compute the same value")
(assert (< lone-fused lone-unfused)
        (string "a fused lone take-while must mint fewer (no walker closure): "
                lone-fused " vs " lone-unfused))

# Over a prefix the intermediate array goes too, so the saving is strictly larger
# than the lone case's — the intermediate-elimination signature.
(def tm-fused
  (allocs (fn [] (map (fn [y] (* y 2)) (take-while (fn [x] (even? x)) all-even)))))
(def tm-unfused (allocs (fn [] (map t2 (take-while evp all-even)))))
(assert (= (->list (map (fn [y] (* y 2))
                        (take-while (fn [x] (even? x)) all-even)))
           (->list (map t2 (take-while evp all-even))))
        "fused and un-fused map-over-take-while compute the same value")
(assert (> (- tm-unfused tm-fused) (- lone-unfused lone-fused))
        (string "the saving grows with the composition (one intermediate array): "
                "map-over-take-while saved " (- tm-unfused tm-fused)
                ", lone saved " (- lone-unfused lone-fused)))

(println "dissolution-take-while-fuse: ok (lone saved "
         (- lone-unfused lone-fused) ", map-over-take-while saved "
         (- tm-unfused tm-fused) ")")
