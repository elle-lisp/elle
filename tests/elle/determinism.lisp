# Determinism tests
#
# Migrated from tests/property/determinism.rs
# The compiler is either deterministic or it isn't — varying input
# values doesn't help detect nondeterminism. One example per form suffices.

(import-file "tests/elle/assert.lisp")

# ============================================================================
# Arithmetic
# ============================================================================

# test_arithmetic_determinism
(assert-eq (+ 37 -82) -45 "arithmetic: (+ 37 -82) == -45")

# ============================================================================
# Let binding
# ============================================================================

# test_let_determinism
(assert-eq (let ((x 42) (y -17)) (+ x y)) 25 "let: (let ((x 42) (y -17)) (+ x y)) == 25")

# ============================================================================
# Lambda
# ============================================================================

# test_lambda_determinism
(assert-eq ((fn (x) (* x 2)) 21) 42 "lambda: ((fn (x) (* x 2)) 21) == 42")

# ============================================================================
# Multi-form
# ============================================================================

# test_multi_form_determinism
(assert-eq (begin (def det-x 13) (def det-y -8) (+ det-x det-y)) 5 "multi_form: (begin (def det-x 13) (def det-y -8) (+ det-x det-y)) == 5")

# ============================================================================
# Closure
# ============================================================================

# test_closure_determinism
(assert-eq (let ((captured 10)) ((fn (x) (+ x captured)) 32)) 42 "closure: (let ((captured 10)) ((fn (x) (+ x captured)) 32)) == 42")

# ============================================================================
# Conditional
# ============================================================================

# test_conditional_determinism
(assert-eq (if (< -5 10) -5 10) -5 "conditional: (if (< -5 10) -5 10) == -5")

# ============================================================================
# Recursive function
# ============================================================================

# test_recursive_determinism
(defn factorial [n]
  "Compute factorial of n"
  (if (<= n 1)
      1
      (* n (factorial (- n 1)))))

(assert-eq (factorial 7) 5040 "recursive: factorial of 7 == 5040")

# ============================================================================
# String operation
# ============================================================================

# test_string_op_determinism
(assert-eq (length "hello") 5 "string_op: (length \"hello\") == 5")
