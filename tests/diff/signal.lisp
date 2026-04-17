# ── Signal and silence agreement across tiers ────────────────────────
#
# Closures that use silence/muffle are still plain closures from the
# tier dispatch perspective. This test verifies that silent closures
# produce the same results on every tier.

(def diff ((import "tests/diff/harness")))

# ── Silent closures ──────────────────────────────────────────────────

# A silence-declared closure: the simplest case.
(defn silent-add [a b]
  (silence)
  (muffle :error)
  (+ a b))

(diff:assert-agree silent-add 3 7)
(diff:assert-agree silent-add -10 20)
(diff:assert-agree silent-add 0 0)

# Silent branching.
(defn silent-abs [x]
  (silence)
  (muffle :error)
  (if (< x 0) (- 0 x) x))

(diff:assert-agree silent-abs -7)
(diff:assert-agree silent-abs 5)
(diff:assert-agree silent-abs 0)

# Silent multi-op.
(defn silent-poly [a b]
  (silence)
  (muffle :error)
  (+ (* a a) (* b b)))

(diff:assert-agree silent-poly 3 4)
(diff:assert-agree silent-poly 0 0)
(diff:assert-agree silent-poly -5 12)

# Silent with locals.
(defn silent-let [x]
  (silence)
  (muffle :error)
  (let [[y (* x x)]]
    (+ y x)))

(diff:assert-agree silent-let 5)
(diff:assert-agree silent-let -3)
(diff:assert-agree silent-let 0)

(println "signal: OK")
