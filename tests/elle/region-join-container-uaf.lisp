(elle/epoch 13)
# audited: 2026-09-29
# Guard for the append-only container join: a pushed value stays readable while anything holds it.
# docs/impl/region/colocation.md
#
# Each builder below is one the append seed admits, so every pushed value is born
# in the container's region and holds no count of its own. After the builder
# returns, one reference to that region keeps them all. Each shape drops the
# container's own holder while an element, a copy, or another container still
# names a value inside it, churns region ids, and reads the value back. A region
# freed early faults under --trace=guardfree.

(defn pairs [n]
  (def out @[])
  (var k 0)
  (while (< k n)
    (push out [k (string "v" k)])
    (assign k (+ k 1)))
  out)

(defn churn []
  (var i 0)
  (while (< i 300)
    (pair (string "c" i) i)
    (assign i (+ i 1))))

# An element read out of a container nothing else holds.
(def one (get (pairs 20) 7))
(churn)
(assert (= (get one 0) 7) "an element outlives its container's holder")
(assert (= (get one 1) "v7") "and so does the string it holds")

# A frozen copy holds the container's values.
(def frozen (freeze (pairs 20)))
(churn)
(assert (= (get (get frozen 19) 1) "v19")
        "a frozen copy keeps the joined values")

# An element stored into another container.
(def other @[])
(let [built (pairs 20)]
  (push other (get built 3))
  (push other (get built 11)))
(churn)
(assert (= (get (get other 1) 1) "v11") "another container keeps a joined value")

# A pop from the returned container hands the caller the popped value.
(def popped
  (let [built (pairs 20)]
    (pop built)))
(churn)
(assert (= (get popped 1) "v19") "a popped value outlives its container")

# The builder in a loop: each container frees whole, and the next reuses its ids.
(var total 0)
(var round 0)
(while (< round 50)
  (let [built (pairs 30)]
    (assign total (+ total (get (get built 29) 0))))
  (assign round (+ round 1)))
(assert (= total (* 50 29)) "every round read its own container")

# A closure over an element.
(def reader
  (let [e (get (pairs 10) 4)]
    (fn [] (get e 1))))
(churn)
(assert (= (reader) "v4") "a captured element outlives its container")

(println "region-join-container-uaf: ok")
