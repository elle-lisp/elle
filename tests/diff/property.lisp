# ── Property-based tier agreement ─────────────────────────────────────
#
# Uses diff:prop with random integer inputs to stress-test tier
# agreement on arithmetic operations. Requires the random plugin.

# ── Check prerequisite ───────────────────────────────────────────────

(def [has-rng? rng] (protect (import "plugin/random")))
(when (not has-rng?)
  (println "SKIP property-diff: random plugin not available")
  (exit 0))

(def diff ((import "tests/diff/harness")))

# Seed for reproducibility.
(rng:seed 12345)

# ── Generators ───────────────────────────────────────────────────────

(defn gen-pair []
  [(rng:int 1000000) (rng:int 1000000)])

(defn gen-single []
  [(rng:int 1000000)])

(defn gen-nonzero-pair []
  (let [[a (rng:int 1000000)]
        [b (rng:int 1000000)]]
    [a (if (= b 0) 1 b)]))

# ── Properties ───────────────────────────────────────────────────────

(diff:prop (fn [a b] (+ a b)) gen-pair :n 100)
(diff:prop (fn [a b] (- a b)) gen-pair :n 100)
(diff:prop (fn [a b] (* a b)) gen-pair :n 100)
(diff:prop (fn [a b] (/ a b)) gen-nonzero-pair :n 100)
(diff:prop (fn [a b] (rem a b)) gen-nonzero-pair :n 100)

# Unary.
(diff:prop (fn [x] (- 0 x)) gen-single :n 50)
(diff:prop (fn [x] (bit/not x)) gen-single :n 50)

# Combined.
(diff:prop (fn [a b] (+ (* a b) (- a b))) gen-pair :n 100)

(println "property: OK")
