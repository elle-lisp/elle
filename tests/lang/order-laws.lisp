(elle/epoch 15)
# audited: 2026-11-18
# The one order of all values is an order, and `sort`, a struct's keys and a set's members follow it.
# docs/types.md
#
# The counter-factual: three orders existed. A struct ranked its keys with a
# string ahead of a keyword and a list pair after an array, and a set put the
# empty list ahead of a string. An assertion below that reads two of
# `compare`, `sort`, `keys` and a set fails when the two disagree.

(def nan (/ 0.0 0.0))
(def inf (/ 1.0 0.0))
(def f (fn [] 1))
(def g (fn [] 2))
(def fib (fiber/new (fn [] 1) 0))
(def par (parameter 0))
(def bx (box 1))

# ── the laws of an order, over a sample of every kind ─────────────────
(def sample
  [nil true 1 'a :k "s" () (list 1) [1] b[1] {:a 1} f bx length fib par |1|
   false -1 0 0.5 1.0 2 -0.0 0.0 nan inf 'b :j "" "sa" (list 1 2) (list 2) []
   [1 2] @[1] @[0 5] [[1] "s"] b[] @b[1] {} {:a 2} @{:a 1} g || |2| @|1|])

(each a in sample
  (assert (= 0 (compare a a)) (string "every value compares 0 with itself: " a))
  (each b in sample
    (let [ab (compare a b)]
      (assert (or (= ab -1) (= ab 0) (= ab 1))
              (string "compare returns -1, 0 or 1: " a " against " b))
      (assert (= ab (- (compare b a)))
              (string "compare is antisymmetric: " a " against " b))
      (assert (= (= ab 0) (= 1 (length (set a b))))
              (string "two values compare 0 exactly when a set holds them as one member: "
                      a " against " b))
      (when (<= ab 0)
        (each c in sample
          (when (<= (compare b c) 0)
            (assert (<= (compare a c) 0)
                    (string "compare is transitive: " a ", " b ", " c))))))))

# ── sort, struct keys and set members follow compare ──────────────────
(let [sorted (sort sample)]
  (assert (= (length sorted) (length sample)) "sort keeps every element")
  (each i in (range (- (length sorted) 1))
    (assert (<= (compare (get sorted i) (get sorted (+ i 1))) 0)
            (string "sort leaves no pair out of the order compare reports, at "
                    i))))

(assert (= [[0 9] [1 2] [1 3]] (sort [[1 3] [0 9] [1 2]]))
        "an array of arrays sorts")

(assert (= (list :a "a") (keys {"a" 1 :a 2}))
        "a struct lists a keyword key before a string key")

(def keyed
  (struct fib 11 f 10 b[1] 9 [1] 8 (list 1) 7 () 6 "s" 5 :k 4 'q 3 7 2 true 1
          nil 0))
(def key-order (list nil true 7 'q :k "s" () (list 1) [1] b[1] f fib))

(assert (= key-order (keys keyed))
        "a struct's keys come out in the order of all values")
(assert (= key-order (keys (thaw keyed))) "and so do an @struct's")
(assert (= (list 0 1 2 3 4 5 6 7 8 9 10 11) (values keyed))
        "the values follow their keys")
(assert (= (->array key-order) (sort (->array (keys keyed))))
        "which is the order sort gives the same keys")

(def member-order [nil true 7 'q :k "s" () (list 1) [1] b[1] f fib])
(assert (= member-order
           (->array (set fib f b[1] [1] (list 1) () "s" :k 'q 7 true nil)))
        "a set's members come out in the order of all values")
(let [ms @||]
  (each m in (reverse member-order)
    (add ms m))
  (assert (= member-order (->array ms)) "and so do an @set's"))


(println "order-laws: all tests passed")
