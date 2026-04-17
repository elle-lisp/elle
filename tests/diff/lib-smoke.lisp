# ── lib/differential smoke test ───────────────────────────────────────

(def diff ((import "tests/diff/harness")))

# Reference closure for the rest of the test.
(defn add [a b] (+ a b))

# call returns a struct.
(def report (diff:call add 3 4))
(assert (= (get report :agreed) true) "agreed")
(assert (= (get report :result) 7)    "result = 7")

# eligible-tiers includes :bytecode at minimum.
(def tiers (diff:eligible-tiers add 1 2))
(assert (contains? tiers :bytecode) "bytecode is always eligible")
(assert (contains? tiers :jit)      "jit is eligible for plain arithmetic")

# assert-agree returns the agreed value.
(assert (= (diff:assert-agree add 100 -50) 50) "assert-agree returns value")

# Branching closure.
(defn abs1 [x] (if (< x 0) (- 0 x) x))
(assert (= (diff:assert-agree abs1 -7) 7)  "abs1(-7)")
(assert (= (diff:assert-agree abs1 0)  0)  "abs1(0)")
(assert (= (diff:assert-agree abs1 42) 42) "abs1(42)")

# Multi-block closure with let.
(defn poly [x] (let [[s (* x x)]] (+ s 1)))
(assert (= (diff:assert-agree poly 5)  26)  "poly(5)")
(assert (= (diff:assert-agree poly -3) 10)  "poly(-3)")

(println "lib/differential smoke test OK")
