(elle/epoch 15)
# audited: 2026-11-10
# A top-level def that a loop runs again stays readable after the loop, through a closure or directly.
# docs/impl/region/bindings.md
#
# A `def` inside a top-level `while` re-stores the file binding's cell on every
# pass. The loop also carries the name forward: each pass copies the cell's
# content into the loop's own version of the binding, and every read after the
# loop reads that version. The copy is a read out of the cell, which keeps its
# own reference.
#
# The counter-factual: the gate donated the copy's store as if it held a fresh
# allocation, because the walk names no region for the cell's content. A
# donated store takes no reference, yet its drop-on-overwrite releases the
# displaced value, which the cell releases too. The next pass then writes over
# a freed value, and the sidecar's guardfree faults there.

(defn churn []
  (var i 0)
  (while (< i 300)
    (array i (string i) @[i])
    (assign i (+ i 1))))

# (a) read after the loop through a closure.
(def @a-i 0)
(while (< a-i 3)
  (def a-v @[a-i 41])
  (assign a-i (+ a-i 1)))
(defn a-read-fn []
  (get a-v 1))
(churn)
(def a-read (a-read-fn))

# (b) read after the loop directly.
(def @b-i 0)
(while (< b-i 3)
  (def b-v @[b-i 42])
  (assign b-i (+ b-i 1)))
(churn)
(def b-read (get b-v 1))

# (c) a def in the inner of two nested loops.
(def @c-i 0)
(while (< c-i 3)
  (def @c-j 0)
  (while (< c-j 2)
    (def c-v @[c-i c-j 43])
    (assign c-j (+ c-j 1)))
  (assign c-i (+ c-i 1)))
(defn c-read-fn []
  (get c-v 2))
(churn)
(def c-read (c-read-fn))

# (d) a def that only some passes run.
(def @d-i 0)
(while (< d-i 4)
  (when (odd? d-i) (def d-v @[d-i 44]))
  (assign d-i (+ d-i 1)))
(defn d-read-fn []
  (get d-v 0))
(churn)
(def d-read (d-read-fn))

# (e) the control: an `assign` the loop runs again, where the binding is
# declared mutable and so is no def's cell.
(def @e-v nil)
(def @e-i 0)
(while (< e-i 3)
  (assign e-v @[e-i 45])
  (assign e-i (+ e-i 1)))
(defn e-read-fn []
  (get e-v 1))
(churn)
(def e-read (e-read-fn))

(assert (= a-read 41) "a def a loop re-stores is read through a closure")
(assert (= (get a-v 0) 2) "the closure reads the last pass's value")
(assert (= b-read 42) "a def a loop re-stores is read directly")
(assert (= (get b-v 0) 2) "the direct read sees the last pass's value")
(assert (= c-read 43) "a def in a nested loop is read through a closure")
(assert (= [(get c-v 0) (get c-v 1)] [2 1]) "the nested def keeps its last pass")
(assert (= d-read 3) "a def some passes skip keeps the last pass that ran it")
(assert (= e-read 45) "an assign a loop re-runs is read through a closure")

(println "region-def-loop-store-uaf: ok")
