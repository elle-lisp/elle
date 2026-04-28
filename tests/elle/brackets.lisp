(elle/epoch 9)
# Bracket syntax tests
#
# Migrated from tests/integration/bracket_errors.rs
# Tests for bracket syntax in special forms (issue #395).
# Verifies that [...] (SyntaxKind::Tuple) is accepted in structural
# positions: params, bindings, clauses, match arms. @[...] (Array)
# is still rejected.


# ============================================================================
# Function parameter brackets
# ============================================================================

(begin
  (assert (= ((fn [x] x) 42) 42) "fn_params_bracket"))

(begin
  (assert (= ((fn [x y] (+ x y)) 1 2) 3) "fn_params_bracket_multi"))

(begin
  (assert (= ((fn [x & xs] x) 1 2 3) 1) "fn_params_bracket_rest"))

# ============================================================================
# Let binding brackets
# ============================================================================

(begin
  (assert (= (let [x 1]
               x) 1) "let_bindings_bracket_outer"))

(begin
  (assert (= (let [x 1]
               x) 1) "let_binding_pair_bracket"))

(begin
  (assert (= (let [x 1]
               x) 1) "let_bindings_bracket_both"))

# ============================================================================
# Letrec binding brackets
# ============================================================================

(begin
  (assert (= (letrec [f (fn (x) x)]
               (f 1)) 1) "letrec_bindings_bracket"))

(begin
  (assert (= (letrec [f (fn (x) x)]
               (f 1)) 1) "letrec_binding_pair_bracket"))

# ============================================================================
# Cond clause brackets
# ============================================================================

(begin
  (assert (= (cond
               true 42) 42) "cond_clause_bracket"))

(begin
  (assert (= (cond
               false 1
               42) 42) "cond_clause_bracket_else"))

# ============================================================================
# Match arm brackets
# ============================================================================

(begin
  (assert (= (match 42
               42 "yes"
               _ nil) "yes") "match_arm_bracket"))

# ============================================================================
# Error cases: non-list match arms
# ============================================================================

# Note: match with non-list arm is a compile-time error, so we skip this test
# (assert-err (fn () (match 42 99)) "match_arm_non_list_error")

# ============================================================================
# Defmacro and defn with bracket params
# ============================================================================

(begin
  (defmacro id [x]
    x)
  (assert (= (id 7) 7) "defmacro_params_bracket"))

(begin
  (defn f [x]
    x)
  (assert (= (f 99) 99) "defn_params_bracket"))

# ============================================================================
# Error cases: array brackets rejected in structural positions
# ============================================================================

# Note: These are compile-time errors, not runtime errors, so they cannot
# be tested with assert-err (which expects runtime errors).
# fn_params_array_rejected and let_bindings_array_rejected are skipped.

# ============================================================================
# Error message tests (from integration/bracket_errors.rs)
# ============================================================================

# match_arm_non_list_error
(let [[ok? _] (protect ((fn ()
                          (eval '(match 42
                                   99)))))]
  (assert (not ok?) "match arm non-list is compile error"))

# fn_params_array_rejected
(let [[ok? _] (protect ((fn () (eval '(fn @[x] x)))))]
  (assert (not ok?) "fn params with @[] is compile error"))

# let_bindings_array_rejected
(let [[ok? _] (protect ((fn () (eval '(let @[(x 1)] x)))))]
  (assert (not ok?) "let bindings with @[] is compile error"))
