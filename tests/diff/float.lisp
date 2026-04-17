# ── Float agreement across tiers ────────────────────────────────────
#
# Covers: float constants, float arithmetic (Add/Sub/Mul/Div/Rem),
# float comparison, float negation, mixed int+float promotion,
# float returns through conditional branches, and float arguments.

(def diff ((import "tests/diff/harness")))

# ── Constant float return ──────────────────────────────────────────

(defn const-pi [x] 3.14)
(diff:assert-agree const-pi 0)
(diff:assert-agree const-pi 1)
(diff:assert-agree const-pi -42)

(defn const-neg [x] -2.5)
(diff:assert-agree const-neg 0)

# ── Float arithmetic ───────────────────────────────────────────────

(defn fadd [x] (+ 1.5 2.5))
(diff:assert-agree fadd 0)

(defn fsub [x] (- 10.0 3.5))
(diff:assert-agree fsub 0)

(defn fmul [x] (* 2.0 3.0))
(diff:assert-agree fmul 0)

(defn fdiv [x] (/ 7.0 2.0))
(diff:assert-agree fdiv 0)

(defn frem [x] (rem 7.5 2.0))
(diff:assert-agree frem 0)

# ── Float negation ─────────────────────────────────────────────────

(defn fneg [x] (- 0.0 3.14))
(diff:assert-agree fneg 0)

# ── Float comparison (result is int, not float) ───────────────────

(defn fcmp-gt [x] (if (> 3.0 2.0) 1 0))
(diff:assert-agree fcmp-gt 0)

(defn fcmp-lt [x] (if (< 1.0 2.0) 1 0))
(diff:assert-agree fcmp-lt 0)

(defn fcmp-eq [x] (if (= 2.0 2.0) 1 0))
(diff:assert-agree fcmp-eq 0)

# ── Conditional float return ──────────────────────────────────────

(defn fbranch [x] (if (> x 0) 1.0 -1.0))
(diff:assert-agree fbranch 1)
(diff:assert-agree fbranch 0)
(diff:assert-agree fbranch -1)

# ── Mixed int+float promotion ────────────────────────────────────

(defn mixed-add [x] (+ 1 2.5))
(diff:assert-agree mixed-add 0)

(defn mixed-mul [x] (* 3 1.5))
(diff:assert-agree mixed-mul 0)

# ── Float arguments ──────────────────────────────────────────────

(defn fdouble [x] (* x 2.0))
(diff:assert-agree fdouble 3.14)
(diff:assert-agree fdouble 0.0)
(diff:assert-agree fdouble -1.5)

(defn finc [x] (+ x 1.0))
(diff:assert-agree finc 2.5)
(diff:assert-agree finc -0.5)

(defn fsum [a b] (+ a b))
(diff:assert-agree fsum 1.5 2.5)
(diff:assert-agree fsum 0.0 0.0)
(diff:assert-agree fsum -1.0 1.0)

(defn fabs [x] (if (> x 0.0) x (- 0.0 x)))
(diff:assert-agree fabs 3.14)
(diff:assert-agree fabs -2.5)
(diff:assert-agree fabs 0.0)

# ── Mixed int and float arguments ───────────────────────────────

(defn mixed-arg-add [a b] (+ a b))
(diff:assert-agree mixed-arg-add 1 2.5)
(diff:assert-agree mixed-arg-add 1.5 2)

(println "float: OK")
