(elle/epoch 12)
# A top-level loud gate whose condition is unmet. Run DIRECTLY (not under the
# test runner), an uncaught :gated signal must be treated as a clean SKIP —
# exit 0 with the reason on stderr — so gate! is a universal skip mechanism
# (the same intent the runner records as status=skip), not a crash. This is
# what lets a service/FFI test self-skip under `make smoke` (direct `elle FILE`)
# without the dangerous (sys/exit 0) idiom.
(gate! false "gated-toplevel: dependency absent"
       (assert false "must not run when gated"))
(println "must not print when gated")
