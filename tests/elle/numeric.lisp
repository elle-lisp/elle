# Numeric correctness tests
#
# Mixed int/float arithmetic, comparisons, overflow handling,
# IEEE 754 special values, and hash consistency.

# ── Mixed int/float arithmetic ──────────────────────────────────────────
(assert (= (+ 1 0.5) 1.5) "+ int float")
(assert (= (- 1 0.5) 0.5) "- int float")
(assert (= (* 2 0.5) 1.0) "* int float")
(assert (= (/ 1.0 2) 0.5) "/ float int")
(assert (= (/ 7.0 2) 3.5) "/ float int non-even")

# ── Integer division truncates ──────────────────────────────────────────
(assert (= (/ 5 3) 1) "int div truncates")
(assert (= (/ -5 3) -1) "int div truncates negative")

# ── Float display preserves type ────────────────────────────────────────
(assert (= (string 3.0) "3.0") "float string keeps .0")
(assert (= (string 3.14) "3.14") "float string keeps decimals")
(assert (= (string 0.0) "0.0") "zero float string")

# ── IEEE 754 division ──────────────────────────────────────────────────
(assert (inf? (/ 1.0 0.0)) "float/0 = inf")
(assert (inf? (/ 1.0 0)) "float/int0 = inf")
(assert (inf? (/ -1.0 0.0)) "-float/0 = -inf")
(assert (nan? (/ 0.0 0.0)) "0.0/0.0 = NaN")
(def [ok _] (protect (/ 1 0)))
(assert (not ok) "int/0 errors")

# ── IEEE 754 constants ──────────────────────────────────────────────────
(assert (inf? (+inf)) "+inf is infinite")
(assert (inf? (-inf)) "-inf is infinite")
(assert (nan? (nan)) "nan is NaN")
(assert (= (string (+inf)) "inf") "+inf displays as inf")
(assert (= (string (-inf)) "-inf") "-inf displays as -inf")
(assert (= (string (nan)) "NaN") "nan displays as NaN")

# ── min / max mixed ────────────────────────────────────────────────────
(assert (= (min 3 2.5) 2.5) "min int float")
(assert (= (max 3 2.5) 3) "max int float")
(assert (= (min 0.5 1 2) 0.5) "min variadic mixed")
(assert (= (max 0.5 1 2) 2) "max variadic mixed")

# ── Hash consistency ────────────────────────────────────────────────────
(assert (= (hash 1) (hash 1.0)) "hash 1 = hash 1.0")
(assert (= (hash 0) (hash 0.0)) "hash 0 = hash 0.0")
(assert (= (hash -1) (hash -1.0)) "hash -1 = hash -1.0")

# ── Sort mixed ──────────────────────────────────────────────────────────
(assert (= (sort [2 0.5 1 1.5]) [0.5 1 1.5 2]) "sort mixed")

# ── pow mixed ───────────────────────────────────────────────────────────
(assert (= (pow 2 -1) 0.5) "pow int neg exp")
(assert (= (pow 2.0 -1) 0.5) "pow float neg exp")
(assert (= (pow 2.0 3) 8.0) "pow float int")
(assert (= (pow 0 0) 1) "pow 0 0")

# ── Integer overflow signals error ──────────────────────────────────────
(def [add-ok _] (protect (+ 9223372036854775807 1)))
(assert (not add-ok) "int add overflow errors")

(def [sub-ok _] (protect (- -9223372036854775808 1)))
(assert (not sub-ok) "int sub overflow errors")

(def [mul-ok _] (protect (* 9223372036854775807 2)))
(assert (not mul-ok) "int mul overflow errors")

# ── NaN comparisons ─────────────────────────────────────────────────────
# Note: (= nan nan) is true in Elle (structural equality, not IEEE 754).
# Ordering comparisons correctly reject NaN.
(assert (not (< (nan) 1)) "NaN not < 1")
(assert (not (< 1 (nan))) "1 not < NaN")
(assert (not (> (nan) 1)) "NaN not > 1")
