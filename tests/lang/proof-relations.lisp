(elle/epoch 15)
# audited: 2026-11-18
# std/proof: a relation, and the merge, join, antijoin and map of relations, after datafrog 2.0.1.
# docs/stdlib/proof.md
#
# The counter-factual: a relation that keeps its tuples in insertion order
# passes any test that sorts what it reads. Each assertion here compares the
# array `elements` returns, in the order it returns it.

(def proof ((import "std/proof")))

(defn tuples [r]
  "A relation's tuples."
  (proof:elements r))

# ── a relation is its tuples, sorted and distinct ─────────────────────
(assert (= [[1 2] [1 3] [2 1]]
           (tuples (proof:relation [[2 1] [1 3] [1 2] [1 3]])))
        "a relation sorts its tuples and keeps one of each")
(assert (= :array (type-of (tuples (proof:relation @[[2 1] [1 2]]))))
        "elements returns an immutable array")
(assert (= [1 2 3] (tuples (proof:relation (list 3 1 2 1))))
        "a relation builds from a list, and a tuple is any value")
(assert (= [] (tuples (proof:relation []))) "an empty relation has no tuples")
(assert (= 1 (length (tuples (proof:relation [[1] @[1]]))))
        "[1] and @[1] are one tuple")
(assert (= 3 (length (tuples (proof:relation [2 1.0 1 1.5]))))
        "1 and 1.0 are one tuple")
(assert (= (sort ["s" [1] :k nil 1])
           (tuples (proof:relation ["s" [1] :k nil 1])))
        "tuples of every kind sort by the one order")

(let [source @[[2] [1]]
      r (proof:relation source)]
  (push source [0])
  (assert (= [[1] [2]] (tuples r)) "a relation does not follow its source array"))

# ── merge ─────────────────────────────────────────────────────────────
(defn merged [xs ys]
  (tuples (proof:merge (proof:relation xs) (proof:relation ys))))

(assert (= [1 2 3] (merged [] [3 1 2]))
        "merge with an empty relation on the left")
(assert (= [1 2 3] (merged [3 1 2] [])) "and on the right")
(assert (= [] (merged [] [])) "and on both sides")
(assert (= [1 2 3 4] (merged [1 2] [3 4])) "merge of two relations in sequence")
(assert (= [1 2 3 4] (merged [3 4] [1 2])) "and in the other sequence")
(assert (= [1 2 3 4 5] (merged [1 3 5] [2 4])) "merge interleaves")
(assert (= [1 2 3] (merged [1 2 3] [2 3])) "merge keeps one of a shared tuple")
(assert (= [1 2 3] (merged [1 2] [2 3])) "also where the two meet end to end")
(assert (= [1 2 3 4 5 6] (merged [1 4 5] [1 2 3 6]))
        "and where the two start alike")
(assert (= [1] (merged [1] [1])) "a relation merged with itself is itself")

# ── join, antijoin and map over relations ─────────────────────────────
(def left (proof:relation [[1 :a] [1 :b] [2 :c] [4 :d]]))
(def right (proof:relation [[1 :x] [2 :y] [2 :z] [3 :w]]))

(assert (= (sort [[1 :a :x] [1 :b :x] [2 :c :y] [2 :c :z]])
           (tuples (proof:join left right (fn [k v1 v2] [k v1 v2]))))
        "join gives f every pair of values that share a key")
(assert (= [:same] (tuples (proof:join left right (fn [k v1 v2] :same))))
        "and returns a relation, so equal results are one tuple")
(assert (= [] (tuples (proof:join left (proof:relation []) (fn [k v1 v2] k))))
        "a join with an empty relation is empty")
(assert (= [[[1 2] :a :x]]
           (tuples (proof:join (proof:relation [[[1 2] :a] [[1 3] :b]])
                               (proof:relation [[[1 2] :x] [[2 2] :y]])
                               (fn [k v1 v2] [k v1 v2]))))
        "a key may be an array, which keys a rule on two columns")
(assert (= [[1 :a :x]]
           (tuples (proof:join (proof:relation [[1 :a]])
                               (proof:relation [[1.0 :x]])
                               (fn [k v1 v2] [k v1 v2]))))
        "two keys match when they compare 0")

(assert (= [[4 :d]]
           (tuples (proof:antijoin left (proof:relation [1 2 3])
                                   (fn [k v] [k v]))))
        "antijoin keeps the tuples whose key the second relation lacks")
(assert (= (tuples left)
           (tuples (proof:antijoin left (proof:relation []) (fn [k v] [k v]))))
        "an antijoin against an empty relation keeps every tuple")

(assert (= [[1 2] [2 1] [3 1]]
           (tuples (proof:map (proof:relation [[1 2] [1 3] [2 1]])
                              (fn [[a b]] [b a]))))
        "map gives f each tuple, and sorts what f returns")
(assert (= [:t] (tuples (proof:map left (fn [t] :t))))
        "and keeps one of each result")

# ── the submodules compose as the module does ─────────────────────────
(def relation ((import "std/proof/relation")))
(def variable ((import "std/proof/variable") :relation relation))
(def leapjoin ((import "std/proof/leapjoin") :relation relation))
(assert (= [1 2 3] (relation:elements (relation:relation [3 1 2 1])))
        "the relation submodule stands alone")
(let [it (variable:iteration)
      v (variable:variable it "v")]
  (variable:extend v [3 1])
  (while (variable:changed? it) nil)
  (assert (= [1 3] (relation:elements (variable:complete v)))
          "the variable submodule builds on the relation it is given"))
(assert (= [[1 nil]]
           (relation:elements (leapjoin:leapjoin (relation:relation [1 2])
                                (leapjoin:prefix-filter (fn [t] (= t 1)))
                                (fn [t v] [t v]))))
        "and so does the leapjoin submodule")

(println "proof-relations: all tests passed")
