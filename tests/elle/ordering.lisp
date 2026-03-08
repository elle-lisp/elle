## Ordering and equality tests
##
## Tests for Eq/Hash/Ord consistency at the Elle level.
## Verifies the Buffer PartialEq fix and structural equality.

(import-file "tests/elle/assert.lisp")

# ============================================================================
# Structural equality
# ============================================================================

# Lists
(assert-true (= (list 1 2 3) (list 1 2 3)) "list structural eq")
(assert-false (= (list 1 2 3) (list 1 2 4)) "list structural neq")
(assert-true (= (list) (list)) "empty list eq")

# Tuples
(assert-true (= [1 2 3] [1 2 3]) "tuple structural eq")
(assert-false (= [1 2 3] [1 2 4]) "tuple structural neq")
(assert-true (= [] []) "empty tuple eq")

# Strings
(assert-true (= "hello" "hello") "string eq")
(assert-false (= "hello" "world") "string neq")
(assert-true (= "" "") "empty string eq")

# Structs
(assert-true (= {:a 1 :b 2} {:a 1 :b 2}) "struct eq")
(assert-false (= {:a 1} {:a 2}) "struct neq")

# ============================================================================
# Buffer equality (was broken — Buffer arm was missing from PartialEq)
# ============================================================================

(assert-true (= @"hello" @"hello") "buffer structural eq")
(assert-false (= @"hello" @"world") "buffer structural neq")
(assert-true (= @"" @"") "empty buffer eq")

# ============================================================================
# Cross-type inequality
# ============================================================================

(assert-false (= 1 "1") "int != string")
(assert-false (= nil false) "nil != false")
(assert-false (= nil ()) "nil != empty list")
(assert-false (= [1 2] @[1 2]) "tuple != array")

# ============================================================================
# NaN equality
# ============================================================================

# NaN is stored inline with TAG_NAN encoding. Inline values compare by
# raw bits, so NaN == NaN was already true before the Eq change.
# The HeapObject::Float path (via SendValue roundtrip) now also uses
# bitwise comparison, but that path is not exercisable from Elle.
(assert-true (= (sqrt -1) (sqrt -1)) "NaN = NaN")
(assert-true (identical? (sqrt -1) (sqrt -1)) "NaN identical? NaN")
