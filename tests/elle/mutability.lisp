(elle/epoch 8)
## Mutability tests — epoch 8 immutable-by-default bindings
## Positive tests only; negative tests (reject assign) stay in Rust integration tests.

## ── def @ ──────────────────────────────────────────────────────────────

(def @x 1)
(assign x 2)
(assert (= x 2) "def @ allows assign")

## ── let @ ──────────────────────────────────────────────────────────────

(let [@y 10]
  (assign y 20)
  (assert (= y 20) "let @ allows assign"))

## ── letrec @ ───────────────────────────────────────────────────────────

(letrec [@count 0
         tick (fn [] (assign count (inc count)) count)]
  (tick)
  (tick)
  (assert (= count 2) "letrec @ allows assign"))

## ── fn @ ───────────────────────────────────────────────────────────────

(defn mutate-param [@n]
  (assign n (* n 2))
  n)
(assert (= (mutate-param 5) 10) "fn @ allows assign to param")

## ── mixed @ and non-@ in same let ──────────────────────────────────────

(let [a 1
      @b 2]
  (assign b 99)
  (assert (= a 1) "non-@ binding stays immutable value")
  (assert (= b 99) "@ binding allows assign"))

## ── immutable def used as value ────────────────────────────────────────

(def pi 3)
(assert (= pi 3) "immutable def works as value")

## ── destructure @ ──────────────────────────────────────────────────────

(def [@m n] [1 2])
(assign m 10)
(assert (= (+ m n) 12) "destructure @ allows assign")
