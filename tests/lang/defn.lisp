(elle/epoch 14)
# audited: 2026-09-30
# defn binds a name to a function that answers its last body form, and a defn in a body is visible to the forms after it.
# docs/functions.md
# docs/bindings.md

(defn subtract [x y]
  (- x y))
(def subtract-fn (fn [x y] (- x y)))
(assert (= (subtract 10 3) 7) "defn binds a function")
(assert (= (subtract 10 3) (subtract-fn 10 3)) "defn is def of a fn")

(defn last-form [x]
  (+ x 1)
  (* x 2))
(assert (= (last-form 5) 10) "a function answers its last body form")

(defn fact [n]
  (if (= n 0)
    1
    (* n (fact (- n 1)))))
(assert (= (fact 0) 1) "a defn calls itself by its own name: base case")
(assert (= (fact 5) 120) "a defn calls itself by its own name: recursion")

(defn outer [x]
  (defn inner [y]
    (+ y x))
  (inner 5))
(assert (= (outer 10) 15)
        "a defn in a body is visible after it and captures the parameter")
