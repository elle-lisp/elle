(elle/epoch 15)
# audited: 2026-10-30
# Mutually recursive bindings keep working after one of them is reassigned.
# docs/bindings.md
#
## The counter-factual: two top-level bindings whose functions name each other,
## one of them a `var` that is then reassigned. The program crashed, because the
## memory the two functions shared was released while one of them still held it.

## 1. Two `var`s; the reassigned one holds a number afterwards.
(var f (fn [] g))
(var g (fn [] f))
(assign f 7)
(assert (= f 7) "the reassigned var holds its new value")
(assert (= (g) 7) "the other function reads the reassigned var")

## 2. A `var` and a `def`; the first function, kept under a second name, calls
## the second, which calls the new value of the first.
(var ping (fn [n] (if (= n 0) :old (pong n))))
(def pong (fn [n] (ping (- n 1))))
(def first-ping ping)
(assign ping (fn [n] :new))
(assert (= (first-ping 1) :new) "the old function reaches the new value")

## 3. The same pair inside a function.
(defn local-pair []
  (var a (fn [] b))
  (var b (fn [] a))
  (assign a 7)
  (b))
(assert (= (local-pair) 7) "a local mutual pair reads the reassigned var")
