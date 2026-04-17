# ── Unary op agreement across tiers ───────────────────────────────────
#
# Covers: UnaryOp::{Neg,BitNot}. UnaryOp::Not returns a bool which can't
# round-trip through MLIR's i64 path — it lives in branch.lisp instead.

(def diff ((import "std/differential")))

(defn negv [x] (- 0 x))
(diff:assert-agree negv 5)
(diff:assert-agree negv -5)
(diff:assert-agree negv 0)
(diff:assert-agree negv 1000000)

(defn bitnotv [x] (bit/not x))
(diff:assert-agree bitnotv 0)
(diff:assert-agree bitnotv -1)
(diff:assert-agree bitnotv 5)
(diff:assert-agree bitnotv 0xff)

(println "unary: OK")
