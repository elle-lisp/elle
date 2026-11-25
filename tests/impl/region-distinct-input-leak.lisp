(elle/epoch 18)
# audited: 2026-11-24
# A kept result of distinct over an array does not keep the array alive.
# docs/impl/region/rules.md
#
# Each call builds a fresh input array and keeps distinct's result in `keep`.
# The kept results cost one object per call. An input the result keeps alive
# costs one more per call, so the window reads twice the window.
#
# The counter-factual: an `each` that reads its element with `first` on the
# list arm and `get` on the indexed arm, in one binding, keeps distinct's input
# alive with its result when distinct walks the array with it.
#
# The subject runs through a mutable binding, so no graft folds the call.

(def window 2000)

(include-file "gauge/window.lisp")

(def keep @[])
(def @distinct-call distinct)

(defn fresh-array []
  [(keyword "a") (keyword "b") (keyword "a")])

(defn fresh-mutable []
  @[(keyword "a") (keyword "b") (keyword "a")])

(def rows
  [["array   " (fn [] (push keep (distinct-call (fresh-array))))]
   ["@array  " (fn [] (push keep (distinct-call (fresh-mutable))))]])

(println "region-distinct-input-leak over " window " calls:")
(each row in rows
  (let [d (measure (get row 1) 100 window)]
    (println "  " (get row 0) " " d)
    (assert (< d (+ window (/ window 2)))
            (string (get row 0) " keeps its input, delta=" d))))

# Value preservation: the first occurrence of each element, in order.
(assert (= (distinct-call [:a :b :a]) [:a :b]) "distinct over an array")
(assert (= (freeze (distinct-call @[:a :b :a])) [:a :b])
        "distinct over an @array")
(assert (mutable? (distinct-call @[:a])) "an @array gives an @array")

(println "region-distinct-input-leak: ok")
