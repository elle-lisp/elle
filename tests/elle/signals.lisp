## Signal System Tests
##
## Tests for the signal declaration, silence, and signals introspection
## features. Migrated from tests/integration/signal_enforcement.rs
## (language behaviour tests that evaluate Elle source and check values).

(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false :assert-list-eq assert-list-eq :assert-equal assert-equal :assert-not-nil assert-not-nil :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind} ((import-file "tests/elle/assert.lisp")))

# ============================================================================
# (signal :keyword) declaration
# ============================================================================

# signal declaration returns the keyword
(assert-eq (signal :heartbeat_c2a) :heartbeat_c2a
  "signal declaration returns keyword")

# signal in expression position
(def x (signal :expr_pos_c2c))
(assert-eq x :expr_pos_c2c
  "signal in expression position")

# signal declaration with non-keyword argument errors
(assert-err (fn () (eval '(signal heartbeat_c2b)))
  "signal declaration requires keyword")

# signal declaration of builtin keyword errors
(assert-err (fn () (eval '(signal :error)))
  "signal declaration of builtin errors")

# duplicate signal declaration errors
(assert-err (fn () (eval '(begin (signal :dup_c2d) (signal :dup_c2d))))
  "duplicate signal declaration errors")

# ============================================================================
# silence runtime checks — passing cases
# ============================================================================

# silence with silent function passes at runtime
(begin
  (def apply-inert (fn (f x y) (silence f) (f x y)))
  (assert-eq (apply-inert + 42 1) 43
    "silence runtime: silent function passes"))

# silence with non-closure (primitive) passes at runtime
(begin
  (def apply-inert2 (fn (f x y) (silence f) (f x y)))
  (assert-eq (apply-inert2 + 42 1) 43
    "silence runtime: non-closure passes"))

# silence with bounded keyword passes for silent closure
(begin
  (signal :rt_c5a)
  (def apply-bounded (fn (f) (silence f :rt_c5a) (f)))
  (assert-eq (apply-bounded (fn () nil)) nil
    "silence runtime: bounded keyword passes"))

# silence with dynamic variable passes for silent function
(begin
  (def apply-inert3 (fn (f x y) (silence f) (f x y)))
  (var g +)
  (assert-eq (apply-inert3 g 42 1) 43
    "silence runtime: dynamic silent passes"))

# ============================================================================
# silence runtime checks — failing cases
# ============================================================================

# silence with yielding closure fails at runtime
(defn apply-inert4 (f x) (silence f) (f x))
(def [ok4? err4] (protect (apply-inert4 (fn (x) (yield x)) 42)))
(assert-false ok4? "silence runtime: yielding closure fails")
(assert-eq (get err4 :error) :signal-violation
  "silence runtime: yielding closure is signal-violation")

# silence with bounded keyword fails for yielding closure
(signal :rt_c5b2)
(defn apply-bounded2 (f) (silence f :rt_c5b2) (f))
(def [ok5? err5] (protect (apply-bounded2 (fn () (yield 1)))))
(assert-false ok5? "silence runtime: bounded keyword fails for yielding closure")
(assert-eq (get err5 :error) :signal-violation
  "silence runtime: bounded keyword is signal-violation")

# silence with dynamic variable fails for yielding closure
(defn apply-inert5 (f x) (silence f) (f x))
(var g2 (fn (x) (yield x)))
(def [ok6? _] (protect (apply-inert5 g2 42)))
(assert-false ok6? "silence runtime: dynamic yielding closure fails")

# ============================================================================
# (signals) introspection primitive
# ============================================================================

# signals returns a struct
(assert-eq (type-of (signals)) :struct
  "signals primitive returns struct")

# signals contains builtin :error at bit 0
(def registry (signals))
(assert-eq (get registry :error) 0
  "signals contains builtin :error")

# signals contains user-defined signals
(begin
  (signal :intro_c6a)
  (def reg2 (signals))
  # bit position depends on how many user signals were registered before this one
  (assert-true (>= (get reg2 :intro_c6a) 16)
    "signals contains user-defined signal at bit >= 16"))
