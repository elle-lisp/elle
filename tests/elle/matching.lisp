(elle/epoch 1)
## Match Expression Tests
##
## Migrated from tests/property/matching.rs (behavioral property tests).
## Tests wildcard patterns, match in expression position, guards, and or-patterns.


# ============================================================================
# Wildcard catches all values
# ============================================================================

(assert (= (match 42 (_ :caught)) :caught) "wildcard catches positive int")
(assert (= (match -1000 (_ :caught)) :caught) "wildcard catches negative int")
(assert (= (match 0 (_ :caught)) :caught) "wildcard catches zero")

# ============================================================================
# Match result in call position
# ============================================================================

(assert (= (+ 1 (match 42 (42 42) (_ 0))) 43) "match result in call: exact match")
(assert (= (+ 1 (match 99 (99 99) (_ 0))) 100) "match result in call: another exact match")
(assert (= (+ 1 (match 7 (7 7) (_ 0))) 8) "match result in call: small value")

# ============================================================================
# Guard sees binding
# ============================================================================

(assert (= (match 5 (x when (> x 0) :pos) (x when (< x 0) :neg) (_ :zero)) :pos) "guard sees binding: positive")
(assert (= (match -3 (x when (> x 0) :pos) (x when (< x 0) :neg) (_ :zero)) :neg) "guard sees binding: negative")
(assert (= (match 0 (x when (> x 0) :pos) (x when (< x 0) :neg) (_ :zero)) :zero) "guard sees binding: zero")

# ============================================================================
# Or-pattern membership
# ============================================================================

(assert (= (match 1 ((or 1 3 5 7 9) :odd) ((or 0 2 4 6 8) :even) (_ :out)) :odd) "or-pattern: 1 is odd")
(assert (= (match 2 ((or 1 3 5 7 9) :odd) ((or 0 2 4 6 8) :even) (_ :out)) :even) "or-pattern: 2 is even")
(assert (= (match 0 ((or 1 3 5 7 9) :odd) ((or 0 2 4 6 8) :even) (_ :out)) :even) "or-pattern: 0 is even")
(assert (= (match 9 ((or 1 3 5 7 9) :odd) ((or 0 2 4 6 8) :even) (_ :out)) :odd) "or-pattern: 9 is odd")
(assert (= (match 4 ((or 1 3 5 7 9) :odd) ((or 0 2 4 6 8) :even) (_ :out)) :even) "or-pattern: 4 is even")
