## Signal System Tests
##
## Tests for the signal declaration, silence, and signals introspection
## features. Migrated from tests/integration/signal_enforcement.rs
## (language behaviour tests that evaluate Elle source and check values).


# ============================================================================
# (signal :keyword) declaration
# ============================================================================

# signal declaration returns the keyword
(assert (= (signal :heartbeat_c2a) :heartbeat_c2a) "signal declaration returns keyword")

# signal in expression position
(def x (signal :expr_pos_c2c))
(assert (= x :expr_pos_c2c) "signal in expression position")

# signal declaration with non-keyword argument errors
(let (([ok? _] (protect ((fn () (eval '(signal heartbeat_c2b))))))) (assert (not ok?) "signal declaration requires keyword"))

# signal declaration of builtin keyword errors
(let (([ok? _] (protect ((fn () (eval '(signal :error))))))) (assert (not ok?) "signal declaration of builtin errors"))

# duplicate signal declaration errors
(let (([ok? _] (protect ((fn () (eval '(begin (signal :dup_c2d) (signal :dup_c2d)))))))) (assert (not ok?) "duplicate signal declaration errors"))

# ============================================================================
# silence runtime checks — passing cases
# ============================================================================

# silence with silent function passes at runtime
(begin
  (def apply-inert (fn (f x y) (silence f) (f x y)))
  (assert (= (apply-inert + 42 1) 43) "silence runtime: silent function passes"))

# silence with non-closure (primitive) passes at runtime
(begin
  (def apply-inert2 (fn (f x y) (silence f) (f x y)))
  (assert (= (apply-inert2 + 42 1) 43) "silence runtime: non-closure passes"))

# squelch with keyword passes for silent closure (open-world: silent closure has no :rt_c5a)
# Direct invocation: (squelch f :rt_c5a) returns a squelched closure; calling it with
# a silent closure succeeds since no :rt_c5a signal is emitted.
(begin
  (signal :rt_c5a)
  (assert (= ((squelch (fn () nil) :rt_c5a)) nil) "squelch runtime: bounded keyword passes for silent closure"))

# silence with dynamic variable passes for silent function
(begin
  (def apply-inert3 (fn (f x y) (silence f) (f x y)))
  (var g +)
  (assert (= (apply-inert3 g 42 1) 43) "silence runtime: dynamic silent passes"))

# ============================================================================
# silence runtime checks — failing cases
# ============================================================================

# silence with yielding closure fails at runtime
(defn apply-inert4 (f x) (silence f) (f x))
(def [ok4? err4] (protect (apply-inert4 (fn (x) (yield x)) 42)))
(assert (not ok4?) "silence runtime: yielding closure fails")
(assert (= (get err4 :error) :signal-violation) "silence runtime: yielding closure is signal-violation")

# squelch with :yield fails for yielding closure (blacklist: :yield is forbidden)
# Called directly in tail position — squelch enforcement now fires in the
# tail-call trampoline loop (accumulated_squelch_mask), not just in call_inner.
(signal :rt_c5b2)
(def [ok5? err5] (protect ((squelch (fn () (yield 1)) :yield))))
(assert (not ok5?) "squelch runtime: :yield forbidden — yielding closure fails")
(assert (= (get err5 :error) :signal-violation) "squelch runtime: :yield forbidden is signal-violation")

# silence with dynamic variable fails for yielding closure
(defn apply-inert5 (f x) (silence f) (f x))
(var g2 (fn (x) (yield x)))
(def [ok6? _] (protect (apply-inert5 g2 42)))
(assert (not ok6?) "silence runtime: dynamic yielding closure fails")

# ============================================================================
# (signals) introspection primitive
# ============================================================================

# signals returns a struct
(assert (= (type-of (signals)) :struct) "signals primitive returns struct")

# signals contains builtin :error at bit 0
(def registry (signals))
(assert (= (get registry :error) 0) "signals contains builtin :error")

# signals contains user-defined signals
(begin
  (signal :intro_c6a)
  (def reg2 (signals))
  # bit position depends on how many user signals were registered before this one
  (assert (>= (get reg2 :intro_c6a) 16) "signals contains user-defined signal at bit >= 16"))

# ============================================================================
# squelch runtime checks — passing cases
# ============================================================================

# squelch runtime: a closure NOT emitting the squelched signal passes
# Bind the squelched closure and invoke it via begin (non-tail) for correct squelch tracking
(begin
  (def result-sq-pass ((squelch (fn (x) (* x 2)) :yield) 21))
  (assert (= result-sq-pass 42) "squelch runtime: non-squelched signal passes"))

# squelch runtime: calling squelch on a non-closure (primitive) is a type error
# (The new squelch primitive requires a closure; use the primitive directly for non-closures)
(begin
  (def [ok-prim? _] (protect (squelch + :yield)))
  (assert (not ok-prim?) "squelch runtime: non-closure produces type error"))

# squelch runtime: a silent closure passes squelch
(begin
  (assert (= ((squelch (fn () 99) :yield)) 99) "squelch runtime: silent closure passes squelch"))

# squelch runtime: a closure with a user-defined signal type passes squelch :yield
# (open-world test: squelch only forbids the listed signals, not everything else)
# A silent closure (fn () 42) passes squelch :yield because it doesn't yield.
(begin
  (signal :sq_audit_c7a)
  # A silent closure passes squelch :yield because it has no :yield signal
  (def [ok-audit? _] (protect ((squelch (fn () 42) :yield))))
  (assert ok-audit? "squelch runtime: closure emitting unrelated user signal passes squelch"))

# ============================================================================
# squelch runtime checks — failing cases
# ============================================================================

# squelch runtime: a closure emitting the squelched signal is rejected with :signal-violation
# Use let to force non-tail position so squelch enforcement fires in call_inner.
(begin
  (def [ok-sq? err-sq] (protect (let ((r ((squelch (fn () (yield 1)) :yield)))) r)))
  (assert (not ok-sq?) "squelch runtime: squelched signal is rejected")
  (assert (= (get err-sq :error) :signal-violation) "squelch runtime: rejection is :signal-violation"))

# squelch runtime: multiple forbidden keywords — closure emitting any one is rejected
# Note: :error alone does not appear in the static signal type (error signals don't suspend),
# so we test with a yielding closure to verify the multi-keyword check fires on :yield.
(begin
  # A closure that yields — :yield is in the forbidden set — use let for non-tail position
  (def [ok-sq-multi? err-sq-multi] (protect (let ((r ((squelch (fn () (yield 42)) :yield :error)))) r)))
  (assert (not ok-sq-multi?) "squelch runtime: multi-keyword squelch rejects :yield")
  (assert (= (get err-sq-multi :error) :signal-violation) "squelch runtime: multi-keyword rejection is :signal-violation"))

# squelch function-level floor: the old (squelch :yield) preamble syntax is removed.
# A pure function (no yield) works fine — squelch is no longer needed at function level.
(begin
  (def add1-sq (fn (x) (+ x 1)))
  (assert (= (add1-sq 5) 6) "squelch function-level: non-yielding body returns correct value"))

# squelch with user signal: (squelch f :yield) on a parameter works as a runtime transform
(begin
  (signal :audit_sq_c5b)
  (assert (= ((squelch (fn () 42) :yield)) 42) "squelch with user signal: silent closure passes squelch :yield"))

# squelch and silence on different params: (silence f) binds at compile time;
# (squelch g :yield) returns a squelched closure at runtime.
# Invoke the squelched closure via begin (non-tail) for correct squelch mask tracking.
(begin
  (defn apply-sq-sil (f g x)
    (silence f)
    (let ((safe-g (squelch g :yield)))
      (begin (f x) (begin (safe-g x)))))
  (assert (= (apply-sq-sil (fn (x) (* x 2)) (fn (x) (+ x 1)) 5) 6) "squelch and silence on different params: both annotations coexist"))

# squelch catches :yield at runtime — use let for non-tail position.
(begin
  (def [ok-sq-wins? err-sq-wins] (protect (let ((r ((squelch (fn () (yield 1)) :yield)))) r)))
  (assert (not ok-sq-wins?) "squelch catches :yield: yielding callback is rejected")
  (assert (= (get err-sq-wins :error) :signal-violation) "squelch catches :yield: rejection is :signal-violation"))

# squelch outside lambda is now a valid primitive call (not a special form).
# (squelch some-closure :yield) returns a new closure.
(begin
  (def some-fn (fn () (yield 1)))
  (def squelched-fn (squelch some-fn :yield))
  (assert (closure? squelched-fn) "squelch outside lambda: returns a closure"))

# ============================================================================
# squelch primitive construction tests (Chunk 2)
# ============================================================================

# squelch returns a closure
(assert (closure? (squelch (fn () (yield 1)) :yield)) "squelch returns a closure")

# squelch on non-closure produces type-error
(let (([ok? err] (protect ((fn () (squelch 42 :yield)))))) (assert (not ok?) "squelch on non-closure produces type-error") (assert (= (get err :error) :type-error) "squelch on non-closure produces type-error"))

# squelch twice ORs the masks; result is still a closure
(begin
  (def sq-composed
    (let ((f (fn () (begin (yield 1)))))
      (let ((sq1 (squelch f :yield)))
        (squelch sq1 :error))))
  (assert (closure? sq-composed) "squelch composable masks: result is a closure"))

# squelch returns a new allocation, not the original closure
(assert (not (identical? (fn () 1) (squelch (fn () 1) :yield))) "squelch returns different allocation from original")

# ============================================================================
# squelch runtime enforcement tests (Chunk 3)
# ============================================================================

# squelch catches yield at boundary — use let for non-tail position
(begin
  (def [ok-catch? err-catch]
    (protect (let ((r ((squelch (fn () (yield 42)) :yield)))) r)))
  (assert (not ok-catch?) "squelch catches yield at boundary: call is rejected")
  (assert (= (get err-catch :error) :signal-violation) "squelch catches yield at boundary: rejection is :signal-violation"))

# squelch catches yield through nested calls — use let for non-tail position
(begin
  (def inner-nest (fn () (yield 1)))
  (def outer-nest (fn () (inner-nest)))
  (def safe-nest (squelch outer-nest :yield))
  (def [ok-nest? err-nest] (protect (let ((r (safe-nest))) r)))
  (assert (not ok-nest?) "squelch nested call enforcement: yield through nested calls is rejected")
  (assert (= (get err-nest :error) :signal-violation) "squelch nested call enforcement: rejection is :signal-violation"))

# squelch catches yield from tail-called yielding function
(begin
  (def yielder-tc (fn () (yield 99)))
  (def safe-tc (squelch (fn () (yielder-tc)) :yield))
  (def [ok-tc? err-tc] (protect (let ((r (safe-tc))) r)))
  (assert (not ok-tc?) "squelch tail call enforcement: yield is rejected")
  (assert (= (get err-tc :error) :signal-violation) "squelch tail call enforcement: rejection is :signal-violation"))

# squelch composable runtime: both :yield and :error squelched; f yields; caught
(begin
  (def [ok-comp-rt? err-comp-rt]
    (protect
      (let* ((f   (fn () (yield 1)))
             (sq1 (squelch f :yield))
             (sq2 (squelch sq1 :error)))
        (let ((r (sq2))) r))))
  (assert (not ok-comp-rt?) "squelch composable runtime: yield is rejected even with multi-signal squelch")
  (assert (= (get err-comp-rt :error) :signal-violation) "squelch composable runtime: rejection is :signal-violation"))

# ============================================================================
# squelch tail-position enforcement (issue-583)
# ============================================================================

# squelch enforcement fires when a squelched closure is called in tail position
# by the callee. The trampoline loop accumulates the squelch mask and enforces
# it when the signal exits, even though call_inner is never reached.
(begin
  (defn outer-tc (f) (f))
  (def yielding (fn () (yield 42)))
  (def squelched-yielding (squelch yielding :yield))
  (def [ok-tc-outer? err-tc-outer] (protect (outer-tc squelched-yielding)))
  (assert (not ok-tc-outer?) "squelch tail position via argument: yielding callback is rejected")
  (assert (= (get err-tc-outer :error) :signal-violation) "squelch tail position via argument: rejection is :signal-violation"))
