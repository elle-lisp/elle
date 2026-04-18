(elle/epoch 8)
## Arithmetic Law Tests
##
## Migrated from tests/property/arithmetic.rs
## These laws hold for all inputs; representative examples suffice.
## Tests mathematical properties like commutativity, associativity, identity,
## and distributivity with a mix of positive, negative, zero, and boundary values.


# ============================================================================
# add_commutative: (+ a b) == (+ b a)
# ============================================================================

(assert (= (+ 3 5) (+ 5 3)) "add_commutative: positive integers")
(assert (= (+ -7 4) (+ 4 -7)) "add_commutative: negative and positive")
(assert (= (+ 0 99) (+ 99 0)) "add_commutative: zero and positive")
(assert (= (+ -100 -50) (+ -50 -100)) "add_commutative: two negatives")

# ============================================================================
# mul_commutative: (* a b) == (* b a)
# ============================================================================

(assert (= (* 3 7) (* 7 3)) "mul_commutative: positive integers")
(assert (= (* -4 5) (* 5 -4)) "mul_commutative: negative and positive")
(assert (= (* 0 42) (* 42 0)) "mul_commutative: zero and positive")
(assert (= (* -6 -8) (* -8 -6)) "mul_commutative: two negatives")

# ============================================================================
# add_associative: (+ (+ a b) c) == (+ a (+ b c))
# ============================================================================

(assert (= (+ (+ 1 2) 3) (+ 1 (+ 2 3))) "add_associative: positive integers")
(assert (= (+ (+ -10 5) -3) (+ -10 (+ 5 -3))) "add_associative: mixed signs")
(assert (= (+ (+ 0 0) 0) (+ 0 (+ 0 0))) "add_associative: all zeros")
(assert (= (+ (+ 100 -100) 50) (+ 100 (+ -100 50))) "add_associative: cancellation")

# ============================================================================
# mul_associative: (* (* a b) c) == (* a (* b c))
# ============================================================================

(assert (= (* (* 2 3) 4) (* 2 (* 3 4))) "mul_associative: positive integers")
(assert (= (* (* -1 5) -3) (* -1 (* 5 -3))) "mul_associative: mixed signs")
(assert (= (* (* 0 7) 9) (* 0 (* 7 9))) "mul_associative: zero in first position")
(assert (= (* (* 10 -10) 5) (* 10 (* -10 5))) "mul_associative: cancellation")

# ============================================================================
# add_identity: (+ a 0) == a
# ============================================================================

(assert (= (+ 0 0) 0) "add_identity: zero")
(assert (= (+ 42 0) 42) "add_identity: positive")
(assert (= (+ -99 0) -99) "add_identity: negative")
(assert (= (+ 10000 0) 10000) "add_identity: large positive")

# ============================================================================
# mul_identity: (* a 1) == a
# ============================================================================

(assert (= (* 0 1) 0) "mul_identity: zero")
(assert (= (* 42 1) 42) "mul_identity: positive")
(assert (= (* -99 1) -99) "mul_identity: negative")
(assert (= (* 10000 1) 10000) "mul_identity: large positive")

# ============================================================================
# sub_inverse_of_add: (- (+ a b) b) == a
# ============================================================================

(assert (= (- (+ 10 3) 3) 10) "sub_inverse_of_add: positive integers")
(assert (= (- (+ -5 7) 7) -5) "sub_inverse_of_add: negative and positive")
(assert (= (- (+ 0 0) 0) 0) "sub_inverse_of_add: all zeros")
(assert (= (- (+ 100 -50) -50) 100) "sub_inverse_of_add: with negative")

# ============================================================================
# mul_zero: (* a 0) == 0
# ============================================================================

(assert (= (* 0 0) 0) "mul_zero: zero times zero")
(assert (= (* 1 0) 0) "mul_zero: one times zero")
(assert (= (* -1 0) 0) "mul_zero: negative one times zero")
(assert (= (* 42 0) 0) "mul_zero: positive times zero")
(assert (= (* -999 0) 0) "mul_zero: large negative times zero")

# ============================================================================
# distributive: (* a (+ b c)) == (+ (* a b) (* a c))
# ============================================================================

(assert (= (* 2 (+ 3 4)) (+ (* 2 3) (* 2 4))) "distributive: positive integers")
(assert (= (* -1 (+ 5 -3)) (+ (* -1 5) (* -1 -3))) "distributive: negative multiplier")
(assert (= (* 0 (+ 7 9)) (+ (* 0 7) (* 0 9))) "distributive: zero multiplier")
(assert (= (* 10 (+ -5 3)) (+ (* 10 -5) (* 10 3))) "distributive: mixed signs in sum")

# ============================================================================
# div_inverse_of_mul: (/ (* a b) b) == a (b != 0)
# ============================================================================

(assert (= (/ (* 6 3) 3) 6) "div_inverse_of_mul: positive integers")
(assert (= (/ (* -10 5) 5) -10) "div_inverse_of_mul: negative numerator")
(assert (= (/ (* 0 7) 7) 0) "div_inverse_of_mul: zero numerator")
(assert (= (/ (* 100 -4) -4) 100) "div_inverse_of_mul: negative divisor")

# ============================================================================
# div_by_zero_is_error: division by zero signals an error
# ============================================================================

(let [[ok? _] (protect ((fn [] (/ 0 0))))] (assert (not ok?) "div_by_zero_is_error: zero divided by zero"))
(let [[ok? _] (protect ((fn [] (/ 42 0))))] (assert (not ok?) "div_by_zero_is_error: positive divided by zero"))
(let [[ok? _] (protect ((fn [] (/ -1 0))))] (assert (not ok?) "div_by_zero_is_error: negative divided by zero"))

# ============================================================================
# eq_reflexive: (= a a) == true
# ============================================================================

(assert (= 0 0) "eq_reflexive: zero")
(assert (= 42 42) "eq_reflexive: positive")
(assert (= -99 -99) "eq_reflexive: negative")

# ============================================================================
# lt_irreflexive: (< a a) == false
# ============================================================================

(assert (not (< 0 0)) "lt_irreflexive: zero")
(assert (not (< 42 42)) "lt_irreflexive: positive")
(assert (not (< -99 -99)) "lt_irreflexive: negative")

# ============================================================================
# lt_antisymmetric: for a != b, exactly one of (< a b) or (< b a) is true
# ============================================================================

(let [a 3 b 5]
  (assert (or (< a b) (< b a)) "lt_antisymmetric: 3 vs 5 - one is true")
  (assert (not (and (< a b) (< b a))) "lt_antisymmetric: 3 vs 5 - not both true"))

(let [a -7 b 4]
  (assert (or (< a b) (< b a)) "lt_antisymmetric: -7 vs 4 - one is true")
  (assert (not (and (< a b) (< b a))) "lt_antisymmetric: -7 vs 4 - not both true"))

(let [a 0 b 1]
  (assert (or (< a b) (< b a)) "lt_antisymmetric: 0 vs 1 - one is true")
  (assert (not (and (< a b) (< b a))) "lt_antisymmetric: 0 vs 1 - not both true"))

# ============================================================================
# mod_range: (rem a b) has |result| < b (b > 0)
# ============================================================================

(let [result (rem 10 3)]
  (assert (< (abs result) 3) "mod_range: 10 rem 3 is in range"))

(let [result (rem -10 3)]
  (assert (< (abs result) 3) "mod_range: -10 rem 3 is in range"))

(let [result (rem 7 7)]
  (assert (< (abs result) 7) "mod_range: 7 rem 7 is in range"))

(let [result (rem 0 5)]
  (assert (< (abs result) 5) "mod_range: 0 rem 5 is in range"))

# ============================================================================
# int_plus_float_is_float: (float? (+ int float)) == true
# ============================================================================

(assert (float? (+ 3 1.5)) "int_plus_float_is_float: 3 + 1.5")
(assert (float? (+ -7 0.5)) "int_plus_float_is_float: -7 + 0.5")
(assert (float? (+ 0 3.14)) "int_plus_float_is_float: 0 + 3.14")

# ============================================================================
# float_plus_int_is_float: (float? (+ float int)) == true
# ============================================================================

(assert (float? (+ 1.5 3)) "float_plus_int_is_float: 1.5 + 3")
(assert (float? (+ 0.5 -7)) "float_plus_int_is_float: 0.5 + -7")
(assert (float? (+ 3.14 0)) "float_plus_int_is_float: 3.14 + 0")
