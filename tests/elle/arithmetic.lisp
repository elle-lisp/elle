## Arithmetic Law Tests
##
## Migrated from tests/property/arithmetic.rs
## These laws hold for all inputs; representative examples suffice.
## Tests mathematical properties like commutativity, associativity, identity,
## and distributivity with a mix of positive, negative, zero, and boundary values.

(import-file "./examples/assertions.lisp")

# ============================================================================
# add_commutative: (+ a b) == (+ b a)
# ============================================================================

(assert-eq (+ 3 5) (+ 5 3) "add_commutative: positive integers")
(assert-eq (+ -7 4) (+ 4 -7) "add_commutative: negative and positive")
(assert-eq (+ 0 99) (+ 99 0) "add_commutative: zero and positive")
(assert-eq (+ -100 -50) (+ -50 -100) "add_commutative: two negatives")

# ============================================================================
# mul_commutative: (* a b) == (* b a)
# ============================================================================

(assert-eq (* 3 7) (* 7 3) "mul_commutative: positive integers")
(assert-eq (* -4 5) (* 5 -4) "mul_commutative: negative and positive")
(assert-eq (* 0 42) (* 42 0) "mul_commutative: zero and positive")
(assert-eq (* -6 -8) (* -8 -6) "mul_commutative: two negatives")

# ============================================================================
# add_associative: (+ (+ a b) c) == (+ a (+ b c))
# ============================================================================

(assert-eq (+ (+ 1 2) 3) (+ 1 (+ 2 3)) "add_associative: positive integers")
(assert-eq (+ (+ -10 5) -3) (+ -10 (+ 5 -3)) "add_associative: mixed signs")
(assert-eq (+ (+ 0 0) 0) (+ 0 (+ 0 0)) "add_associative: all zeros")
(assert-eq (+ (+ 100 -100) 50) (+ 100 (+ -100 50)) "add_associative: cancellation")

# ============================================================================
# mul_associative: (* (* a b) c) == (* a (* b c))
# ============================================================================

(assert-eq (* (* 2 3) 4) (* 2 (* 3 4)) "mul_associative: positive integers")
(assert-eq (* (* -1 5) -3) (* -1 (* 5 -3)) "mul_associative: mixed signs")
(assert-eq (* (* 0 7) 9) (* 0 (* 7 9)) "mul_associative: zero in first position")
(assert-eq (* (* 10 -10) 5) (* 10 (* -10 5)) "mul_associative: cancellation")

# ============================================================================
# add_identity: (+ a 0) == a
# ============================================================================

(assert-eq (+ 0 0) 0 "add_identity: zero")
(assert-eq (+ 42 0) 42 "add_identity: positive")
(assert-eq (+ -99 0) -99 "add_identity: negative")
(assert-eq (+ 10000 0) 10000 "add_identity: large positive")

# ============================================================================
# mul_identity: (* a 1) == a
# ============================================================================

(assert-eq (* 0 1) 0 "mul_identity: zero")
(assert-eq (* 42 1) 42 "mul_identity: positive")
(assert-eq (* -99 1) -99 "mul_identity: negative")
(assert-eq (* 10000 1) 10000 "mul_identity: large positive")

# ============================================================================
# sub_inverse_of_add: (- (+ a b) b) == a
# ============================================================================

(assert-eq (- (+ 10 3) 3) 10 "sub_inverse_of_add: positive integers")
(assert-eq (- (+ -5 7) 7) -5 "sub_inverse_of_add: negative and positive")
(assert-eq (- (+ 0 0) 0) 0 "sub_inverse_of_add: all zeros")
(assert-eq (- (+ 100 -50) -50) 100 "sub_inverse_of_add: with negative")

# ============================================================================
# mul_zero: (* a 0) == 0
# ============================================================================

(assert-eq (* 0 0) 0 "mul_zero: zero times zero")
(assert-eq (* 1 0) 0 "mul_zero: one times zero")
(assert-eq (* -1 0) 0 "mul_zero: negative one times zero")
(assert-eq (* 42 0) 0 "mul_zero: positive times zero")
(assert-eq (* -999 0) 0 "mul_zero: large negative times zero")

# ============================================================================
# distributive: (* a (+ b c)) == (+ (* a b) (* a c))
# ============================================================================

(assert-eq (* 2 (+ 3 4)) (+ (* 2 3) (* 2 4)) "distributive: positive integers")
(assert-eq (* -1 (+ 5 -3)) (+ (* -1 5) (* -1 -3)) "distributive: negative multiplier")
(assert-eq (* 0 (+ 7 9)) (+ (* 0 7) (* 0 9)) "distributive: zero multiplier")
(assert-eq (* 10 (+ -5 3)) (+ (* 10 -5) (* 10 3)) "distributive: mixed signs in sum")

# ============================================================================
# div_inverse_of_mul: (/ (* a b) b) == a (b != 0)
# ============================================================================

(assert-eq (/ (* 6 3) 3) 6 "div_inverse_of_mul: positive integers")
(assert-eq (/ (* -10 5) 5) -10 "div_inverse_of_mul: negative numerator")
(assert-eq (/ (* 0 7) 7) 0 "div_inverse_of_mul: zero numerator")
(assert-eq (/ (* 100 -4) -4) 100 "div_inverse_of_mul: negative divisor")

# ============================================================================
# div_by_zero_is_error: division by zero signals an error
# ============================================================================

(assert-err (fn [] (/ 0 0)) "div_by_zero_is_error: zero divided by zero")
(assert-err (fn [] (/ 42 0)) "div_by_zero_is_error: positive divided by zero")
(assert-err (fn [] (/ -1 0)) "div_by_zero_is_error: negative divided by zero")

# ============================================================================
# eq_reflexive: (= a a) == true
# ============================================================================

(assert-true (= 0 0) "eq_reflexive: zero")
(assert-true (= 42 42) "eq_reflexive: positive")
(assert-true (= -99 -99) "eq_reflexive: negative")

# ============================================================================
# lt_irreflexive: (< a a) == false
# ============================================================================

(assert-false (< 0 0) "lt_irreflexive: zero")
(assert-false (< 42 42) "lt_irreflexive: positive")
(assert-false (< -99 -99) "lt_irreflexive: negative")

# ============================================================================
# lt_antisymmetric: for a != b, exactly one of (< a b) or (< b a) is true
# ============================================================================

(let ([a 3] [b 5])
  (assert-true (or (< a b) (< b a)) "lt_antisymmetric: 3 vs 5 - one is true")
  (assert-false (and (< a b) (< b a)) "lt_antisymmetric: 3 vs 5 - not both true"))

(let ([a -7] [b 4])
  (assert-true (or (< a b) (< b a)) "lt_antisymmetric: -7 vs 4 - one is true")
  (assert-false (and (< a b) (< b a)) "lt_antisymmetric: -7 vs 4 - not both true"))

(let ([a 0] [b 1])
  (assert-true (or (< a b) (< b a)) "lt_antisymmetric: 0 vs 1 - one is true")
  (assert-false (and (< a b) (< b a)) "lt_antisymmetric: 0 vs 1 - not both true"))

# ============================================================================
# mod_range: (rem a b) has |result| < b (b > 0)
# ============================================================================

(let ([result (rem 10 3)])
  (assert-true (< (abs result) 3) "mod_range: 10 rem 3 is in range"))

(let ([result (rem -10 3)])
  (assert-true (< (abs result) 3) "mod_range: -10 rem 3 is in range"))

(let ([result (rem 7 7)])
  (assert-true (< (abs result) 7) "mod_range: 7 rem 7 is in range"))

(let ([result (rem 0 5)])
  (assert-true (< (abs result) 5) "mod_range: 0 rem 5 is in range"))

# ============================================================================
# int_plus_float_is_float: (float? (+ int float)) == true
# ============================================================================

(assert-true (float? (+ 3 1.5)) "int_plus_float_is_float: 3 + 1.5")
(assert-true (float? (+ -7 0.5)) "int_plus_float_is_float: -7 + 0.5")
(assert-true (float? (+ 0 3.14)) "int_plus_float_is_float: 0 + 3.14")

# ============================================================================
# float_plus_int_is_float: (float? (+ float int)) == true
# ============================================================================

(assert-true (float? (+ 1.5 3)) "float_plus_int_is_float: 1.5 + 3")
(assert-true (float? (+ 0.5 -7)) "float_plus_int_is_float: 0.5 + -7")
(assert-true (float? (+ 3.14 0)) "float_plus_int_is_float: 3.14 + 0")
