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

# Keyword less-than
(assert (< :apple :banana) ":apple < :banana")
(assert (not (< :banana :apple)) ":banana < :apple is false")
(assert (not (< :apple :apple)) ":apple < :apple is false")

# Keyword greater-than
(assert (> :banana :apple) ":banana > :apple")
(assert (not (> :apple :banana)) ":apple > :banana is false")

# Keyword less-than-or-equal
(assert (<= :apple :banana) ":apple <= :banana")
(assert (<= :apple :apple) ":apple <= :apple")

# Keyword greater-than-or-equal
(assert (>= :banana :apple) ":banana >= :apple")
(assert (>= :apple :apple) ":apple >= :apple")

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

# String vs integer error
(let ((result (protect (< "a" 1))))
  (assert (not (get result 0)) "string < int should error")
  (let ((err (get result 1)))
    (assert (= (get err :error) :type-error) "error kind should be :type-error")))

# String vs keyword error
(let ((result (protect (< "a" :b))))
  (assert (not (get result 0)) "string < keyword should error")
  (let ((err (get result 1)))
    (assert (= (get err :error) :type-error) "error kind should be :type-error")))

# Keyword vs integer error
(let ((result (protect (< :a 1))))
  (assert (not (get result 0)) "keyword < int should error")
  (let ((err (get result 1)))
    (assert (= (get err :error) :type-error) "error kind should be :type-error")))

# Buffer comparison error
(let ((result (protect (< @"a" @"b"))))
  (assert (not (get result 0)) "buffer < buffer should error")
  (let ((err (get result 1)))
    (assert (= (get err :error) :type-error) "error kind should be :type-error")))

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
