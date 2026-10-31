(elle/epoch 15)
# audited: 2026-10-30
# A function that names its own mutable binding calls what the binding holds when the call runs.
# docs/bindings.md
#
## The counter-factual: a self-reference that named the running closure kept
## calling the first function after `assign` gave the binding a new one. Each
## case keeps the first function under a second name, reassigns the binding,
## and then calls the first function, which must reach the new value.

## 1. A top-level `var`.
(var f (fn [n] (if (= n 0) :old (f 0))))
(def first-f f)
(assign f (fn [n] :new))
(assert (= (first-f 1) :new) "a top-level var's self-call reads the var")

## 2. A `var` inside a function.
(defn local-var []
  (var g (fn [n] (if (= n 0) :old (g 0))))
  (def first-g g)
  (assign g (fn [n] :new))
  (first-g 1))
(assert (= (local-var) :new) "a local var's self-call reads the var")

## 3. An `@` binding of a `letrec`.
(defn local-letrec []
  (letrec [@go (fn [n] (if (= n 0) :old (go 0)))]
    (def first-go go)
    (assign go (fn [n] :new))
    (first-go 1)))
(assert (= (local-letrec) :new) "a mutable letrec binding's self-call reads it")

## 4. A self-reference in value position.
(var h (fn [] h))
(def first-h h)
(assign h 42)
(assert (= (first-h) 42) "a var's value-position self-reference reads the var")
