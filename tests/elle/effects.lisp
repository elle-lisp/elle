## Effect System Tests
##
## Tests for the effect declaration, restrict, and effects introspection
## features. Migrated from tests/integration/effect_enforcement.rs
## (language behaviour tests that evaluate Elle source and check values).

(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false :assert-list-eq assert-list-eq :assert-equal assert-equal :assert-not-nil assert-not-nil :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind} ((import-file "tests/elle/assert.lisp")))

# ============================================================================
# (effect :keyword) declaration
# ============================================================================

# effect declaration returns the keyword
(assert-eq (effect :heartbeat_c2a) :heartbeat_c2a
  "effect declaration returns keyword")

# effect in expression position
(def x (effect :expr_pos_c2c))
(assert-eq x :expr_pos_c2c
  "effect in expression position")

# ============================================================================
# restrict runtime checks — passing cases
# ============================================================================

# restrict with inert function passes at runtime
(begin
  (def apply-inert (fn (f x y) (restrict f) (f x y)))
  (assert-eq (apply-inert + 42 1) 43
    "restrict runtime: inert function passes"))

# restrict with non-closure (primitive) passes at runtime
(begin
  (def apply-inert2 (fn (f x y) (restrict f) (f x y)))
  (assert-eq (apply-inert2 + 42 1) 43
    "restrict runtime: non-closure passes"))

# restrict with bounded keyword passes for inert closure
(begin
  (effect :rt_c5a)
  (def apply-bounded (fn (f) (restrict f :rt_c5a) (f)))
  (assert-eq (apply-bounded (fn () nil)) nil
    "restrict runtime: bounded keyword passes"))

# restrict with dynamic variable passes for inert function
(begin
  (def apply-inert3 (fn (f x y) (restrict f) (f x y)))
  (var g +)
  (assert-eq (apply-inert3 g 42 1) 43
    "restrict runtime: dynamic inert passes"))

# ============================================================================
# (effects) introspection primitive
# ============================================================================

# effects returns a struct
(assert-eq (type-of (effects)) :struct
  "effects primitive returns struct")

# effects contains builtin :error at bit 0
(def registry (effects))
(assert-eq (get registry :error) 0
  "effects contains builtin :error")

# effects contains user-defined effects
(begin
  (effect :intro_c6a)
  (def reg2 (effects))
  # bit position depends on how many user effects were registered before this one
  (assert-true (>= (get reg2 :intro_c6a) 16)
    "effects contains user-defined effect at bit >= 16"))
