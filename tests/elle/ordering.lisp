(elle/epoch 9)
## Ordering and equality tests
##
## Tests for Eq/Hash/Ord consistency at the Elle level.
## Verifies the Buffer PartialEq fix and structural equality.


# ============================================================================
# Structural equality
# ============================================================================

# Lists
(assert (= (list 1 2 3) (list 1 2 3)) "list structural eq")
(assert (not (= (list 1 2 3) (list 1 2 4))) "list structural neq")
(assert (= (list) (list)) "empty list eq")

# Arrays
(assert (= [1 2 3] [1 2 3]) "array structural eq")
(assert (not (= [1 2 3] [1 2 4])) "array structural neq")
(assert (= [] []) "empty array eq")

# Strings
(assert (= "hello" "hello") "string eq")
(assert (not (= "hello" "world")) "string neq")
(assert (= "" "") "empty string eq")

# Structs
(assert (= {:a 1 :b 2} {:a 1 :b 2}) "struct eq")
(assert (not (= {:a 1} {:a 2})) "struct neq")

# ============================================================================
# @string equality (was broken — Buffer arm was missing from PartialEq)
# ============================================================================

(assert (= (thaw "hello") (thaw "hello")) "@string structural eq")
(assert (not (= (thaw "hello") (thaw "world"))) "@string structural neq")
(assert (= (thaw "") (thaw "")) "empty @string eq")

# ============================================================================
# Cross-type inequality
# ============================================================================

(assert (not (= 1 "1")) "int != string")
(assert (not (= nil false)) "nil != false")
(assert (not (= nil ())) "nil != empty list")
(assert (= [1 2] @[1 2]) "array = @array (cross-mutability equality)")

# ============================================================================
# NaN equality
# ============================================================================

# NaN is stored inline with TAG_NAN encoding. Inline values compare by
# raw bits, so NaN == NaN was already true before the Eq change.
# The HeapObject::Float path (via SendValue roundtrip) now also uses
# bitwise comparison, but that path is not exercisable from Elle.
(assert (= (sqrt -1) (sqrt -1)) "NaN = NaN")
(assert (identical? (sqrt -1) (sqrt -1)) "NaN identical? NaN")
