(elle/epoch 13)
# audited: 2026-09-29
# Guard for the one-region rest list: a tail of the list outlives the list's head.
# docs/impl/region/colocation.md
#
# A variadic callee collects its rest arguments into one region, so every cons of
# the list shares one count. A tail handed out of the callee keeps that region,
# and the whole list with it, for as long as the tail lives. Each shape hands a
# tail out, drops the head, churns region ids and reads the tail. A region freed
# early faults under --trace=guardfree.

(defn churn []
  (var i 0)
  (while (< i 300)
    (pair (string "c" i) i)
    (assign i (+ i 1))))

(defn tail-of [& xs]
  (rest xs))
(defn third-tail [& xs]
  (rest (rest (rest xs))))

# Tails stored into a container that outlives the call.
(def kept @[])
(push kept (tail-of 1 2 3 4))
(push kept (third-tail (string "a") (string "b") (string "c") (string "d")))
(churn)
(assert (= (first (get kept 0)) 2) "a stored tail outlives the list's head")
(assert (= (first (get kept 1)) "d") "and the heap value its cons holds")

# A global bound to a tail.
(def held (tail-of [1 2] [3 4] [5 6]))
(churn)
(assert (= (get (first held) 0) 3) "a bound tail outlives the list's head")

# `apply` in tail position hands the list to another variadic callee.
(defn inner [& ys]
  ys)
(defn outer [& xs]
  (apply inner xs))
(def applied (outer (string "p") (string "q")))
(churn)
(assert (= (first (rest applied)) "q") "an applied list survives the hand-off")

# A closure over a tail.
(defn capture-tail [& xs]
  (let [t (rest xs)]
    (fn [] (first t))))
(def reader (capture-tail 1 (string "second") 3))
(churn)
(assert (= (reader) "second") "a captured tail outlives the list's head")

# A variadic call in a loop: each list frees whole, and the next reuses its ids.
(var total 0)
(var round 0)
(while (< round 200)
  (assign total (+ total (first (tail-of round (+ round 1) (+ round 2)))))
  (assign round (+ round 1)))
(assert (= total 20100) "every round read its own list")

(println "region-rest-list-uaf: ok")
