(elle/epoch 12)
# audited: 2026-09-20
# tests/elle/trait-operator-fallback.lisp — :iter, :empty and :conj carry the
# whole collection operator family.
# docs/traits.md
#
## An operator with no method of its own drives the :iter trait method, and
## rebuilds its answer with :Collection :empty and :conj.
##
## The counter-factual: `bag` below implements nothing but those three
## methods, so before the operators consulted the trait table every assertion
## here raised :type-error. The builtin assertions at the foot are the other
## half of the claim — a plain set, struct or array must reach none of this.

(defn bag [items]
  "A collection that carries :iter, :empty and :conj, and nothing else."
  (with-traits {:items items}
               @{:Sequence {:iter (fn [self]
                                    (fiber/new (fn []
                                      (each x in (self :items)
                                        (yield x))) |:yield|))}
                 :Collection {:empty (fn [self] (bag []))
                              :conj (fn [self x]
                                      (bag (append (self :items) [x])))}}))

(defn held [b]
  "The elements a bag holds, as an array."
  (b :items))

# ============================================================================
# map, filter, reduce, each
# ============================================================================

(assert (= (held (map (fn [x] (+ x 1)) (bag [1 2 3]))) [2 3 4])
        "map runs the iterator and rebuilds with :conj")

(assert (= (held (filter even? (bag [1 2 3 4]))) [2 4])
        "filter runs the iterator and rebuilds with :conj")

(assert (= (reduce (fn [acc x] (+ acc x)) 0 (bag [1 2 3])) 6)
        "reduce folds over the iterator")

(assert (= (fold (fn [acc x] (* acc x)) 1 (bag [2 3 4])) 24)
        "fold folds over the iterator")

(begin
  (def seen @[])
  (each x in (bag [1 2 3])
    (push seen x))
  (assert (= (freeze seen) [1 2 3]) "each walks the iterator"))

# `each` expands to a while loop, so break still leaves the enclosing block —
# which is why the iterator arm materializes rather than calling a closure.
(assert (= (block :found
             (each x in (bag [1 2 3])
               (when (> x 1) (break :found x)))
             nil) 2) "break inside each still leaves the enclosing block")

# ============================================================================
# The rest of the operator family
# ============================================================================

(assert (= (held (map-indexed (fn [i x] (+ i x)) (bag [10 20 30]))) [10 21 32])
        "map-indexed")
(assert (= (held (distinct (bag [1 1 2 1 3]))) [1 2 3]) "distinct")
(assert (= (held (take-while (fn [x] (< x 3)) (bag [1 2 3 1]))) [1 2])
        "take-while")
(assert (= (held (drop-while (fn [x] (< x 3)) (bag [1 2 3 1]))) [3 1])
        "drop-while")
(assert (= (held (interpose 0 (bag [1 2 3]))) [1 0 2 0 3]) "interpose")
(assert (= (held (mapcat (fn [x] [x x]) (bag [1 2]))) [1 1 2 2]) "mapcat")
(assert (= (held (flatten (bag [[1 2] [3]]))) [1 2 3]) "flatten")
(assert (= (held (sort-by (fn [x] (- 0 x)) (bag [1 3 2]))) [3 2 1]) "sort-by")
(assert (= (held (sort-with (fn [a b] (- a b)) (bag [3 1 2]))) [1 2 3])
        "sort-with")
(assert (= (held (reverse (bag [1 2 3]))) [3 2 1]) "reverse")
(assert (= (held (butlast (bag [1 2 3]))) [1 2]) "butlast")
(assert (= (last (bag [1 2 3])) 3) "last")
(assert (= (count even? (bag [1 2 3 4])) 2) "count")
(assert (= (find-index (fn [x] (= x 3)) (bag [1 2 3])) 2) "find-index")
## take and drop run over the elements and answer the shapes they answer for
## an array: take builds a list, drop hands back the remaining slice.
(assert (= (take 2 (bag [1 2 3])) (list 1 2)) "take")
(assert (= (drop 2 (bag [1 2 3])) [3]) "drop")

(assert (= (map freeze (held (partition 2 (bag [1 2 3 4])))) [[1 2] [3 4]])
        "partition chunks the iterator")

(assert (any? (fn [x] (= x 2)) (bag [1 2 3])) "any?")
(assert (all? (fn [x] (> x 0)) (bag [1 2 3])) "all?")
(assert (= (find (fn [x] (> x 1)) (bag [1 2 3])) 2) "find")

# frequencies and group-by are written with `each`, so they inherit its
# dispatch rather than carrying a trait arm of their own.
(assert (= (frequencies (bag [:a :a :b])) {:a 2 :b 1}) "frequencies")
(assert (= (freeze (get (group-by (fn [x] (if (even? x) :even :odd))
                                  (bag [1 2 3 4])) :even)) [2 4]) "group-by")
(assert (= (zip (bag [1 2]) [10 20]) (list (list 1 10) (list 2 20)))
        "zip reads :iter on each input it is given")

# ============================================================================
# What the fallback refuses
# ============================================================================

(begin
  (def opaque (with-traits {:a 1} @{:Sequence {:first (fn [self] 1)}}))
  (let [[mapped? mapped-err] (protect (map identity opaque))]
    (assert (not mapped?) "no method and no :iter is still an error")
    (assert (= (get mapped-err :error) :type-error) "and it is a :type-error"))
  # A traited struct still walks its own pairs under `each` — the struct arm
  # is the builtin handling the third layer sits behind. A value that is no
  # container at all is what `each` refuses.
  (let [[walked? _] (protect (each x in 42
                               x))]
    (assert (not walked?) "each refuses a value that is no collection")))

# A collection that iterates but cannot be rebuilt answers the scalar
# operators and refuses the collection-valued ones.
(begin
  (def stream-only
    (with-traits {:tag :stream-only}
                 @{:Sequence {:iter (fn [self]
                                      (fiber/new (fn []
                                        (yield 1)
                                        (yield 2)) |:yield|))}}))
  (assert (= (fold (fn [acc x] (+ acc x)) 0 stream-only) 3)
          "a scalar operator needs only :iter")
  (let [[mapped? _] (protect (map identity stream-only))]
    (assert (not mapped?) "map needs :empty and :conj to answer")))

# An iterator that raises must not look like an exhausted one.
(begin
  (def boom
    (with-traits {:tag :boom}
                 @{:Sequence {:iter (fn [self]
                                      (fiber/new (fn []
                                        (yield 1)
                                        (error {:error :boom})) |:yield|))}}))
  (let [[ok? err] (protect (fold (fn [acc x] (+ acc x)) 0 boom))]
    (assert (not ok?) "a raising iterator does not silently truncate")
    (assert (= (get err :error) :boom) "and its error reaches the caller")))

# ============================================================================
# The builtin container families reach none of this
# ============================================================================

(assert (= (map (fn [x] (* x 2)) [1 2 3]) [2 4 6]) "array map is unchanged")
(assert (= (map (fn [x] (* x 2)) (list 1 2)) (list 2 4)) "list map is unchanged")
(assert (= (map (fn [x] (* x 2)) (set 1 2 3)) (set 2 4 6))
        "set map is unchanged")
(assert (= (filter even? [1 2 3 4]) [2 4]) "array filter is unchanged")
(assert (= (fold (fn [acc x] (+ acc x)) 0 [1 2 3]) 6) "array fold is unchanged")
(assert (= (fold (fn [acc kv] (+ acc (get kv 1))) 0 {:a 1 :b 2}) 3)
        "a plain struct still folds over its key-value pairs")

(begin
  (def keys-seen @[])
  (each [k v] in {:a 1 :b 2}
    (push keys-seen k))
  (assert (= (freeze keys-seen) [:a :b])
          "each over a plain struct still walks its pairs"))

(begin
  (def set-seen @[])
  (each x in (set 1 2 3)
    (push set-seen x))
  (assert (= (length set-seen) 3) "each over a plain set still walks it"))

(println "trait-operator-fallback: all tests passed")
