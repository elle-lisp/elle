(elle/epoch 12)
## wasm/tier-error-signal — an error raised inside a closure on the forced WASM
## tier reaches the caller as an error, not as a value.
##
## THE TRAP: a compiled WASM closure answers on two channels. The `status` word
## it returns says whether it suspended (0 = ran to completion, >0 = the resume
## state it parked at). The signal it raised is written to linear memory[0..8).
## `WasmTier::call` read only `status` and built the caller's SignalBits from it
## (`SignalBits::new(status as u64)`), so a primitive that failed inside the
## closure wrote SIG_ERROR to memory, returned status 0, and reached the caller
## as SIG_OK carrying the error struct as an ordinary return value.
##
## COUNTER-FACTUAL: the *value* is the same either way — the error struct comes
## back whichever channel is read — so comparing return values across tiers
## passes. The divergence is in pass/fail: `protect` reported ok?=true on :wasm
## and ok?=false on :bytecode and :jit. Assert on ok?, not on the value.

# Gate: a build without the wasm feature answers every `compile/run-on :wasm`
# with :tier-rejected. `(fn [] 0)` is trivially standalone-emittable, so a
# rejection of it means the tier is absent, not that a closure was ineligible.
(def _wasm-available
  (let [[ok? v] (protect (compile/run-on :wasm (fn [] 0)))]
    (if (and (not ok?) (= (get v :error) :tier-rejected))
      (error (struct :error :gated :reason "WASM tier not compiled in"))
      true)))

# The failing call must NOT be in tail position. `standalone_emittable`
# (src/wasm/emit.rs) refuses a TailCall, so a tail-position `(length 5)` is
# rejected before it runs and never reaches the signal channel at all.
(defn failing-body []
  (let [x (length 5)]
    x))

(let [[ok? v] (protect (compile/run-on :wasm failing-body))]
  (assert (not ok?)
          "a primitive error inside a WASM-tier closure propagates as an error")
  (assert (= (get v :error) :type-error)
          "the propagated error keeps its own payload, not a wasm-error wrapper"))

# Every tier that can host this closure must agree it fails. A tier the build
# lacks answers :tier-rejected; that is absence, not disagreement, so skip it.
(each tier in [:bytecode :jit :wasm]
  (let [[ok? v] (protect (compile/run-on tier failing-body))]
    (unless (and (not ok?) (= (get v :error) :tier-rejected))
      (assert (not ok?)
              (string "tier " tier " must report the error as a failure"))
      (assert (= (get v :error) :type-error)
              (string "tier " tier " must report the type-error payload")))))

(println "wasm-tier-error-signal: ok")
