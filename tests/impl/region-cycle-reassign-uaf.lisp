(elle/epoch 15)
# audited: 2026-10-30
# A cycle of top-level functions stays live and callable after one member is reassigned.
# docs/impl/region/letrec.md
#
# The sidecar arms guardfree.
#
# A top-level `var` keeps a static-slot cell although it is reassigned, so the
# closure-cycle merge admitted a cycle whose member's cell an `assign` later
# repoints. The arena then took two releases and was freed while the other
# member's cell still held a closure in it: a panic in a release build, and a
# SIGSEGV under guardfree. Heap churn between the calls recycles a freed page,
# so a stale read is loud.

(var ping (fn [n] (if (= n 0) :old (pong n))))
(def pong (fn [n] (ping (- n 1))))
(def first-ping ping)
(assign ping (fn [n] :new))

(var f (fn [] g))
(var g (fn [] f))
(assign f 7)

(defn churn [k]
  (def @acc @[])
  (def @i 0)
  (while (< i k)
    (push acc (string "churn-" i))
    (assign i (+ i 1)))
  (length acc))

(def @n 0)
(while (< n 200)
  (churn 50)
  (assert (= (first-ping 1) :new) "the old member reaches the new value")
  (assert (= (pong 1) :new) "the other member calls the new value")
  (assert (= (g) 7) "the pair's other member reads the reassigned var")
  (assign n (+ n 1)))
