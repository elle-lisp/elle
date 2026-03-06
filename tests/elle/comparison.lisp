# Comparison operators — string and keyword comparison
#
# Tests for <, >, <=, >= on strings and keywords.
# Migrated from tests/integration/comparison.rs.

(import-file "./examples/assertions.lisp")

# ============================================================================
# String comparison — basic
# ============================================================================

# String less-than
(assert-true (< "a" "b") "a < b")
(assert-false (< "b" "a") "b < a is false")
(assert-false (< "a" "a") "a < a is false")

# String greater-than
(assert-true (> "b" "a") "b > a")
(assert-false (> "a" "b") "a > b is false")
(assert-false (> "a" "a") "a > a is false")

# String less-than-or-equal
(assert-true (<= "a" "b") "a <= b")
(assert-true (<= "a" "a") "a <= a")
(assert-false (<= "b" "a") "b <= a is false")

# String greater-than-or-equal
(assert-true (>= "b" "a") "b >= a")
(assert-true (>= "a" "a") "a >= a")
(assert-false (>= "a" "b") "a >= b is false")

# ============================================================================
# Keyword comparison
# ============================================================================

# Keyword less-than
(assert-true (< :apple :banana) ":apple < :banana")
(assert-false (< :banana :apple) ":banana < :apple is false")
(assert-false (< :apple :apple) ":apple < :apple is false")

# Keyword greater-than
(assert-true (> :banana :apple) ":banana > :apple")
(assert-false (> :apple :banana) ":apple > :banana is false")

# Keyword less-than-or-equal
(assert-true (<= :apple :banana) ":apple <= :banana")
(assert-true (<= :apple :apple) ":apple <= :apple")

# Keyword greater-than-or-equal
(assert-true (>= :banana :apple) ":banana >= :apple")
(assert-true (>= :apple :apple) ":apple >= :apple")

# ============================================================================
# Edge cases
# ============================================================================

# Empty string comparison
(assert-true (< "" "a") "empty < a")
(assert-false (< "" "") "empty < empty is false")
(assert-true (<= "" "") "empty <= empty")
(assert-true (> "a" "") "a > empty")

# Unicode string comparison
(assert-true (< "α" "β") "α < β (Greek letters)")

# String prefix comparison
(assert-true (< "abc" "abcd") "abc < abcd (prefix)")
(assert-true (> "abcd" "abc") "abcd > abc (prefix)")

# ============================================================================
# Mixed-type errors
# ============================================================================

# String vs integer error
(let ((result (protect (< "a" 1))))
  (assert-false (get result 0) "string < int should error")
  (let ((err (get result 1)))
    (assert-eq (get err :error) :type-error "error kind should be :type-error")))

# String vs keyword error
(let ((result (protect (< "a" :b))))
  (assert-false (get result 0) "string < keyword should error")
  (let ((err (get result 1)))
    (assert-eq (get err :error) :type-error "error kind should be :type-error")))

# Keyword vs integer error
(let ((result (protect (< :a 1))))
  (assert-false (get result 0) "keyword < int should error")
  (let ((err (get result 1)))
    (assert-eq (get err :error) :type-error "error kind should be :type-error")))

# Buffer comparison error
(let ((result (protect (< @"a" @"b"))))
  (assert-false (get result 0) "buffer < buffer should error")
  (let ((err (get result 1)))
    (assert-eq (get err :error) :type-error "error kind should be :type-error")))

# ============================================================================
# Numeric comparison (preserved from existing behavior)
# ============================================================================

# Integer comparison still works
(assert-true (< 1 2) "1 < 2")
(assert-false (< 2 1) "2 < 1 is false")
(assert-true (> 2 1) "2 > 1")
(assert-true (<= 1 1) "1 <= 1")
(assert-true (>= 1 1) "1 >= 1")

# Float comparison
(assert-true (< 1.5 2.5) "1.5 < 2.5")
(assert-true (> 2.5 1.5) "2.5 > 1.5")
