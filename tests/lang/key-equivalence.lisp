(elle/epoch 15)
# audited: 2026-11-18
# The key equivalence: which two values are one set member and one hash, and
# that `compare` returning 0 means the same.
# docs/types.md
#
# The counter-factual: `hash` read a value's representation. An array and an
# @array of the same elements hashed apart, and so did two immutable sets
# built apart, though a set held each pair as one member. A table that finds
# a key by its hash then misses a key that the order calls present.

(def nan (/ 0.0 0.0))

# ── mutability is not part of a key ───────────────────────────────────
(def twins
  [[[1 2] @[1 2]] ["ab" @"ab"] [b[1 2] @b[1 2]] [{:a 1} @{:a 1}] [|1 2| @|1 2|]])

(each [fixed mutable] in twins
  (assert (= (hash fixed) (hash mutable))
          (string "a value and its mutable twin hash alike: " fixed))
  (assert (= 1 (length (set fixed mutable)))
          (string "and a set holds the two as one member: " fixed))
  (assert (has? (set fixed) mutable)
          (string "so the twin finds the value in a set: " fixed)))

(assert (= (hash [[1] "a" b[2] {:k |1|}]) (hash @[@[1] @"a" @b[2] @{:k @|1|}]))
        "mutability is ignored at every depth")
(assert (= (hash (list [1] "a")) (hash (list @[1] @"a"))) "and inside a list")

# ── a set hashes by its members ───────────────────────────────────────
(assert (= (hash (set 1 2)) (hash (apply set (range 1 3))))
        "two sets built apart hash alike")
(assert (= (hash |1 2 3|) (hash (add |1 2| 3)))
        "a set hashes as the set it grew into")
(assert (not= (hash |1 2|) (hash |1 3|)) "two sets of other members hash apart")

# ── numbers ───────────────────────────────────────────────────────────
(assert (= (hash 1) (hash 1.0)) "an integer and a float of one value hash alike")
(assert (= 1 (length (set 1 1.0))) "and are one set member")
(assert (not= (hash -0.0) (hash 0.0)) "-0.0 and 0.0 hash apart")
(assert (= 2 (length (set -0.0 0.0))) "and are two set members")
(assert (= (hash nan) (hash nan)) "a NaN hashes as itself")
(assert (has? (set nan) nan) "and is found in a set that holds it")
(assert (has? (set [nan]) [nan]) "as is a collection that holds one")

# ── compare returning 0 and an equal hash go together ─────────────────
(def f (fn [] 1))
(def fib (fiber/new (fn [] 1) 0))
(def sample
  [nil true false 1 1.0 2 -0.0 0.0 nan 'a :k "s" @"s" () (list 1) [1] @[1] [1 2]
   b[1] @b[1] {:a 1} @{:a 1} f fib |1| @|1| |1 2|])

(each a in sample
  (each b in sample
    (when (= 0 (compare a b))
      (assert (= (hash a) (hash b))
              (string "two values that compare 0 hash alike: " a " and " b)))))

(println "key-equivalence: all tests passed")
