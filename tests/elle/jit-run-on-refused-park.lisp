(elle/epoch 13)
# audited: 2026-09-28
# A suspension `compile/run-on :jit` refuses ends its park's funding.
#
# docs/impl/region/park.md
#
# The forced JIT tier cannot host a suspension, so it answers a park with a
# :tier-rejected error and the fiber runs on. The park that raised the signal
# is over, and the host discharges its funding (`abandon_hosted_park`). The JIT
# tier answers a park on two arms: its own code yielded, or a bytecode tail
# callee did. Each case below reaches one arm.
#
# The counter-factual is a refusal that leaves the funding standing. The error
# then leaves the protect fiber from tail position, the driver builds its error
# park, and the delivery ledger rejects an unconsumed park (a panic in a debug
# build). A release build would mint a resume reference that nothing releases.

(def _jit-available
  (let [[ok? v] (protect (compile/run-on :jit (fn [] 0)))]
    (if (and (not ok?) (= (get v :error) :tier-rejected))
      (error (struct :error :gated :reason "JIT tier not compiled in"))
      true)))

(defn refused? [r]
  (and (not (get r 0)) (= (get (get r 1) :error) :tier-rejected)
       (= (get (get r 1) :reason) :ineligible)))

# The JIT-compiled thunk itself parks, at a tail io call.
(assert (refused? (protect (compile/run-on :jit (fn [] (ev/sleep 0)))))
        "a thunk that parks is refused")

# The thunk tail-calls a bytecode closure, and that closure parks.
(defn sleeper []
  (ev/sleep 0))
(assert (refused? (protect (compile/run-on :jit (fn [] (sleeper)))))
        "a tail callee that parks is refused")

# The fiber runs on after both refusals.
(assert (= (ev/sleep 0) nil) "io still works after the refusals")

(println "jit-run-on-refused-park: ok")
