(elle/epoch 15)
# audited: 2026-10-30
# A self-recursive function that a nested lambda also names recurses as itself, whichever reference comes first.
# docs/functions.md
#
## The counter-factual: the function names itself directly, and then a lambda
## in its body names it again. A lambda that captured an empty slot in place
## of the function raised "Cannot call nil" when it ran. The same body with the
## two references swapped worked, so each shape here runs in both orders.

## 1. A tree walk. `map` takes the function as a value, and the `fold` lambda
## calls it. Each leaf comes back incremented only if the walk reached it.
(defn walk [v]
  (cond
    (array? v) (map walk v)
    (struct? v)
      (fold (fn [acc [k x]] (put acc k (walk x))) {} (pairs v))
    (+ v 1)))
(assert (= (walk {:a [1 {:b 2}]}) {:a [2 {:b 3}]})
        "a value-position self-reference before a lambda that calls the function")

(defn walk-swapped [v]
  (cond
    (struct? v)
      (fold (fn [acc [k x]] (put acc k (walk-swapped x))) {} (pairs v))
    (array? v) (map walk-swapped v)
    (+ v 1)))
(assert (= (walk-swapped {:a [1 {:b 2}]}) {:a [2 {:b 3}]})
        "a lambda that calls the function before a value-position self-reference")

## 2. A direct self-call beside a lambda that calls the function.
(defn count-down [n]
  (if (= n 0)
    0
    (+ (count-down 0) ((fn [] (+ 1 (count-down (- n 1))))))))
(assert (= (count-down 4) 4)
        "a self-call before a lambda that calls the function")

(defn count-down-swapped [n]
  (if (= n 0)
    0
    (+ ((fn [] (+ 1 (count-down-swapped (- n 1))))) (count-down-swapped 0))))
(assert (= (count-down-swapped 4) 4)
        "a lambda that calls the function before a self-call")

## 3. The call two lambdas deep.
(defn deep [n]
  (if (= n 0)
    :done
    (begin
      (deep 0)
      ((fn [] ((fn [] (deep (- n 1)))))))))
(assert (= (deep 3) :done) "a self-call before a call two lambdas deep")

## 4. A local `letrec` and a local `def`, each inside a function.
(defn local-letrec [n]
  (letrec [go (fn [m]
                (if (= m 0)
                  0
                  (+ (go 0) ((fn [] (+ m (go (- m 1))))))))]
    (go n)))
(assert (= (local-letrec 4) 10) "a local letrec named by its own nested lambda")

(defn local-def [n]
  (def go
    (fn [m]
      (if (= m 0)
        0
        (+ (go 0) ((fn [] (+ m (go (- m 1)))))))))
  (go n))
(assert (= (local-def 4) 10) "a local def named by its own nested lambda")
