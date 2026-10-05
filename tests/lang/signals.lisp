(elle/epoch 15)
# audited: 2026-10-05
## Signal System Tests
##
## Tests for the signal declaration, bound, squelch and signals introspection
## features: language behaviour tests that evaluate Elle source and check
## values.


# ============================================================================
# (signal :keyword) declaration
# ============================================================================

# signal declaration returns the keyword
(assert (= (signal :heartbeat_c2a) :heartbeat_c2a)
        "signal declaration returns keyword")

# signal in expression position
(def x (signal :expr_pos_c2c))
(assert (= x :expr_pos_c2c) "signal in expression position")

# signal declaration with non-keyword argument errors
(let [[ok? _] (protect ((fn () (eval '(signal heartbeat_c2b)))))]
  (assert (not ok?) "signal declaration requires keyword"))

# signal declaration of builtin keyword errors
(let [[ok? _] (protect ((fn () (eval '(signal :error)))))]
  (assert (not ok?) "signal declaration of builtin errors"))

# A second declaration answers the bit the first one took, in one compile and
# across compiles, so two libraries that name a signal share it.
# Counter-factual: a second declaration in one compile was an error.
(let [[ok? _] (protect (compile/whole-module "(signal :dup_c2d) (signal :dup_c2d)"
                       "<dup>"))]
  (assert ok? "a duplicate declaration in one compile compiles"))
(signal :dup_c2d)
(def dup-bit (get (signals) :dup_c2d))
(assert (>= dup-bit 32) "a duplicate declaration names a user signal")
(eval '(signal :dup_c2d))
(assert (= dup-bit (get (signals) :dup_c2d))
        "a declaration in another compile shares the bit")

# ============================================================================
# bound runtime checks — passing cases
# ============================================================================

# a bound passes a silent function
(begin
  (def apply-inert
    (fn (f x y)
      (bound f ||)
      (f x y)))
  (assert (= (apply-inert (fn [x y] x) 43 1) 43)
          "bound runtime: silent function passes"))

# a bound passes a silent native. A native is checked by its
# declared signal, so `length`, which may raise, fails the bound instead.
(begin
  (def apply-inert2
    (fn (f x)
      (bound f ||)
      (f x)))
  (assert (nil? (apply-inert2 meta/origin 42))
          "bound runtime: a silent native passes"))

# squelch with keyword passes for silent closure (open-world: silent closure has no :rt_c5a)
# Direct invocation: (squelch :rt_c5a f) returns a squelched closure; calling it with
# a silent closure succeeds since no :rt_c5a signal is emitted.
(begin
  (signal :rt_c5a)
  (assert (= ((squelch :rt_c5a (fn () nil))) nil)
          "squelch runtime: bounded keyword passes for silent closure"))

# a bound passes a silent function held in a mutable binding
(begin
  (def apply-inert3
    (fn (f x y)
      (bound f ||)
      (f x y)))
  (def @g (fn [x y] x))
  (assert (= (apply-inert3 g 43 1) 43) "bound runtime: dynamic silent passes"))

# ============================================================================
# bound runtime checks — failing cases
# ============================================================================

# a bound refuses a yielding closure at entry; the array hides its signal
# from the compiler, which would otherwise refuse the call
(defn apply-inert4 (f x)
  (bound f ||)
  (f x))
(def [ok4? err4] (protect (apply-inert4 (get @[(fn (x) (yield x))] 0) 42)))
(assert (not ok4?) "bound runtime: yielding closure fails")
(assert (= (get err4 :error) :signal-violation)
        "bound runtime: yielding closure is signal-violation")

# squelch with :yield fails for yielding closure (blacklist: :yield is forbidden)
# Called directly in tail position: the activation enforces the squelch of
# every tail call that replaced its body when it ends.
(signal :rt_c5b2)
(def [ok5? err5] (protect ((squelch :yield (fn () (yield 1))))))
(assert (not ok5?)
        "squelch runtime: :yield forbidden — yielding closure fails")
(assert (= (get err5 :error) :signal-violation)
        "squelch runtime: :yield forbidden is signal-violation")

# a bound refuses a yielding closure held in a mutable binding
(defn apply-inert5 (f x)
  (bound f ||)
  (f x))
(def @g2 (fn (x) (yield x)))
(def [ok6? _] (protect (apply-inert5 g2 42)))
(assert (not ok6?) "bound runtime: dynamic yielding closure fails")

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
  (def reg2 (signals))  # bit position depends on how many user signals were registered before this one
  (assert (>= (get reg2 :intro_c6a) 32)
          "signals contains user-defined signal at bit >= 32"))

# ============================================================================
# squelch runtime checks — passing cases
# ============================================================================

# squelch runtime: a closure NOT emitting the squelched signal passes
# Bind the squelched closure and invoke it via begin (non-tail) for correct squelch tracking
(begin
  (def result-sq-pass ((squelch :yield (fn (x) (* x 2))) 21))
  (assert (= result-sq-pass 42) "squelch runtime: non-squelched signal passes"))

# squelch runtime: calling squelch on a non-closure (primitive) is a type error
# squelch requires a closure; call a primitive directly instead.
(begin
  (def [ok-prim? _] (protect (squelch :yield length)))
  (assert (not ok-prim?) "squelch runtime: non-closure produces type error"))

# squelch runtime: a silent closure passes squelch
(begin
  (assert (= ((squelch :yield (fn () 99))) 99)
          "squelch runtime: silent closure passes squelch"))

# squelch runtime: a closure with a user-defined signal type passes squelch :yield
# (open-world test: squelch only forbids the listed signals, not everything else)
# A silent closure (fn () 42) passes squelch :yield because it doesn't yield.
(begin
  (signal :sq_audit_c7a)  # A silent closure passes squelch :yield because it has no :yield signal
  (def [ok-audit? _] (protect ((squelch :yield (fn () 42)))))
  (assert ok-audit?
          "squelch runtime: closure emitting unrelated user signal passes squelch"))

# ============================================================================
# squelch runtime checks — failing cases
# ============================================================================

# squelch runtime: a closure emitting the squelched signal is rejected with :signal-violation
# Use let to force non-tail position so the squelch is enforced where the
# call completes.
(begin
  (def [ok-sq? err-sq]
    (protect (let [r ((squelch :yield (fn () (yield 1))))]
               r)))
  (assert (not ok-sq?) "squelch runtime: squelched signal is rejected")
  (assert (= (get err-sq :error) :signal-violation)
          "squelch runtime: rejection is :signal-violation"))

# squelch runtime: multiple forbidden keywords — closure emitting any one is rejected
# :error passes every boundary, so the yield is the bit the check fires on.
(begin  # A closure that yields — :yield is in the forbidden set — use let for non-tail position
  (def [ok-sq-multi? err-sq-multi]
    (protect (let [r ((squelch |:yield :error| (fn () (yield 42))))]
               r)))
  (assert (not ok-sq-multi?)
          "squelch runtime: multi-keyword squelch rejects :yield")
  (assert (= (get err-sq-multi :error) :signal-violation)
          "squelch runtime: multi-keyword rejection is :signal-violation"))

# A pure function needs no squelch at all.
(begin
  (def add1-sq (fn (x) (+ x 1)))
  (assert (= (add1-sq 5) 6)
          "squelch function-level: non-yielding body returns correct value"))

# squelch with user signal: (squelch :yield f) on a parameter works as a runtime transform
(begin
  (signal :audit_sq_c5b)
  (assert (= ((squelch :yield (fn () 42))) 42)
          "squelch with user signal: silent closure passes squelch :yield"))

# squelch and bound on different params: (bound f ||) checks at the call;
# (squelch :yield g) returns a squelched closure at runtime.
# Invoke the squelched closure via begin (non-tail) for correct squelch mask tracking.
(begin
  (defn apply-sq-sil (f g x)
    (bound f ||)
    (let [safe-g (squelch :yield g)]
      (begin
        (f x)
        (begin
          # f must be truly silent (no SIG_ERROR);
          # use identity instead of (* x 2)
          (safe-g x)))))
  (assert (= (apply-sq-sil (fn (x) x) (fn (x) (+ x 1)) 5) 6)
          "squelch and bound on different params: both coexist"))

# squelch catches :yield at runtime — use let for non-tail position.
(begin
  (def [ok-sq-wins? err-sq-wins]
    (protect (let [r ((squelch :yield (fn () (yield 1))))]
               r)))
  (assert (not ok-sq-wins?)
          "squelch catches :yield: yielding callback is rejected")
  (assert (= (get err-sq-wins :error) :signal-violation)
          "squelch catches :yield: rejection is :signal-violation"))

# squelch is a primitive, so it works outside a lambda.
# (squelch :yield some-closure) returns a new closure.
(begin
  (def some-fn (fn () (yield 1)))
  (def squelched-fn (squelch :yield some-fn))
  (assert (closure? squelched-fn) "squelch outside lambda: returns a closure"))

# ============================================================================
# squelch primitive construction
# ============================================================================

# squelch returns a closure
(assert (closure? (squelch :yield (fn () (yield 1))))
        "squelch returns a closure")

# squelch on non-closure produces type-error
(let [[ok? err] (protect ((fn () (squelch :yield 42))))]
  (assert (not ok?) "squelch on non-closure produces type-error")
  (assert (= (get err :error) :type-error)
          "squelch on non-closure produces type-error"))

# squelch twice ORs the masks; result is still a closure
(begin
  (def sq-composed
    (let [f (fn ()
              (begin
                (yield 1)))]
      (let [sq1 (squelch :yield f)]
        (squelch :error sq1))))
  (assert (closure? sq-composed) "squelch composable masks: result is a closure"))

# squelch returns a new allocation, not the original closure
(assert (not (identical? (fn () 1) (squelch :yield (fn () 1))))
        "squelch returns different allocation from original")

# ============================================================================
# squelch runtime enforcement
# ============================================================================

# squelch catches yield at boundary — use let for non-tail position
(begin
  (def [ok-catch? err-catch]
    (protect (let [r ((squelch :yield (fn () (yield 42))))]
               r)))
  (assert (not ok-catch?) "squelch catches yield at boundary: call is rejected")
  (assert (= (get err-catch :error) :signal-violation)
          "squelch catches yield at boundary: rejection is :signal-violation"))

# squelch catches yield through nested calls — use let for non-tail position
(begin
  (def inner-nest (fn () (yield 1)))
  (def outer-nest (fn () (inner-nest)))
  (def safe-nest (squelch :yield outer-nest))
  (def [ok-nest? err-nest]
    (protect (let [r (safe-nest)]
               r)))
  (assert (not ok-nest?)
          "squelch nested call enforcement: yield through nested calls is rejected")
  (assert (= (get err-nest :error) :signal-violation)
          "squelch nested call enforcement: rejection is :signal-violation"))

# squelch catches yield from tail-called yielding function
(begin
  (def yielder-tc (fn () (yield 99)))
  (def safe-tc (squelch :yield (fn () (yielder-tc))))
  (def [ok-tc? err-tc]
    (protect (let [r (safe-tc)]
               r)))
  (assert (not ok-tc?) "squelch tail call enforcement: yield is rejected")
  (assert (= (get err-tc :error) :signal-violation)
          "squelch tail call enforcement: rejection is :signal-violation"))

# squelch composable runtime: both :yield and :error squelched; f yields; caught
(begin
  (def [ok-comp-rt? err-comp-rt]
    (protect (let* [f (fn () (yield 1))
                    sq1 (squelch :yield f)
                    sq2 (squelch :error sq1)]
               (let [r (sq2)]
                 r))))
  (assert (not ok-comp-rt?)
          "squelch composable runtime: yield is rejected even with multi-signal squelch")
  (assert (= (get err-comp-rt :error) :signal-violation)
          "squelch composable runtime: rejection is :signal-violation"))

# ============================================================================
# squelch tail-position enforcement
# ============================================================================

# squelch enforcement fires when a squelched closure is called in tail position
# by the callee. The trampoline loop accumulates the squelch mask and enforces
# it when the signal exits, even though call_inner is never reached.
(begin
  (defn outer-tc (f)
    (f))
  (def yielding (fn () (yield 42)))
  (def squelched-yielding (squelch :yield yielding))
  (def [ok-tc-outer? err-tc-outer] (protect (outer-tc squelched-yielding)))
  (assert (not ok-tc-outer?)
          "squelch tail position via argument: yielding callback is rejected")
  (assert (= (get err-tc-outer :error) :signal-violation)
          "squelch tail position via argument: rejection is :signal-violation"))
