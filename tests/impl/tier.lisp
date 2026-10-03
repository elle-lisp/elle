(elle/epoch 12)
# Runtime tier-introspection: (vm/tier), (backend? :tier), and the gate! macro.
#
# These power the agent-first test runner's skip mechanism (docs/test-runner.md
# § Gating, docs/compile-time.md). The closure is compiled once and dispatched
# to a tier by compile/run-on, so the tier — and thus whether a gate is open —
# is a RUNTIME fact. Assertions encode that contract independently of the impl.

# ── (vm/tier): the active backend tier as a keyword ───────────────────

# With no forced tier (ordinary execution) the tier is :bytecode.
(assert (= (vm/tier) :bytecode) "vm/tier is :bytecode under normal execution")

# Under compile/run-on the closure learns the tier it was dispatched to.
(assert (= (compile/run-on :bytecode (fn [] (vm/tier))) :bytecode)
        "vm/tier reflects the forced :bytecode tier")

# The active tier is restored once compile/run-on returns (RAII guard), so a
# forced-tier dispatch never leaks its tier into surrounding code.
(compile/run-on :bytecode (fn [] :ignored))
(assert (= (vm/tier) :bytecode) "active tier restored after run-on returns")

# ── (backend? :tier): does :tier match the active tier? ───────────────

(assert (compile/run-on :bytecode (fn [] (backend? :bytecode)))
        "backend? :bytecode is true on the bytecode tier")
(assert (not (compile/run-on :bytecode (fn [] (backend? :jit))))
        "backend? :jit is false on the bytecode tier")
# A non-keyword argument is simply never the active tier.
(assert (not (backend? 42)) "backend? of a non-keyword is false")

# ── gate!: loud conditional compilation ───────────────────────────────

# When COND is truthy the body runs and the gate yields the body's value.
(assert (= (gate! true "unused" 41 42) 42) "gate! runs body when cond is truthy")

# When COND is unmet the body does NOT run; the gate emits a structured :gated
# signal carrying the reason, which protect catches as [false payload].
(let [r (protect (gate! false "needs JIT" (error "body must not run")))]
  (assert (not (get r 0)) "gate! signals (does not pass) when cond is unmet")
  (let [payload (get r 1)]
    (assert (= (get payload :error) :gated) "gate! emits :gated")
    (assert (= (get payload :reason) "needs JIT") "gate! carries the reason")))

# The canonical runner pattern: a JIT-gated form skips on the bytecode tier.
(let [r (protect (compile/run-on :bytecode (fn []
                                   (gate! (backend? :jit) "needs JIT"
                                   (assert true "only under JIT")))))]
  (assert (not (get r 0)) "JIT-gated form does not pass on the bytecode tier")
  (assert (= (get (get r 1) :error) :gated) "it skips via :gated")
  (assert (= (get (get r 1) :reason) "needs JIT") "with the gate's reason"))

(println "tier/backend?/gate! tests passed")
