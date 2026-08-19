(elle/epoch 12)
# Drop-while loop fusion — value preservation + realization
# (docs/impl/dissolution.md § "Drop-while — the stage that starts late").
#
# `(drop-while pred coll)` skips the leading run its predicate admits and passes on
# every element from the first one the predicate rejects. It wears a `filter`'s
# two-argument shape and produces a COLLECTION, so it is a pipeline stage rather
# than a terminal: ops chain over it. Its fused form is a guard with the sides
# swapped — a `dropping` flag the rejecting element clears, after which every
# element passes. The flag gates the PIPELINE, never the walk: a drop-while has no
# early exit, so the loop condition stays the bare range test. This file is the
# behavioral gauge; the codegen gauge (the dispatch gone, the predicate inline, the
# flag where it belongs) lives in `src/hir/typeinfer/fuse.rs`.
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

# ── the leading run is skipped ─────────────────────────────────────────
(assert (= (->list (drop-while (fn [x] (even? x)) [2 4 5 6 8])) (list 5 6 8))
        "drop-while skips the leading run and keeps everything after it")
(assert (= (->list (drop-while (fn [x] (even? x)) [1 2 4])) (list 1 2 4))
        "a first-element rejection keeps everything")
(assert (= (->list (drop-while (fn [x] (even? x)) [2 4 6])) ())
        "an undecided walk keeps nothing")
(assert (= (->list (drop-while (fn [x] (even? x)) [2 4 5 6 8]))
           (->list (drop-while evp [2 4 5 6 8])))
        "the fused drop-while agrees with the un-fused named-fn form")

# Only `nil` and `false` are falsy, so a predicate returning 0 or "" still drops.
(assert (= (length (drop-while (fn [x] 0) [1 2 3])) 0)
        "0 is truthy — the guard follows Elle truthiness")
(assert (= (length (drop-while (fn [x] nil) [1 2 3])) 3) "nil is falsy")

# A named same-unit predicate inlines by cloning, and a stdlib `defn` carried
# across the compile-unit boundary does too.
(defn small? [x]
  (even? x))
(assert (= (length (drop-while small? [2 4 5])) 1)
        "a named same-unit predicate inlines")
(assert (= (length (drop-while inc [1 2 3])) 0)
        "a cross-unit stdlib predicate inlines (every number is truthy)")

# ── the two facts drop-while's own array arm decides ───────────────────
# Its array arm returns the accumulator with no `(if (mutable? coll) …)` test, so
# the result is MUTABLE even over an immutable base — and its `(empty? coll)`
# clause precedes that arm, so an empty input answers with the empty LIST. Fusion
# reproduces both: a rewrite may not change a value.
(assert (mutable? (drop-while (fn [x] (even? x)) [2 4 5]))
        "the fused result is unfrozen, as the stdlib array arm's is")
(assert (= (type-of (drop-while (fn [x] (even? x)) [])) :list)
        "an empty base answers with `()`, as the stdlib empty? clause does")
(assert (= (type-of (drop-while evp []))
           (type-of (drop-while (fn [x] (even? x)) [])))
        "fused and un-fused agree on the empty base")
(assert (= (type-of (drop-while (fn [x] (even? x)) [2 4])) :@array)
        "a non-empty base answers with the accumulator even when nothing is kept")

# A chain holding a drop-while is unfrozen throughout, because `map` and `filter`
# are type-preserving over the mutable array it hands on.
(assert (mutable? (map (fn [y] (* y 2)) (drop-while (fn [x] (even? x)) [2 4 5])))
        "a map over a drop-while is unfrozen too")
(assert (= (type-of (map (fn [y] (* y 2)) (drop-while (fn [x] (even? x)) [])))
           :list) "an empty base still answers `()` through the whole chain")

# ── composition: one loop, no intermediate array ───────────────────────
(assert (= (->list (map (fn [y] (* y 2))
                        (drop-while (fn [x] (even? x)) [2 4 5 6]))) (list 10 12))
        "map-over-drop-while fuses to the same value")
(assert (= (->list (map (fn [y] (* y 2))
                        (drop-while (fn [x] (even? x)) [2 4 5 6])))
           (->list (map t2 (drop-while evp [2 4 5 6]))))
        "the fused composition agrees with the un-fused staged ops")
(assert (= (->list (drop-while (fn [y] (even? y)) (map (fn [x] (* x 2)) [1 2 5])))
           ()) "drop-while over a map prefix sees the TRANSFORMED values")
(assert (= (->list (drop-while (fn [y] (even? y)) (map (fn [x] (* x 3)) [1 2 5])))
           (->list (drop-while evp (map (fn [x] (* x 3)) [1 2 5]))))
        "the fused prefix composition agrees with the un-fused staged ops")

# A scalar terminal over a drop-while collapses to one loop with no array at all.
(assert (= (count (fn [y] (number? y)) (drop-while (fn [x] (even? x)) [2 4 5 6]))
           2) "count over a drop-while counts only what the run passed on")
(assert (= (count (fn [y] (number? y)) (drop-while (fn [x] (even? x)) [2 4 5 6]))
           (count nump (drop-while evp [2 4 5 6])))
        "the fused count-over-drop-while agrees with the un-fused form")
(assert (= (fold (fn (a x) (+ a x)) 0 (drop-while (fn [x] (even? x)) [2 4 5 6]))
           11) "fold over a drop-while folds only what the run passed on")
(assert (any? (fn [y] (odd? y)) (drop-while (fn [x] (even? x)) [2 4 5 6]))
        "a search decides over the elements the run passed on")

# A drop-while REMOVES a leading run, so the elements it passes on renumber — a
# find-index over one must answer the position in its output, not the base index.
(assert (= (find-index (fn [y] (odd? y))
                       (drop-while (fn [x] (even? x)) [2 4 5 7])) 0)
        "find-index over a drop-while answers the renumbered position")
(assert (= (find-index (fn [y] (odd? y))
                       (drop-while (fn [x] (even? x)) [2 4 5 7]))
           (find-index (fn [y] (odd? y)) (drop-while evp [2 4 5 7])))
        "the fused find-index agrees with the un-fused staged form")
(assert (= (find-index (fn [y] (even? y))
                       (drop-while (fn [x] (even? x))
                                   (map (fn [z] (+ z 1)) [1 3 4 7]))) 1)
        "a map prefix under a drop-while renumbers by the dropped run alone")

# ── where the predicate stops ──────────────────────────────────────────
# The predicate runs on the leading run plus the element that ends it, and on no
# later one — the flag is read before the test. `(/ 6 0)` on the third element is
# reached only if it is not.
(assert (= (length (drop-while (fn [x] (even? (/ 6 x))) [1 2 0])) 2)
        "the predicate stops at the first rejection — no later element reaches it")

# The walk itself never stops: every element past the rejection must still be
# pushed, and with a prefix every element's transform must still run.
(assert (= (->list (drop-while (fn [y] (even? y))
                               (map (fn [x] (* x 3)) [2 4 5 6]))) (list 15 18))
        "the walk reaches every element past the rejection")
(assert (= :every-element (try
                            (drop-while (fn [y] (nil? y))
                                        (map (fn [x]
                                          (if (zero? x)
                                            (error :every-element)
                                            (* x 2))) [3 0]))
                            (catch e e)))
        "a prefix runs on every element — its error still surfaces")

# ── the declines ───────────────────────────────────────────────────────
# A `filter` inner to a drop-while can hand an empty collection on from a NON-empty
# base, where the staged op answers `()` and a fused loop its accumulator, so the
# chain declines whole. The value is the stdlib's either way.
(assert (= (type-of (drop-while (fn [y] (even? y))
                                (filter (fn [x] (nil? x)) [1 2 3]))) :list)
        "a filter that empties a non-empty base still answers `()`")
(assert (= (->list (drop-while (fn [y] (even? y))
                               (filter (fn [x] (number? x)) [2 "a" 4 5])))
           (list 5)) "the declined chain still computes the stdlib value")

# The two untyped array arms empty a non-empty base the same way, so either inner
# to the other declines the outer op.
(assert (= (->list (drop-while (fn [y] (odd? y))
                               (take-while (fn [x] (number? x)) [1 3 4 5])))
           (list 4 5))
        "a take-while inner to a drop-while declines and still computes")
(assert (= (->list (take-while (fn [y] (odd? y))
                               (drop-while (fn [x] (even? x)) [2 4 5 7 8])))
           (list 5 7))
        "a drop-while inner to a take-while declines and still computes")

# `drop-while`'s array arm re-reads `(length coll)` every iteration where the fused
# loop captures `len` once, so a mutable base stays a plain call.
(def @mut @[2 4 5 6])
(assert (= (->list (drop-while (fn [x] (even? x)) mut)) (list 5 6))
        "a mutable @array base runs through the un-fused stdlib op")
(assert (= (length mut) 4) "the walk does not disturb the mutable base")

# A predicate reading an enclosing local fuses — the splice is the call site, so the
# name is in scope (docs/impl/dissolution.md § "Captures").
(assert (= (->list (let [limit 5]
                     (drop-while (fn [x] (even? (+ x limit))) [1 3 4])))
           (list 4)) "a capturing predicate fuses to the stdlib value")

# ── the base survives the walk ─────────────────────────────────────────
(def base [2 4 6 8 9 10])
(assert (= (->list (drop-while (fn [x] (even? x)) base)) (list 9 10))
        "a Var-bound base fuses")
(assert (= (get base 0) 2) "the base Var survives the fused walk")

# ── Realization: two walker closures, two cells, and the intermediate ──
# `arena/total-allocs` is a cumulative, monotonic count of objects ever minted
# (docs/impl/dissolution.md § "The gauge"). `drop-while`'s array arm walks with TWO
# `letrec`-bound self-recursive closures — one to find the start, one to copy from
# it — so the un-fused form mints two closures and two forward cells per call on
# top of the predicate closure; the fused loop mints none.
(defn allocs [thunk]
  (let [before (arena/total-allocs)]
    (thunk)
    (- (arena/total-allocs) before)))

(def mostly-even [0 2 4 6 8 10 12 14 16 17])
(def lone-fused (allocs (fn [] (drop-while (fn [x] (even? x)) mostly-even))))
(def lone-unfused (allocs (fn [] (drop-while evp mostly-even))))
(assert (= (->list (drop-while (fn [x] (even? x)) mostly-even))
           (->list (drop-while evp mostly-even)))
        "fused and un-fused lone drop-while compute the same value")
(assert (< lone-fused lone-unfused)
        (string "a fused lone drop-while must mint fewer (no walker closures): "
                lone-fused " vs " lone-unfused))

# Over a prefix the intermediate array goes too, so the saving is strictly larger
# than the lone case's — the intermediate-elimination signature.
(def dm-fused
  (allocs (fn []
            (map (fn [y] (* y 2)) (drop-while (fn [x] (even? x)) mostly-even)))))
(def dm-unfused (allocs (fn [] (map t2 (drop-while evp mostly-even)))))
(assert (= (->list (map (fn [y] (* y 2))
                        (drop-while (fn [x] (even? x)) mostly-even)))
           (->list (map t2 (drop-while evp mostly-even))))
        "fused and un-fused map-over-drop-while compute the same value")
(assert (> (- dm-unfused dm-fused) (- lone-unfused lone-fused))
        (string "the saving grows with the composition (one intermediate array): "
                "map-over-drop-while saved " (- dm-unfused dm-fused)
                ", lone saved " (- lone-unfused lone-fused)))

(println "dissolution-drop-while-fuse: ok (lone saved "
         (- lone-unfused lone-fused) ", map-over-drop-while saved "
         (- dm-unfused dm-fused) ")")
