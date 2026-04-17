# ── Capture agreement tests ───────────────────────────────────────────
#
# Verify that closures with captured numeric values produce the same
# result across all compilation tiers.

(def diff ((import "tests/diff/harness")))

# Integer capture
(let [[n 5]]
  (diff:assert-agree (fn [x] (+ x n)) 3))

# Multiple integer captures
(let [[a 2] [b 3]]
  (diff:assert-agree (fn [x] (+ (* a x) b)) 4))

# Float capture
(diff:with-tolerance 1e-10)
(let [[f 1.5]]
  (diff:assert-agree (fn [x] (+ x f)) 3))
(diff:with-tolerance nil)

# Capture + bool return (requires Part 1)
(let [[n 10]]
  (diff:assert-agree (fn [x] (< x n)) 5))

(let [[n 10]]
  (diff:assert-agree (fn [x] (< x n)) 15))

(println "capture: OK")
