(elle/epoch 12)
# Mapcat loop fusion — value preservation + realization
# (docs/impl/dissolution.md § "Mapcat — the stage that fans out").
#
# `(mapcat f coll)` applies `f` to each element and splices the collection `f`
# returns into one flat result. It produces a collection, so it is a pipeline stage;
# what makes it unlike every other stage is that it threads a whole RUN of values on
# per base element instead of one, so its fused form puts a second `while` inside the
# element statement and splices the rest of the pipeline inside that inner walk.
#
# The inner walk is an indexed one, so fusion is gated on `f`'s body proving an ARRAY
# result: over a list `(get inner j)` would be O(j) and the fused walk quadratic,
# which is a bounded scratch saving paid for with an unbounded time cost. This file is
# the behavioral gauge; the codegen gauge (the dispatch gone, the body inline, two
# `while`s, one accumulator) lives in `src/hir/typeinfer/fuse.rs`.
#
# The cross-check reference applies the same ops through named functions with a
# `match` body — a binding-introducing form the inline-clone whitelist declines — so
# they stay plain staged calls and mint what the fused form does not. Same value.

(defn mc [x]
  (match x
    _ [x (* x 10)]))
(defn t1 [x]
  (match x
    _ (+ x 1)))
(defn p1 [x]
  (match x
    _ (odd? x)))
(defn tri [x]
  (match x
    _ [x x x]))
(defn p2 [x]
  (match x
    _ (even? x)))

# ── the fan-out ────────────────────────────────────────────────────────
(assert (= (->list (mapcat (fn [x] [x (* x 10)]) [1 2 3])) (list 1 10 2 20 3 30))
        "each base element contributes its whole returned run, in walk order")
(assert (= (->list (mapcat (fn [x] [x (* x 10)]) [1 2 3]))
           (->list (mapcat mc [1 2 3])))
        "the fused mapcat agrees with the un-fused named-fn form")
(assert (= (length (mapcat (fn [x] []) [1 2 3])) 0)
        "an empty per-element result contributes nothing")
(assert (= (->list (mapcat (fn [x] [x x x]) [1 2])) (list 1 1 1 2 2 2))
        "one base element may contribute many")

# ── the two facts mapcat's own array arm decides ───────────────────────
# Its array arm returns the `@array` it filled with no `(if (mutable? coll) …)` test,
# so the result is MUTABLE even over an immutable base — and its `(empty? coll)`
# clause precedes that arm, so an empty input answers with the empty LIST. A non-empty
# base whose every result is empty still answers with the (empty) accumulator.
(assert (mutable? (mapcat (fn [x] [x]) [1 2 3]))
        "the fused result is unfrozen, as the stdlib array arm's is")
(assert (= (type-of (mapcat (fn [x] [x]) [])) :list)
        "an empty base answers `()`, as the stdlib empty? clause does")
(assert (= (type-of (mapcat (fn [x] []) [1 2 3])) :@array)
        "a non-empty base answers with the accumulator, however empty it stayed")
(assert (= (type-of (mapcat mc [])) (type-of (mapcat (fn [x] [x (* x 10)]) [])))
        "fused and un-fused agree on the empty base")

# A chain holding a mapcat is unfrozen throughout, because `map` and `filter` are
# type-preserving over the mutable array it hands on.
(assert (mutable? (map (fn [y] (+ y 1)) (mapcat (fn [x] [x (* x 10)]) [1 2])))
        "a map over a mapcat is unfrozen too")
(assert (= (type-of (map (fn [y] (+ y 1)) (mapcat (fn [x] [x (* x 10)]) [])))
           :list) "an empty base still answers `()` through the whole chain")

# ── composition: one loop, no flat collection between the ops ──────────
(assert (= (->list (map (fn [y] (+ y 1)) (mapcat (fn [x] [x (* x 10)]) [1 2 3])))
           (list 2 11 3 21 4 31)) "map over a mapcat sees each spliced element")
(assert (= (->list (map (fn [y] (+ y 1)) (mapcat (fn [x] [x (* x 10)]) [1 2 3])))
           (->list (map t1 (mapcat mc [1 2 3]))))
        "the fused composition agrees with the un-fused staged ops")
(assert (= (->list (mapcat (fn [y] [y (- y)]) (map (fn [x] (+ x 1)) [1 2 3])))
           (list 2 -2 3 -3 4 -4))
        "mapcat over a map prefix fans out the TRANSFORMED values")
(assert (= (->list (filter (fn [y] (odd? y))
                           (mapcat (fn [x] [x (* x 10)]) [1 2 3]))) (list 1 3))
        "a filter outer to a mapcat runs inside the inner walk")

# A length-preserving stage inner to a mapcat is admitted: `len` still decides the
# base's emptiness, which is what the `()` arm answers off.
(assert (= (->list (mapcat (fn [y] [y y])
                           (map-indexed (fn [i x] (+ i x)) [10 20])))
           (list 10 10 21 21))
        "a map-indexed inner to a mapcat numbers by the base walk")

# ── scalar terminals over a mapcat ─────────────────────────────────────
(assert (= (fold (fn (a y) (+ a y)) 0 (mapcat (fn [x] [x (* x 10)]) [1 2 3])) 66)
        "fold over a mapcat folds every spliced element")
(assert (= (count (fn [y] (odd? y)) (mapcat (fn [x] [x (* x 10)]) [1 2 3])) 2)
        "count over a mapcat tallies every spliced element")
(assert (= (count (fn [y] (odd? y)) (mapcat (fn [x] [x (* x 10)]) [1 2 3]))
           (count p1 (mapcat mc [1 2 3])))
        "the fused terminal agrees with the un-fused staged ops")
(assert (any? (fn [y] (even? y)) (mapcat (fn [x] [x x]) [1 3 4]))
        "a search over a mapcat decides on a spliced element")

# A mapcat RENUMBERS — one base element becomes a run of any length — so a
# `find-index` past one answers with the survivor count the pipeline carries, never
# the base index. Here the deciding element sits at flat position 3 while the base
# walk is only on its second element.
(assert (= (find-index (fn [y] (even? y)) (mapcat (fn [x] [x x x]) [1 2 3])) 3)
        "find-index over a mapcat answers a position in the FLAT collection")
(assert (= (find-index (fn [y] (even? y)) (mapcat (fn [x] [x x x]) [1 2 3]))
           (find-index p2 (mapcat tri [1 2 3])))
        "…and disagrees with the un-fused form nowhere")

# ── the declines ───────────────────────────────────────────────────────
# The inner walk is indexed, so a function whose result is not a proven array stays a
# plain call — the stdlib op walks a list with first/rest, which the fused form has no
# stage for.
(assert (= (->list (mapcat (fn [x] (list x (* x 10))) [1 2])) (list 1 10 2 20))
        "a list-returning function declines to the stdlib op")
(assert (= (->list (mapcat (fn [x] (if (odd? x) [x] [])) [1 2 3 4 5]))
           (list 1 3 5))
        "a result reached only through a branch is unproven and declines")

# A mapcat can hand an empty collection on from a non-empty base, so it is refused
# inside any untyped array arm — `len` decides that arm's emptiness off the BASE.
(assert (= (->list (take-while (fn [y] (< y 20))
                               (mapcat (fn [x] [x (* x 10)]) [1 2 3])))
           (list 1 10 2))
        "a take-while outer to a mapcat declines and takes the stdlib value")
(assert (= (->list (mapcat (fn [y] [y y]) (mapcat (fn [x] [x]) [1 2])))
           (list 1 1 2 2))
        "a mapcat inner to another mapcat declines and takes the stdlib value")
(assert (= (->list (mapcat (fn [y] [y y]) (filter (fn [x] (odd? x)) [1 2 3])))
           (list 1 1 3 3))
        "a filter inner to a mapcat declines and takes the stdlib value")

# A capturing function is left alone; the value is unchanged.
(def bump 5)
(assert (= (->list (mapcat (fn [x] [x bump]) [1 2])) (list 1 5 2 5))
        "a capturing function declines and still computes the stdlib value")

# ── the mutable-array arm ──────────────────────────────────────────────
# `mapcat`'s array arm walks the base through `each`, which captures `(length seq)`
# once and reads the base live — exactly what the fused loop does — so a lone mapcat
# fuses over a mutable base, with the accumulator returned unfrozen as ever.
(def @mut @[1 2])
(assert (= (->list (mapcat (fn [x] [x (* x 2)]) mut)) (list 1 2 2 4))
        "a lone mapcat fuses over a mutable @array base")
(assert (= (length mut) 2) "the walk does not disturb the mutable base")
(assert (mutable? (mapcat (fn [x] [x]) @[1 2])) "and its result is unfrozen")
(assert (= (->list (map (fn [y] (+ y 1)) (mapcat (fn [x] [x]) @[1 2])))
           (list 2 3))
        "a composition over a mutable base declines and takes the stdlib value")

# ── named functions ────────────────────────────────────────────────────
(defn pairup [x]
  [x (* x 10)])
(assert (= (->list (mapcat pairup [1 2 3])) (list 1 10 2 20 3 30))
        "a named same-unit function whose body proves an array inlines")
(assert (= (->list (mapcat pairup [1 2 3])) (->list (mapcat mc [1 2 3])))
        "…and agrees with the un-fused form")

# ── the base survives the walk ─────────────────────────────────────────
(def base [7 8])
(assert (= (->list (mapcat (fn [x] [x x]) base)) (list 7 7 8 8))
        "a Var-bound base fuses")
(assert (= (get base 0) 7) "the base Var survives the fused walk")

# ── Realization: the flat collection between the ops ───────────────────
# `arena/total-allocs` is a cumulative, monotonic count of objects ever minted
# (docs/impl/dissolution.md § "The gauge"). A LONE mapcat is not weighed: its array
# arm walks with two `each` macro expansions rather than a `letrec` closure, and the
# per-element array its function returns is minted by both forms, so there is no
# closure to dissolve. What fusion removes is the flat collection between the mapcat
# and whatever consumes it.
(defn allocs [thunk]
  (let [before (arena/total-allocs)]
    (thunk)
    (- (arena/total-allocs) before)))

(def nums [0 1 2 3 4 5 6 7 8 9])
(def comp-fused
  (allocs (fn [] (map (fn [y] (+ y 1)) (mapcat (fn [x] [x (* x 10)]) nums)))))
(def comp-unfused (allocs (fn [] (map t1 (mapcat mc nums)))))
(assert (= (->list (map (fn [y] (+ y 1)) (mapcat (fn [x] [x (* x 10)]) nums)))
           (->list (map t1 (mapcat mc nums))))
        "fused and un-fused map-over-mapcat compute the same value")
(assert (< comp-fused comp-unfused)
        (string "a fused map-over-mapcat must mint fewer (no flat collection): "
                comp-fused " vs " comp-unfused))

# A scalar terminal removes the flat collection AND the walker closure its own array
# arm binds in a `letrec`, so its saving is strictly larger than the composition's.
(def term-fused
  (allocs (fn [] (count (fn [y] (odd? y)) (mapcat (fn [x] [x (* x 10)]) nums)))))
(def term-unfused (allocs (fn [] (count p1 (mapcat mc nums)))))
(assert (= (count (fn [y] (odd? y)) (mapcat (fn [x] [x (* x 10)]) nums))
           (count p1 (mapcat mc nums)))
        "fused and un-fused count-over-mapcat compute the same value")
(assert (> (- term-unfused term-fused) (- comp-unfused comp-fused))
        (string "a scalar terminal saves the walker closure on top of the flat "
                "collection: count saved " (- term-unfused term-fused)
                ", map saved " (- comp-unfused comp-fused)))

(println "dissolution-mapcat-fuse: ok (map-over-mapcat saved "
         (- comp-unfused comp-fused) ", count-over-mapcat saved "
         (- term-unfused term-fused) ")")
