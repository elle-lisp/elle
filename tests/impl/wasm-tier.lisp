(elle/epoch 15)
# audited: 2026-10-06
# The tiered WASM backend returns the right values for silent closures, and refuses a closure that may raise.
# docs/impl/wasm.md
#
# Each closure runs on the tier through `compile/run-on :wasm`, which compiles
# it to its own module whatever policy the process runs under. A build without
# the backend answers every such call with :tier-rejected, and the file gates
# itself there: every value below is also a language claim that the language
# suite checks on the build's own tier.
#
# The trap: the tier holds no park, so its gate (`standalone_emittable`,
# src/wasm/emit.rs) admits only a closure whose inferred signal is silent. `+`,
# `=`, `get` and `first` may raise, so a closure that calls one is refused, and
# the closures here use the `%` intrinsics, which are silent. An intrinsic must
# prove its operands, so `triangle` matches on `%type-of` first. Each closure
# also binds its result rather than ending in a call, because the gate refuses
# a tail call. The counter-factual: under `--wasm=N` a refused closure runs on
# the bytecode VM, so a file of refused closures passes without ever reaching
# the tier. `compile/run-on :wasm` answers a refusal with :tier-rejected
# instead.

# `(fn [] 0)` is a shape every standalone module serves, so a rejection of it
# means the backend is absent, not that one closure was ineligible.
(def _tiered
  (let [[ok? v] (protect (compile/run-on :wasm (fn [] 0)))]
    (if (and (not ok?) (= (get v :error) :tier-rejected))
      (error (struct :error :gated :reason "WASM tier not compiled in"))
      true)))

# Past the gate, a rejection is a failure: each closure below is one the tier
# must serve.
(defn triangle [n]
  (let [r (match (%type-of n)
            :integer
              (if (%eq n 0)
                0
                (%add n (triangle (%sub n 1))))
            _ 0)]
    r))

(assert (= (compile/run-on :wasm triangle 10) 55) "wasm-tier: recursive count")

(defn pair-up [a b]
  (let [r (list a b)]
    r))

(assert (= (compile/run-on :wasm pair-up 1 2) (list 1 2)) "wasm-tier: a list")

# Keyword constants reach the module through its constant pool.
(defn struct-read []
  (let [s {:x 42 :y 99}
        r (%get s :x)]
    r))

(assert (= (compile/run-on :wasm struct-read) 42) "wasm-tier: struct read")

# A closure that may raise is refused: an error parks the raising frame, and a
# standalone module has nowhere to park it. The counter-factual is a gate that
# admits it, so the call answers 2.
(defn may-raise [x]
  (let [r (+ x 1)]
    r))

(let [[ok? v] (protect (compile/run-on :wasm may-raise 1))]
  (assert (not ok?) "wasm-tier: a closure that may raise does not run")
  (assert (= (get v :error) :tier-rejected)
          "wasm-tier: the tier refuses a closure that may raise"))

(println "wasm-tier: ok")
