## Tests for primitives migrated from Rust to Elle stdlib.
## Covers: drop, range

## ── drop ────────────────────────────────────────────────────────────

(assert (= (drop 0 (list 1 2 3)) (list 1 2 3)) "drop 0: identity")
(assert (= (drop 1 (list 1 2 3)) (list 2 3)) "drop 1")
(assert (= (drop 2 (list 1 2 3 4 5)) (list 3 4 5)) "drop 2")
(assert (= (drop 3 (list 1 2 3)) ()) "drop all")
(assert (= (drop 5 (list 1 2 3)) ()) "drop more than length")
(assert (= (drop 0 ()) ()) "drop 0 from empty")
(assert (= (drop 5 ()) ()) "drop n from empty")

# error: negative count
(let (([ok? err] (protect ((fn [] (drop -1 (list 1 2)))))))
  (assert (not ok?) "drop negative: errors")
  (assert (= (get err :error) :argument-error) "drop negative: argument-error"))

## ── range ───────────────────────────────────────────────────────────

# (range end)
(let ((r (range 5)))
  (assert (array? r) "range 5: is array")
  (assert (mutable? r) "range 5: is mutable")
  (assert (= (length r) 5) "range 5: length")
  (assert (= (get r 0) 0) "range 5: first")
  (assert (= (get r 4) 4) "range 5: last"))

# (range start end)
(let ((r (range 2 5)))
  (assert (= (length r) 3) "range 2 5: length")
  (assert (= (get r 0) 2) "range 2 5: first")
  (assert (= (get r 2) 4) "range 2 5: last"))

# (range start end step)
(let ((r (range 0 10 3)))
  (assert (= (length r) 4) "range step: length")
  (assert (= (get r 0) 0) "range step: first")
  (assert (= (get r 1) 3) "range step: second")
  (assert (= (get r 3) 9) "range step: last"))

# negative step
(let ((r (range 5 0 -1)))
  (assert (= (length r) 5) "range neg step: length")
  (assert (= (get r 0) 5) "range neg step: first")
  (assert (= (get r 4) 1) "range neg step: last"))

# empty ranges
(assert (= (length (range 0)) 0) "range 0: empty")
(assert (= (length (range 5 5)) 0) "range start=end: empty")
(assert (= (length (range 5 3)) 0) "range start>end no step: empty")

# range with float step
(let ((r (range 0 1.0 0.5)))
  (assert (= (length r) 2) "range float step: length")
  (assert (= (get r 0) 0) "range float step: first")
  (assert (= (get r 1) 0.5) "range float step: second"))

# error: step zero
(let (([ok? err] (protect ((fn [] (range 0 10 0))))))
  (assert (not ok?) "range step=0: errors")
  (assert (= (get err :error) :argument-error) "range step=0: argument-error"))

## ── verify primitives still work (not migrated, but adjacent) ───────

# reverse, last, butlast, take stay native — quick sanity checks
(assert (= (reverse (list 1 2 3)) (list 3 2 1)) "reverse: list")
(assert (= (reverse [1 2 3]) [3 2 1]) "reverse: array")
(assert (= (last (list 1 2 3)) 3) "last: list")
(assert (= (last [1 2 3]) 3) "last: array")
(assert (= (butlast (list 1 2 3)) (list 1 2)) "butlast: list")
(assert (= (take 2 (list 1 2 3)) (list 1 2)) "take: list")

## ── bytes idempotency (bugfix) ─────────────────────────────────────────

(let [[b (bytes "hello")]]
  (assert (identical? (bytes b) b) "bytes: idempotent on bytes")
  (assert (= (bytes b) b) "bytes: equal on bytes"))
(let [[mb (@bytes 1 2 3)]]
  (assert (identical? (@bytes mb) mb) "@bytes: idempotent on @bytes"))
(assert (identical? (bytes (bytes 1 2 3)) (bytes 1 2 3)) "bytes: nested idempotent")

(println "migrated: all tests passed")
