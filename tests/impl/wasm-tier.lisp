(elle/epoch 13)
# audited: 2026-09-29
# The tiered WASM backend returns the right values for a recursive count, a recursive list walk and a struct read.
# docs/impl/wasm.md
#
# Each closure runs on the tier through `compile/run-on :wasm`, which compiles
# it to its own module whatever policy the process runs under. A build without
# the backend answers every such call with :tier-rejected, and the file gates
# itself there: every assertion below is also a language claim that the
# language suite checks on the build's own tier.
#
# The trap: the standalone gate (`standalone_emittable`, src/wasm/emit.rs)
# refuses a closure that ends in a call, or that makes a call which may
# suspend, and `<` is such a call here. So each closure binds its result and
# returns the binding, and tests with `=` and `empty?`. The counter-factual:
# under `--wasm=N` a refused closure runs on the bytecode VM, so a file of
# refused closures passes without ever reaching the tier.

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
  (let [r (if (= n 0)
            0
            (+ n (triangle (- n 1))))]
    r))

(assert (= (compile/run-on :wasm triangle 10) 55) "wasm-tier: recursive count")

(defn my-sum [xs]
  (let [r (if (empty? xs)
            0
            (+ (first xs) (my-sum (rest xs))))]
    r))

(assert (= (compile/run-on :wasm my-sum (list 1 2 3 4 5)) 15)
        "wasm-tier: list sum")

# Keyword constants reach the module through its constant pool.
(defn struct-read []
  (let [s {:x 42 :y 99}
        r (get s :x)]
    r))

(assert (= (compile/run-on :wasm struct-read) 42) "wasm-tier: struct read")

(println "wasm-tier: ok")
