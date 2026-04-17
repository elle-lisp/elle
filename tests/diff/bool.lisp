# ── Boolean return agreement tests ────────────────────────────────────
#
# Verify that closures returning boolean values (from comparisons and
# bool constants) produce the same result across all compilation tiers.

(def diff ((import "tests/diff/harness")))

(diff:assert-agree (fn [x] (> x 0)) 5)
(diff:assert-agree (fn [x] (> x 0)) -1)
(diff:assert-agree (fn [x] (= x 0)) 0)
(diff:assert-agree (fn [x] (= x 0)) 1)
(diff:assert-agree (fn [x y] (< x y)) 3 7)
(diff:assert-agree (fn [x y] (< x y)) 7 3)
(diff:assert-agree (fn [x] (<= x 10)) 10)
(diff:assert-agree (fn [x] (>= x 0)) -1)

(println "bool: OK")
