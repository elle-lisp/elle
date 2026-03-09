# Bracket syntax tests
#
# Migrated from tests/integration/bracket_errors.rs
# Tests for bracket syntax in special forms (issue #395).
# Verifies that [...] (SyntaxKind::Tuple) is accepted in structural
# positions: params, bindings, clauses, match arms. @[...] (Array)
# is still rejected.

(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false :assert-list-eq assert-list-eq :assert-equal assert-equal :assert-not-nil assert-not-nil :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind} ((import-file "tests/elle/assert.lisp")))

# ============================================================================
# Function parameter brackets
# ============================================================================

(begin
  (assert-eq ((fn [x] x) 42) 42 "fn_params_bracket"))

(begin
  (assert-eq ((fn [x y] (+ x y)) 1 2) 3 "fn_params_bracket_multi"))

(begin
  (assert-eq ((fn [x & xs] x) 1 2 3) 1 "fn_params_bracket_rest"))

# ============================================================================
# Let binding brackets
# ============================================================================

(begin
  (assert-eq (let [(x 1)] x) 1 "let_bindings_bracket_outer"))

(begin
  (assert-eq (let ([x 1]) x) 1 "let_binding_pair_bracket"))

(begin
  (assert-eq (let [[x 1]] x) 1 "let_bindings_bracket_both"))

# ============================================================================
# Letrec binding brackets
# ============================================================================

(begin
  (assert-eq (letrec [(f (fn (x) x))] (f 1)) 1 "letrec_bindings_bracket"))

(begin
  (assert-eq (letrec ([f (fn (x) x)]) (f 1)) 1 "letrec_binding_pair_bracket"))

# ============================================================================
# Cond clause brackets
# ============================================================================

(begin
  (assert-eq (cond [true 42]) 42 "cond_clause_bracket"))

(begin
  (assert-eq (cond [false 1] [else 42]) 42 "cond_clause_bracket_else"))

# ============================================================================
# Match arm brackets
# ============================================================================

(begin
  (assert-eq (match 42 [42 "yes"] [_ nil]) "yes" "match_arm_bracket"))

# ============================================================================
# Error cases: non-list match arms
# ============================================================================

# Note: match with non-list arm is a compile-time error, so we skip this test
# (assert-err (fn () (match 42 99)) "match_arm_non_list_error")

# ============================================================================
# Defmacro and defn with bracket params
# ============================================================================

(begin
  (defmacro id [x] x)
  (assert-eq (id 7) 7 "defmacro_params_bracket"))

(begin
  (defn f [x] x)
  (assert-eq (f 99) 99 "defn_params_bracket"))

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
(assert-err (fn () (eval '(match 42 99)))
  "match arm non-list is compile error")

# fn_params_array_rejected
(assert-err (fn () (eval '(fn @[x] x)))
  "fn params with @[] is compile error")

# let_bindings_array_rejected
(assert-err (fn () (eval '(let @[(x 1)] x)))
  "let bindings with @[] is compile error")
