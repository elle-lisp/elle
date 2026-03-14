# Stream combinators — sinks, transforms, port-to-stream converters

(def {:assert-eq assert-eq
      :assert-true assert-true
      :assert-false assert-false
      :assert-err assert-err
      :assert-err-kind assert-err-kind}
  ((import-file "tests/elle/assert.lisp")))

# === Helpers ===

(defn make-range [n]
  "Return a coroutine that yields integers 0..n-1."
  (coro/new (fn []
    (var i 0)
    (while (< i n)
      (yield i)
      (assign i (+ i 1))))))

(defn make-from-list [lst]
  "Return a coroutine that yields each element of lst."
  (coro/new (fn []
    (var remaining lst)
    (while (not (empty? remaining))
      (yield (first remaining))
      (assign remaining (rest remaining))))))

# === Sink combinators ===

# stream/collect: finite coroutine
(assert-eq
  (stream/collect (make-from-list (list 1 2 3)))
  (list 1 2 3)
  "stream/collect: three values in order")

# stream/collect: empty coroutine (immediately done)
(assert-eq
  (stream/collect (make-range 0))
  ()
  "stream/collect: empty source yields empty list")

# stream/fold: sum
(assert-eq
  (stream/fold + 0 (make-range 5))
  10
  "stream/fold: sum 0..4 = 10")

# stream/fold: initial value returned on empty source
(assert-eq
  (stream/fold + 99 (make-range 0))
  99
  "stream/fold: empty source returns init")

# stream/for-each: side effects accumulate into mutable array
(let [[acc @[]]]
  (stream/for-each (fn [v] (push acc v)) (make-range 4))
  (assert-eq (length acc) 4 "stream/for-each: correct element count")
  (assert-eq (get acc 0) 0 "stream/for-each: first element")
  (assert-eq (get acc 3) 3 "stream/for-each: last element"))

# stream/for-each: returns nil
(assert-eq
  (stream/for-each (fn [v] v) (make-range 3))
  nil
  "stream/for-each: returns nil")

# stream/into-array: basic
(let [[result (stream/into-array (make-from-list (list 10 20 30)))]]
  (assert-eq (length result) 3 "stream/into-array: length")
  (assert-eq (get result 0) 10 "stream/into-array: first element")
  (assert-eq (get result 2) 30 "stream/into-array: last element"))

# stream/into-array: empty source
(assert-eq
  (length (stream/into-array (make-range 0)))
  0
  "stream/into-array: empty source yields empty array")
