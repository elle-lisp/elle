(elle/epoch 12)
# Counterfactual: a fiber that SUSPENDS on a structured-concurrency wait via a
# FUNCTION CALL must resume into the code that follows the call.
#
# THE GAP: a call is compiled with the CPS suspending convention (a resumable
# continuation frame) only when the callee's signal marks it as suspending. The
# LIR lowering (src/lir/lower/control/call.rs) keyed that off SIG_YIELD alone.
# But `(emit :wait …)` narrows to SIG_WAIT — WITHOUT SIG_YIELD — so a call to a
# wrapper whose signal is SIG_WAIT (the scheduler's `emit-wait`, and every
# `ev/join`/`ev/scope` built on it) compiled to a PLAIN call with no continuation
# frame. On resume the fiber returned the wait's own result and the code after
# the wait was lost — under `--wasm=full` the whole async scheduler's
# `handle-wait` path. The fix marks SIG_IO and SIG_WAIT calls suspending too.
#
# Each case below waits (an inner join) and then does MORE work with the result;
# the "MORE work" is exactly the continuation a missing frame drops. On the VM
# and JIT the continuation always ran (they save the whole stack, not a CPS
# frame), so this is RED only under `--wasm=full` before the fix and GREEN on
# every tier after — a cross-tier divergence the whole-file runner surfaces.

# A wait (inner join) followed by an arithmetic continuation.
(assert (= 3
           (ev/join (ev/spawn (fn []
                                (let [x (ev/join (ev/spawn (fn [] 1)))]
                                  (+ x 2))))))
        "continuation after a wait-via-call runs")

# The joined value threads through several post-wait steps.
(assert (= 30
           (ev/join (ev/spawn (fn []
                                (let [a (ev/join (ev/spawn (fn [] 10)))
                                      b (+ a 5)]
                                  (* b 2))))))
        "multi-step continuation after a wait-via-call")

# Two sequential waits, each with its own continuation.
(assert (= 100
           (ev/join (ev/spawn (fn []
                                (let [a (ev/join (ev/spawn (fn [] 40)))
                                      c (ev/join (ev/spawn (fn [] 60)))]
                                  (+ a c))))))
        "two sequential waits, each resuming its continuation")

(println "wasm-wait-call-resumes: ok")
