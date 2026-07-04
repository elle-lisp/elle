(elle/epoch 12)
## tests/elle/recur-as-value.lisp
##
## A self-recursive local function is usually CALLED by name, but the same name
## can also appear in VALUE position — returned from its defining scope, passed
## to a higher-order function, or stored in a container and invoked later. In
## every such position the value materialized for the self-reference must be the
## function itself, so that invoking it recurses correctly.
##
## The hazard is silent: if the value materialized for a self-reference were the
## wrong closure (or carried the wrong captured environment), the later
## invocation would recurse as something else and return a plausible wrong
## value — no leak, no use-after-free, so only these value assertions catch it.

## 1. Returned as a value, then invoked. `go` escapes its defining `letrec` as
## the return value; calling the returned closure must run `go`'s recursion.
(defn make-countup []
  (letrec [go (fn [m]
                (if (%lt m 1)
                  0
                  (%add 1 (go (%sub m 1)))))]
    go))
(def f (make-countup))
(assert (= (f 7) 7)
        (concat "returned self-recursive closure must count to 7, got "
                (string (f 7))))
(assert (= (f 0) 0) "returned closure base case")

## 2. Handed to a higher-order function as a value, which invokes it. `go` is a
## VALUE argument to the HOF here while it is also self-CALLED inside its own
## body — both positions of the same self-reference exercised at once.
(defn via-hof [n]
  (letrec [go (fn [m] (if (%lt m 1) :base (go (%sub m 1))))]
    ((fn [h x] (h x)) go n)))
(assert (= (via-hof 5) :base)
        "self-recursive closure handed to a HOF as a value")

## 3. Stored in a container as a value, retrieved, then invoked. The closure put
## into the struct must be the one whose recursion accumulates correctly.
(defn via-struct [n]
  (letrec [go (fn [m acc] (if (%lt m 1) acc (go (%sub m 1) (%add acc 1))))]
    (let [s {:fn go}]
      ((get s :fn) n 0))))
(assert (= (via-struct 9) 9)
        (concat "stored self-recursive closure must count to 9, got "
                (string (via-struct 9))))

## 4. Two distinct self-recursive closures of the same shape, each carrying its
## own captured increment, handed off as values and invoked. A value-position
## self-reference that materialized a shared/stale closure would cross-wire the
## two captured increments and produce a wrong total.
(defn stepper [inc]
  (letrec [go (fn [m acc] (if (%lt m 1) acc (go (%sub m 1) (%add acc inc))))]
    go))
(def s2 (stepper 2))
(def s5 (stepper 5))
(assert (= (s2 4 0) 8)
        (concat "stepper inc=2 over 4 must be 8, got " (string (s2 4 0))))
(assert (= (s5 4 0) 20)
        (concat "stepper inc=5 over 4 must be 20, got " (string (s5 4 0))))
(assert (= (%add (s2 3 0) (s5 3 0)) 21)
        "interleaved invocations of two distinct self-recursive closures must keep separate captured state")

(println "recur-as-value: ok")
