(elle/epoch 9)
## Flat let bindings
##
## Exercises flat (Clojure-style) binding syntax for let, letrec,
## if-let, and when-let. All binding pairs are laid out flat inside a
## single bracket form: [name1 value1 name2 value2 ...].
##
## let is sequential (like Clojure): each binding sees previous ones.
## let* is kept as an alias.

## ── single binding ──────────────────────────────────────────────────
(assert (= (let [x 1]
             x) 1) "flat let single binding")

## ── multiple bindings ───────────────────────────────────────────────
(assert (= (let [a 1
                 b 2]
             (+ a b)) 3) "flat let multi binding")

## ── destructuring ───────────────────────────────────────────────────
(assert (= (let [[x y] [3 4]]
             (+ x y)) 7) "flat let destructuring")

## ── let is sequential (Clojure-style) ───────────────────────────────
(assert (= (let [a 1
                 b (+ a 1)]
             b) 2) "let sequential pair")

(assert (= (let [x 5
                 y (* x 2)
                 z (+ x y)]
             z) 15) "let sequential triple")

## ── let with destructuring + sequential ─────────────────────────────
(assert (= (let [[a b] [10 20]
                 c (+ a b)]
             c) 30) "let destructure + sequential binding")

## ── let* still works (alias) ────────────────────────────────────────
(assert (= (let* [a 1
                  b (+ a 1)]
             b) 2) "let* still works")

## ── letrec mutual recursion ────────────────────────────────────────
(letrec [is-even (fn [n] (if (= n 0) true (is-odd (- n 1))))
         is-odd (fn [n] (if (= n 0) false (is-even (- n 1))))]
  (assert (is-even 4) "flat letrec mutual recursion even")
  (assert (not (is-even 3)) "flat letrec mutual recursion odd"))

## ── if-let ──────────────────────────────────────────────────────────
(assert (= (if-let [x 42] x :nope) 42) "flat if-let truthy")
(assert (= (if-let [x nil] x :nope) :nope) "flat if-let falsy")

## ── when-let ────────────────────────────────────────────────────────
(assert (= (when-let [x 10] (+ x 1)) 11) "flat when-let truthy")
(assert (nil? (when-let [x nil] (+ x 1))) "flat when-let falsy")

## ── nested let ──────────────────────────────────────────────────────
(assert (= (let [x 1]
             (let [y (+ x 1)]
               (+ x y))) 3) "flat nested let")

(println "flatlet: all passed")
