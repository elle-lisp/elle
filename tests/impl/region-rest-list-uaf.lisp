(elle/epoch 14)
# audited: 2026-10-06
# A rest list in one region frees its cells with the head, and leaves every element its caller still reads.
# docs/impl/region/restlist.md
#
# Under guardfree a freed page faults at its next read. Each callee below only
# reads its rest list, so the list takes one region. Every element is a heap
# value in a region of its own, some passed twice, and the caller reads each
# result after the list is gone. Counterfactual: a list region whose free
# releases an element it does not count, or a cell freed before its last read,
# faults here. The loop runs past the JIT threshold, so both tiers build lists.

(defn first-of [& xs]
  "first, which answers an element."
  (first xs))

(defn second-of [& xs]
  "second, which answers an element."
  (second xs))

(defn spliced [& xs]
  "A spliced call: the callee receives the elements."
  (string ;xs))

(defn count-args [& xs]
  "Length, an Immediate primitive."
  (length xs))

(defn held [v]
  "V, from a fixed-arity call, which keeps each call out of tail position."
  v)

(let [@i 0]
  (while (%lt i 50)
    (let [a (string "a" i)
          b (string "b" i)]
      (assert (= (held (first-of a b a)) (string "a" i)) "first of three")
      (assert (= (held (second-of a b a)) (string "b" i)) "second of three")
      (assert (= (held (spliced a b a)) (string "a" i "b" i "a" i)) "splice")
      (assert (= (held (count-args a b a b)) 4) "count")
      (assert (= (held (first-of (string "x" i) b)) (string "x" i))
              "an element only the list and the result hold")
      (assert (= a (string "a" i)) "the caller's element outlives the list")
      (assert (= b (string "b" i)) "and so does the other"))
    (assign i (%add i 1))))

(println "region-rest-list-uaf: ok")
