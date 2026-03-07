## Match Expression Tests
##
## Migrated from tests/property/matching.rs (behavioral property tests).
## Tests wildcard patterns, match in expression position, guards, and or-patterns.

(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false :assert-list-eq assert-list-eq :assert-equal assert-equal :assert-not-nil assert-not-nil :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind} ((import-file "tests/elle/assert.lisp")))

# ============================================================================
# Wildcard catches all values
# ============================================================================

(assert-eq (match 42 (_ :caught)) :caught
  "wildcard catches positive int")
(assert-eq (match -1000 (_ :caught)) :caught
  "wildcard catches negative int")
(assert-eq (match 0 (_ :caught)) :caught
  "wildcard catches zero")

# ============================================================================
# Match result in call position
# ============================================================================

(assert-eq (+ 1 (match 42 (42 42) (_ 0))) 43
  "match result in call: exact match")
(assert-eq (+ 1 (match 99 (99 99) (_ 0))) 100
  "match result in call: another exact match")
(assert-eq (+ 1 (match 7 (7 7) (_ 0))) 8
  "match result in call: small value")

# ============================================================================
# Guard sees binding
# ============================================================================

(assert-eq (match 5 (x when (> x 0) :pos) (x when (< x 0) :neg) (_ :zero)) :pos
  "guard sees binding: positive")
(assert-eq (match -3 (x when (> x 0) :pos) (x when (< x 0) :neg) (_ :zero)) :neg
  "guard sees binding: negative")
(assert-eq (match 0 (x when (> x 0) :pos) (x when (< x 0) :neg) (_ :zero)) :zero
  "guard sees binding: zero")

# ============================================================================
# Or-pattern membership
# ============================================================================

(assert-eq (match 1 ((1 | 3 | 5 | 7 | 9) :odd) ((0 | 2 | 4 | 6 | 8) :even) (_ :out)) :odd
  "or-pattern: 1 is odd")
(assert-eq (match 2 ((1 | 3 | 5 | 7 | 9) :odd) ((0 | 2 | 4 | 6 | 8) :even) (_ :out)) :even
  "or-pattern: 2 is even")
(assert-eq (match 0 ((1 | 3 | 5 | 7 | 9) :odd) ((0 | 2 | 4 | 6 | 8) :even) (_ :out)) :even
  "or-pattern: 0 is even")
(assert-eq (match 9 ((1 | 3 | 5 | 7 | 9) :odd) ((0 | 2 | 4 | 6 | 8) :even) (_ :out)) :odd
  "or-pattern: 9 is odd")
(assert-eq (match 4 ((1 | 3 | 5 | 7 | 9) :odd) ((0 | 2 | 4 | 6 | 8) :even) (_ :out)) :even
  "or-pattern: 4 is even")
