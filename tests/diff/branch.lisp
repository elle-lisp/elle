# ── Branch agreement across tiers ─────────────────────────────────────
#
# Covers: Compare op for each CmpOp variant, Terminator::Branch,
# multi-block control flow.

(def diff ((import "tests/diff/harness")))

(defn abs1 [x] (if (< x 0) (- 0 x) x))
(diff:assert-agree abs1 -7)
(diff:assert-agree abs1 5)
(diff:assert-agree abs1 0)
(diff:assert-agree abs1 -2147483648)

(defn maxv [a b] (if (> a b) a b))
(diff:assert-agree maxv 3 7)
(diff:assert-agree maxv 7 3)
(diff:assert-agree maxv -5 5)
(diff:assert-agree maxv 0 0)

(defn minv [a b] (if (< a b) a b))
(diff:assert-agree minv 3 7)
(diff:assert-agree minv 7 3)
(diff:assert-agree minv -5 5)

(defn sign [x]
  (if (> x 0) 1
    (if (< x 0) -1
      0)))
(diff:assert-agree sign 5)
(diff:assert-agree sign -5)
(diff:assert-agree sign 0)
(diff:assert-agree sign 1000)

# CmpOp::Eq
(defn iszero [x] (if (= x 0) 1 0))
(diff:assert-agree iszero 0)
(diff:assert-agree iszero 1)
(diff:assert-agree iszero -1)

# CmpOp::Ge
(defn nonneg [x] (if (>= x 0) 1 0))
(diff:assert-agree nonneg 0)
(diff:assert-agree nonneg -1)
(diff:assert-agree nonneg 1)

# CmpOp::Le
(defn nonpos [x] (if (<= x 0) 1 0))
(diff:assert-agree nonpos 0)
(diff:assert-agree nonpos -1)
(diff:assert-agree nonpos 1)

(println "branch: OK")
