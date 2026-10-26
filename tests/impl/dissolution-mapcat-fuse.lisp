(elle/epoch 15)
# audited: 2026-10-26
# A fused `mapcat`, in a chain or under a terminal, computes exactly what the
# staged stdlib ops compute, and a consumed one mints fewer objects.
#
# docs/impl/dissolution/stages.md
#
# The codegen pins — the dispatch gone, the body inline, two `while`s, one
# accumulator — are in src/hir/typeinfer/fuse/tests/mapcat.rs.
#
# The trap: the fused inner walk is indexed, so a function whose result is a list
# would make `(get inner j)` O(j) and the fused walk quadratic. Fusion is gated on
# the function's body proving an ARRAY result; the declines below pin that gate.
#
# The cross-check reference applies the same ops through named functions with a
# `block` body. A fragment cannot close over a `block`, so those stay plain staged
# calls and mint what the fused form does not. Same value.

(defn mc [x]
  (block :oracle
    [x (* x 10)]))
(defn t1 [x]
  (block :oracle
    (+ x 1)))
(defn p1 [x]
  (block :oracle
    (odd? x)))
(defn tri [x]
  (block :oracle
    [x x x]))
(defn p2 [x]
  (block :oracle
    (even? x)))

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

# ── the result keeps the base's type ───────────────────────────────────
# An immutable base gives an immutable result, an empty base included, and so does
# a non-empty base whose every result is empty. The counter-factual is a fused loop
# that returns its @array accumulator unfrozen.
(assert (= (type-of (mapcat (fn [x] [x]) [1 2 3])) :array)
        "the fused result over an immutable base is frozen")
(assert (= (type-of (mapcat (fn [x] [x]) [])) :array)
        "an empty base gives an empty array")
(assert (= (type-of (mapcat (fn [x] []) [1 2 3])) :array)
        "a base whose every result is empty gives an empty array")
(assert (= (type-of (mapcat mc [])) (type-of (mapcat (fn [x] [x (* x 10)]) [])))
        "fused and un-fused agree on the empty base")

# A chain holding a mapcat is frozen at its end, like every chain over an
# immutable base.
(assert (= (type-of (map (fn [y] (+ y 1)) (mapcat (fn [x] [x (* x 10)]) [1 2])))
           :array) "a map over a mapcat is frozen too")
(assert (= (type-of (map (fn [y] (+ y 1)) (mapcat (fn [x] [x (* x 10)]) [])))
           :array) "an empty base gives an empty array through the whole chain")

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

# Any stage sits inside a mapcat. A map-indexed numbers by the base walk, and a
# filter that empties a non-empty base leaves the mapcat an empty array to give.
(assert (= (->list (mapcat (fn [y] [y y])
                           (map-indexed (fn [i x] (+ i x)) [10 20])))
           (list 10 10 21 21))
        "a map-indexed inner to a mapcat numbers by the base walk")
(assert (= (->list (mapcat (fn [y] [y y]) (filter (fn [x] (odd? x)) [1 2 3])))
           (->list (mapcat (fn [y] [y y]) (filter p1 [1 2 3]))))
        "a filter inner to a mapcat agrees with the un-fused staged ops")
(assert (= (type-of (mapcat (fn [y] [y y]) (filter (fn [x] (odd? x)) [2 4])))
           :array) "a filter that empties a non-empty base gives an empty array")
(assert (= (->list (mapcat (fn [y] [y y]) (mapcat (fn [x] [x (* x 10)]) [1 2])))
           (->list (mapcat (fn [y] [y y]) (mapcat mc [1 2]))))
        "a mapcat inner to another mapcat agrees with the un-fused staged ops")

# A take-while outer to a mapcat gates its own stage inside the fan-out's walk, and
# the base walk runs on as the staged form's does.
(assert (= (->list (take-while (fn [y] (odd? y))
                               (mapcat (fn [x] [x (* x 10)]) [1 2 3])))
           (->list (take-while p1 (mapcat mc [1 2 3]))))
        "a take-while outer to a mapcat agrees with the un-fused staged ops")

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

# A mapcat renumbers, so a map-indexed outer to one declines and takes the stdlib
# value.
(assert (= (->list (map-indexed (fn [i y] (* i y)) (mapcat (fn [x] [x x]) [1 2])))
           (list 0 1 4 6))
        "a map-indexed outer to a mapcat declines and takes the stdlib value")

# A function reading an enclosing local fuses: the splice is the call site, so the
# name is in scope there.
(assert (= (->list (let [bump 5]
                     (mapcat (fn [x] [x bump]) [1 2]))) (list 1 5 2 5))
        "a capturing function fuses to the stdlib value")

# ── the mutable-array arm ──────────────────────────────────────────────
# `mapcat`'s array arm walks the base through `each`, which captures `(length seq)`
# once and reads the base live — exactly what the fused loop does — so a lone mapcat
# fuses over a mutable base, and returns its accumulator unfrozen.
(def @mut @[1 2])
(assert (= (->list (mapcat (fn [x] [x (* x 2)]) mut)) (list 1 2 2 4))
        "a lone mapcat fuses over a mutable @array base")
(assert (= (length mut) 2) "the walk does not disturb the mutable base")
(assert (= (type-of (mapcat (fn [x] [x]) @[1 2])) :@array)
        "a mutable base gives a mutable result")
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
# A LONE mapcat is not weighed: its array arm walks with two `each` macro
# expansions rather than a `letrec` closure, and both forms mint the per-element
# array its function returns, so there is no closure to dissolve. What fusion
# removes is the flat collection between the mapcat and whatever consumes it.
# `arena/total-allocs` counts every object ever minted.
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
