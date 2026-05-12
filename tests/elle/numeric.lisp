(elle/epoch 10)
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

# ── Integer overflow ───────────────────────────────────────────────────
# Default mode: %-intrinsics use wrapping arithmetic (WASM/SPIR-V semantics).
# --checked-intrinsics: NativeFn path uses checked_add → overflow errors.
(def checked? (vm/config :checked-intrinsics))
(if checked?
  (begin
    (def [add-ok _] (protect (+ 9223372036854775807 1)))
    (assert (not add-ok) "int add overflow errors (checked)")
    (def [sub-ok _] (protect (- -9223372036854775808 1)))
    (assert (not sub-ok) "int sub overflow errors (checked)")
    (def [mul-ok _] (protect (* 9223372036854775807 2)))
    (assert (not mul-ok) "int mul overflow errors (checked)"))
  (begin
    (assert (= (+ 9223372036854775807 1) -9223372036854775808)
            "int add overflow wraps")
    (assert (= (- -9223372036854775808 1) 9223372036854775807)
            "int sub overflow wraps")
    (assert (= (* 9223372036854775807 2) -2) "int mul overflow wraps")))

# ── NaN comparisons ─────────────────────────────────────────────────────
# Note: (= nan nan) is true in Elle (structural equality, not IEEE 754).
# Ordering comparisons correctly reject NaN.
(assert (not (< (nan) 1)) "NaN not < 1")
(assert (not (< 1 (nan))) "1 not < NaN")
(assert (not (> (nan) 1)) "NaN not > 1")

# ── even? / odd? ──────────────────────────────────────────────────────
(assert (even? 0) "even? 0")
(assert (even? 2) "even? 2")
(assert (even? -4) "even? -4")
(assert (not (even? 1)) "even? 1")
(assert (not (even? -3)) "even? -3")
(assert (odd? 1) "odd? 1")
(assert (odd? -3) "odd? -3")
(assert (not (odd? 0)) "odd? 0")
(assert (not (odd? 2)) "odd? 2")
(def [ok _] (protect (even? 1.5)))
(assert (not ok) "even? rejects float")
(def [ok _] (protect (even? "x")))
(assert (not ok) "even? rejects string")

# ── abs ───────────────────────────────────────────────────────────────
(assert (= (abs 5) 5) "abs pos int")
(assert (= (abs -5) 5) "abs neg int")
(assert (= (abs 0) 0) "abs zero")
(assert (= (abs 3.5) 3.5) "abs pos float")
(assert (= (abs -3.5) 3.5) "abs neg float")
(assert (= (abs 0.0) 0.0) "abs zero float")
(def [ok _] (protect (abs "x")))
(assert (not ok) "abs rejects string")
# abs of i64::MIN should overflow
(def [ok _] (protect (abs -9223372036854775808)))
(assert (not ok) "abs i64::MIN overflows")

# ── floor ─────────────────────────────────────────────────────────────
(assert (= (floor 3) 3) "floor int passthrough")
(assert (= (floor -3) -3) "floor neg int passthrough")
(assert (= (floor 3.0) 3) "floor exact float")
(assert (= (floor 3.7) 3) "floor pos float")
(assert (= (floor 3.2) 3) "floor pos float low")
(assert (= (floor -3.2) -4) "floor neg float")
(assert (= (floor -3.7) -4) "floor neg float high")
(assert (= (floor -3.0) -3) "floor neg exact float")
(assert (= (floor 0.5) 0) "floor 0.5")
(assert (= (floor -0.5) -1) "floor -0.5")
(def [ok _] (protect (floor "x")))
(assert (not ok) "floor rejects string")

# ── ceil ──────────────────────────────────────────────────────────────
(assert (= (ceil 3) 3) "ceil int passthrough")
(assert (= (ceil -3) -3) "ceil neg int passthrough")
(assert (= (ceil 3.0) 3) "ceil exact float")
(assert (= (ceil 3.2) 4) "ceil pos float")
(assert (= (ceil 3.7) 4) "ceil pos float high")
(assert (= (ceil -3.2) -3) "ceil neg float")
(assert (= (ceil -3.7) -3) "ceil neg float high")
(assert (= (ceil -3.0) -3) "ceil neg exact float")
(assert (= (ceil 0.5) 1) "ceil 0.5")
(assert (= (ceil -0.5) 0) "ceil -0.5")
(def [ok _] (protect (ceil "x")))
(assert (not ok) "ceil rejects string")

# ── round ─────────────────────────────────────────────────────────────
(assert (= (round 3) 3) "round int passthrough")
(assert (= (round 3.4) 3) "round down")
(assert (= (round 3.5) 4) "round half up")
(assert (= (round 3.6) 4) "round up")
(assert (= (round -3.4) -3) "round neg down")
(assert (= (round -3.5) -4) "round neg half away")
(assert (= (round -3.6) -4) "round neg up")
(assert (= (round 0.5) 1) "round 0.5")
(assert (= (round -0.5) -1) "round -0.5")
(def [ok _] (protect (round "x")))
(assert (not ok) "round rejects string")
