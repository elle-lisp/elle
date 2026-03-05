## Match Expression Tests
##
## Migrated from tests/property/matching.rs
## Tests match compilation: wildcard, expression position, guards, or-patterns.

(import-file "./examples/assertions.lisp")

# ============================================================================
# Wildcard catches all values
# ============================================================================

(assert-eq (match 0 (_ :caught)) :caught "wildcard: zero")
(assert-eq (match 42 (_ :caught)) :caught "wildcard: positive")
(assert-eq (match -999 (_ :caught)) :caught "wildcard: negative")
(assert-eq (match true (_ :caught)) :caught "wildcard: bool")
(assert-eq (match "hello" (_ :caught)) :caught "wildcard: string")
(assert-eq (match nil (_ :caught)) :caught "wildcard: nil")

# ============================================================================
# Match result in call (expression position)
# ============================================================================

(assert-eq (+ 1 (match 5 (5 5) (_ 0))) 6 "result-in-call: exact match")
(assert-eq (+ 1 (match 0 (0 0) (_ 0))) 1 "result-in-call: zero match")
(assert-eq (+ 1 (match 99 (99 99) (_ 0))) 100 "result-in-call: large match")
(assert-eq (+ 1 (match 7 (5 5) (_ 0))) 1 "result-in-call: fallthrough to wildcard")

# ============================================================================
# Guards see bindings from the pattern
# ============================================================================

(assert-eq (match 5 (x when (> x 0) :pos) (x when (< x 0) :neg) (_ :zero))
           :pos "guard: positive")
(assert-eq (match -3 (x when (> x 0) :pos) (x when (< x 0) :neg) (_ :zero))
           :neg "guard: negative")
(assert-eq (match 0 (x when (> x 0) :pos) (x when (< x 0) :neg) (_ :zero))
           :zero "guard: zero")
(assert-eq (match 100 (x when (> x 0) :pos) (x when (< x 0) :neg) (_ :zero))
           :pos "guard: large positive")
(assert-eq (match -100 (x when (> x 0) :pos) (x when (< x 0) :neg) (_ :zero))
           :neg "guard: large negative")

# ============================================================================
# Or-patterns match any alternative
# ============================================================================

(assert-eq (match 1 ((1 | 3 | 5 | 7 | 9) :odd) ((0 | 2 | 4 | 6 | 8) :even) (_ :out))
           :odd "or-pattern: 1 is odd")
(assert-eq (match 4 ((1 | 3 | 5 | 7 | 9) :odd) ((0 | 2 | 4 | 6 | 8) :even) (_ :out))
           :even "or-pattern: 4 is even")
(assert-eq (match 0 ((1 | 3 | 5 | 7 | 9) :odd) ((0 | 2 | 4 | 6 | 8) :even) (_ :out))
           :even "or-pattern: 0 is even")
(assert-eq (match 9 ((1 | 3 | 5 | 7 | 9) :odd) ((0 | 2 | 4 | 6 | 8) :even) (_ :out))
           :odd "or-pattern: 9 is odd")
(assert-eq (match 10 ((1 | 3 | 5 | 7 | 9) :odd) ((0 | 2 | 4 | 6 | 8) :even) (_ :out))
           :out "or-pattern: 10 is out of range")
