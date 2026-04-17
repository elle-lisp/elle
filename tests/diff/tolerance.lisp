# ── Float tolerance in tier agreement ───────────────────────────────
#
# Verifies that the harness's epsilon-tolerance mode works:
# - Exact float results still agree with tolerance set
# - Non-float results ignore tolerance (exact equality)
# - float-close? and values-agree? unit tests

(def diff ((import "tests/diff/harness")))

# ── float-close? unit tests ──────────────────────────────────────

(assert (diff:float-close? 1.0 1.0 1e-10)            "identical floats")
(assert (diff:float-close? 1.0 (+ 1.0 1e-12) 1e-10)  "within epsilon")
(assert (not (diff:float-close? 1.0 1.1 1e-10))       "outside epsilon")
(assert (diff:float-close? 0.0 0.0 1e-10)             "zeros")
(assert (diff:float-close? -3.14 -3.14 1e-10)         "negative identical")

# ── values-agree? unit tests ────────────────────────────────────

# Exact mode (nil epsilon): must be exactly equal.
(assert (diff:values-agree? 42 42 nil)                 "int exact match")
(assert (not (diff:values-agree? 42 43 nil))           "int exact mismatch")
(assert (diff:values-agree? 3.14 3.14 nil)             "float exact match")

# Tolerance mode: floats within epsilon agree.
(assert (diff:values-agree? 1.0 (+ 1.0 1e-12) 1e-10)  "float within eps")
(assert (not (diff:values-agree? 1.0 2.0 1e-10))       "float outside eps")

# Tolerance mode: non-floats still use exact equality.
(assert (diff:values-agree? 42 42 1e-10)               "int ignores epsilon")
(assert (not (diff:values-agree? 42 43 1e-10))          "int exact even with epsilon")

# Mixed int/float: Elle's = coerces, so 42 = 42.0 is true.
# With epsilon, non-float pairs (int vs float) use exact =.
(assert (diff:values-agree? 42 42.0 nil)                "int = float via coercion")
(assert (diff:values-agree? 42 42.0 1e-10)              "int = float with epsilon via coercion")

# ── Tolerance with assert-agree ──────────────────────────────────

# Set tolerance, run float agreement tests.
(diff:with-tolerance 1e-10)

(defn fdouble [x] (* x 2.0))
(diff:assert-agree fdouble 3.14)
(diff:assert-agree fdouble 0.0)

(defn fsum [a b] (+ a b))
(diff:assert-agree fsum 1.5 2.5)

# Int tests still work with tolerance set (tolerance ignored for ints).
(defn add [a b] (+ a b))
(diff:assert-agree add 3 4)

# Clear tolerance, verify exact mode still works.
(diff:with-tolerance nil)

(diff:assert-agree fdouble 3.14)
(diff:assert-agree add 3 4)

(println "tolerance: OK")
