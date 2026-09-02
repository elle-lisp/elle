(elle/epoch 12)
# Comparison operators — string and keyword comparison
#
# Tests for <, >, <=, >= on strings and keywords.
# Migrated from tests/integration/comparison.rs.


# ============================================================================
# String comparison — basic
# ============================================================================

# String less-than
(assert (< "a" "b") "a < b")
(assert (not (< "b" "a")) "b < a is false")
(assert (not (< "a" "a")) "a < a is false")

# String greater-than
(assert (> "b" "a") "b > a")
(assert (not (> "a" "b")) "a > b is false")
(assert (not (> "a" "a")) "a > a is false")

# String less-than-or-equal
(assert (<= "a" "b") "a <= b")
(assert (<= "a" "a") "a <= a")
(assert (not (<= "b" "a")) "b <= a is false")

# String greater-than-or-equal
(assert (>= "b" "a") "b >= a")
(assert (>= "a" "a") "a >= a")
(assert (not (>= "a" "b")) "a >= b is false")

# ============================================================================
# Keyword comparison
# ============================================================================
# Keywords order by name hash (docs/impl/symbol.md § "What the property
# buys"): the order is total and identical in every run, thread, process,
# and tier, but carries no alphabetical meaning. The counter-factual these
# assertions replace: lexicographic order needed a name lookup on every
# compare, which a sorted-container probe and the JIT tier cannot afford.

# Total and irreflexive
(assert (not (< :apple :apple)) ":apple < :apple is false")
(assert (or (< :apple :banana) (< :banana :apple)) "keyword order is total")
(assert (not (and (< :apple :banana) (< :banana :apple)))
        "keyword order is antisymmetric")

# < and > mirror each other
(assert (= (< :apple :banana) (> :banana :apple)) "< and > agree")
(assert (= (< :banana :apple) (> :apple :banana)) "< and > agree, reversed")

# Reflexive bounds
(assert (<= :apple :apple) ":apple <= :apple")
(assert (>= :apple :apple) ":apple >= :apple")
(assert (= (<= :apple :banana) (not (> :apple :banana))) "<= is not->")
(assert (= (>= :apple :banana) (not (< :apple :banana))) ">= is not-<")

# ============================================================================
# Edge cases
# ============================================================================

# Empty string comparison
(assert (< "" "a") "empty < a")
(assert (not (< "" "")) "empty < empty is false")
(assert (<= "" "") "empty <= empty")
(assert (> "a" "") "a > empty")

# Unicode string comparison
(assert (< "α" "β") "α < β (Greek letters)")

# String prefix comparison
(assert (< "abc" "abcd") "abc < abcd (prefix)")
(assert (> "abcd" "abc") "abcd > abc (prefix)")

# ============================================================================
# Mixed-type errors
# ============================================================================
# stdlib check-comparable validates cross-type comparisons.

# String vs integer error
(let [result (protect (< "a" 1))]
  (assert (not (get result 0)) "string < int should error")
  (let [err (get result 1)]
    (assert (= (get err :error) :type-error) "error kind should be :type-error")))

# String vs keyword error
(let [result (protect (< "a" :b))]
  (assert (not (get result 0)) "string < keyword should error")
  (let [err (get result 1)]
    (assert (= (get err :error) :type-error) "error kind should be :type-error")))

# Keyword vs integer error
(let [result (protect (< :a 1))]
  (assert (not (get result 0)) "keyword < int should error")
  (let [err (get result 1)]
    (assert (= (get err :error) :type-error) "error kind should be :type-error")))

# Buffer comparison: freeze to immutable strings for proper comparison
(assert (< (freeze @"a") (freeze @"b")) "frozen @string comparison works")

# ============================================================================
# Numeric comparison (preserved from existing behavior)
# ============================================================================

# Integer comparison still works
(assert (< 1 2) "1 < 2")
(assert (not (< 2 1)) "2 < 1 is false")
(assert (> 2 1) "2 > 1")
(assert (<= 1 1) "1 <= 1")
(assert (>= 1 1) "1 >= 1")

# Float comparison
(assert (< 1.5 2.5) "1.5 < 2.5")
(assert (> 2.5 1.5) "2.5 > 1.5")

# ── Mixed int/float comparison ──────────────────────────────────────────
(assert (< 3 3.3) "int < float: 3 < 3.3")
(assert (not (< 3 2.9)) "int < float: 3 < 2.9 is false")
(assert (< 2.9 3) "float < int: 2.9 < 3")
(assert (> 3.3 3) "float > int: 3.3 > 3")
(assert (> 3 2.9) "int > float: 3 > 2.9")
(assert (not (> 2.9 3)) "float > int: 2.9 > 3 is false")
(assert (<= 3 3.0) "int <= float: 3 <= 3.0")
(assert (<= 2 3.5) "int <= float: 2 <= 3.5")
(assert (not (<= 4 3.5)) "int <= float: 4 <= 3.5 is false")
(assert (>= 3.0 3) "float >= int: 3.0 >= 3")
(assert (>= 4 3.5) "int >= float: 4 >= 3.5")
(assert (not (>= 2 3.5)) "int >= float: 2 >= 3.5 is false")

# ── Mixed int/float sort ordering ──────────────────────────────────────
(assert (= (sort [1 0.5 2]) [0.5 1 2]) "sort mixed int/float")
(assert (= (sort [3 1.5 2 0.5]) [0.5 1.5 2 3]) "sort mixed int/float 4 elements")
(assert (= (compare 1 1.5) -1) "compare int < float")
(assert (= (compare 1.5 1) 1) "compare float > int")
(assert (= (compare 1 1.0) 0) "compare int = float")
