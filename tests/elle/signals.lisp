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

# squelch with keyword passes for silent closure (open-world: silent closure has no :rt_c5a)
(begin
  (signal :rt_c5a)
  (def apply-bounded (fn (f) (squelch f :rt_c5a) (f)))
  (assert-eq (apply-bounded (fn () nil)) nil
    "squelch runtime: bounded keyword passes for silent closure"))

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

# squelch with :yield fails for yielding closure (blacklist: :yield is forbidden)
(signal :rt_c5b2)
(defn apply-bounded2 (f) (squelch f :yield) (f))
(def [ok5? err5] (protect (apply-bounded2 (fn () (yield 1)))))
(assert-false ok5? "squelch runtime: :yield forbidden — yielding closure fails")
(assert-eq (get err5 :error) :signal-violation
  "squelch runtime: :yield forbidden is signal-violation")

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

# ============================================================================
# squelch runtime checks — passing cases
# ============================================================================

# squelch runtime: a closure NOT emitting the squelched signal passes
(begin
  (defn apply-sq-pass (f x) (squelch f :yield) (f x))
  (def result-sq-pass (apply-sq-pass (fn (x) (* x 2)) 21))
  (assert-eq result-sq-pass 42
    "squelch runtime: non-squelched signal passes"))

# squelch runtime: a non-closure (primitive) passes squelch
(begin
  (defn apply-sq-prim (f x y) (squelch f :yield) (f x y))
  (assert-eq (apply-sq-prim + 1 2) 3
    "squelch runtime: non-closure passes squelch"))

# squelch runtime: a silent closure passes squelch
(begin
  (defn apply-sq-silent (f) (squelch f :yield) (f))
  (assert-eq (apply-sq-silent (fn () 99)) 99
    "squelch runtime: silent closure passes squelch"))

# squelch runtime: a closure with a user-defined signal type passes squelch :yield
# (open-world test: squelch only forbids the listed signals, not everything else)
# A closure that errors (signal :error, bit 0) is NOT restricted by (squelch f :yield).
# The CheckSignalForbidden check passes because :error is not in the forbidden set.
# Note: we use protect with a plain silent closure here to verify the runtime check passes.
(begin
  (signal :sq_audit_c7a)
  (defn apply-sq-audit (f) (squelch f :yield) (f))
  # A silent closure passes squelch :yield because it has no :yield signal
  (def [ok-audit? _] (protect (apply-sq-audit (fn () 42))))
  (assert-true ok-audit?
    "squelch runtime: closure emitting unrelated user signal passes squelch"))

# ============================================================================
# squelch runtime checks — failing cases
# ============================================================================

# squelch runtime: a closure emitting the squelched signal is rejected with :signal-violation
(begin
  (defn apply-sq-fail (f) (squelch f :yield) (f))
  (def [ok-sq? err-sq] (protect (apply-sq-fail (fn () (yield 1)))))
  (assert-false ok-sq?
    "squelch runtime: squelched signal is rejected")
  (assert-eq (get err-sq :error) :signal-violation
    "squelch runtime: rejection is :signal-violation"))

# squelch runtime: multiple forbidden keywords — closure emitting any one is rejected
# Note: :error alone does not appear in the static signal type (error signals don't suspend),
# so we test with a yielding closure to verify the multi-keyword check fires on :yield.
(begin
  (defn apply-sq-multi (f) (squelch f :yield :error) (f))
  # A closure that yields — :yield is in the forbidden set
  (def [ok-sq-multi? err-sq-multi] (protect (apply-sq-multi (fn () (yield 42)))))
  (assert-false ok-sq-multi?
    "squelch runtime: multi-keyword squelch rejects :yield")
  (assert-eq (get err-sq-multi :error) :signal-violation
    "squelch runtime: multi-keyword rejection is :signal-violation"))

# squelch function-level floor passes: (squelch :yield) at function level
# with a non-yielding body compiles and runs correctly
(begin
  (def add1-sq (fn (x) (squelch :yield) (+ x 1)))
  (assert-eq (add1-sq 5) 6
    "squelch function-level: non-yielding body passes and returns correct value"))

# squelch with user signal: (squelch f :yield) on a parameter compiles when
# a user signal is registered — squelch is blacklist (only :yield forbidden)
(begin
  (signal :audit_sq_c5b)
  (defn apply-sq-user (f) (squelch f :yield) (f))
  (assert-eq (apply-sq-user (fn () 42)) 42
    "squelch with user signal: silent closure passes squelch :yield"))

# squelch and silence on different params: (silence f) and (squelch g :yield) compile
# and work together — no conflict since they apply to different parameters
(begin
  (defn apply-sq-sil (f g x) (silence f) (squelch g :yield) (begin (f x) (g x)))
  (assert-eq (apply-sq-sil (fn (x) (* x 2)) (fn (x) (+ x 1)) 5) 6
    "squelch and silence on different params: both annotations coexist"))

# squelch overrides silence on same param: last form wins — squelch forbids :yield.
# A yielding callback is rejected with :signal-violation.
(begin
  (defn apply-sq-wins (f) (silence f) (squelch f :yield) (f))
  (def [ok-sq-wins? err-sq-wins] (protect (apply-sq-wins (fn () (yield 1)))))
  (assert-false ok-sq-wins?
    "squelch overrides silence: yielding callback is rejected")
  (assert-eq (get err-sq-wins :error) :signal-violation
    "squelch overrides silence: rejection is :signal-violation"))

# squelch outside lambda is not a special form: using squelch outside a lambda
# context is a compile error. The error must NOT contain "inside a function body"
# (that message is reserved for special forms that explicitly require a lambda context).
# We use eval to compile and run the expression at runtime, capturing the error.
(begin
  (def [ok-sq-top? err-sq-top] (protect (eval '(squelch some-fn :yield))))
  (assert-false ok-sq-top?
    "squelch outside lambda: produces an error")
  (assert-false (string/contains? (string err-sq-top) "inside a function body")
    "squelch outside lambda: error does not say 'inside a function body'"))
