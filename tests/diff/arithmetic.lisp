# ── Arithmetic agreement across tiers ─────────────────────────────────
#
# Covers: BinOp::{Add,Sub,Mul,Div,Rem} in MLIR + JIT + bytecode.

(def diff ((import "tests/diff/harness")))

(defn add [a b] (+ a b))
(defn sub [a b] (- a b))
(defn mul [a b] (* a b))
(defn divv [a b] (/ a b))
(defn modv [a b] (rem a b))

(diff:assert-agree add 3 4)
(diff:assert-agree add -10 30)
(diff:assert-agree add 0 0)
(diff:assert-agree add 1000000 -999999)

(diff:assert-agree sub 10 3)
(diff:assert-agree sub -10 3)
(diff:assert-agree sub 0 -42)

(diff:assert-agree mul 3 7)
(diff:assert-agree mul -4 5)
(diff:assert-agree mul 0 100)
(diff:assert-agree mul -6 -8)

(diff:assert-agree divv 100 5)
(diff:assert-agree divv -100 5)
(diff:assert-agree divv 7 2)

(diff:assert-agree modv 17 5)
(diff:assert-agree modv 100 7)

# Combined.
(defn poly [a b] (+ (* a b) a))
(diff:assert-agree poly 3 7)
(diff:assert-agree poly -2 4)
(diff:assert-agree poly 0 99)

(println "arithmetic: OK")
