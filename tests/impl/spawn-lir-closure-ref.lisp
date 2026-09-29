(elle/epoch 13)
# audited: 2026-09-29
# A spawned closure that calls a stdlib function crosses the thread boundary with its LIR intact.
# docs/threads.md
#
# A closure sent across `sys/spawn-vm` carries its LIR function, so the worker
# thread can compile it. A stdlib function is registered as a primitive, so a
# reference to one inside the closure lowers to a `ValueConst` holding a
# closure value. `convert_value_consts_for_send` (src/lir/types/func.rs)
# replaces each with a `LirConst::ClosureRef` placeholder, and
# `patch_lir_closure_refs` (src/value/send/de.rs) points it at the
# reconstructed closure on the receiving side. Each replacement counts in
# `lir/closure-value-const-count`.
#
# The counter-factual: a sender that gives up on a closure-valued `ValueConst`
# sends no LIR, and the worker runs the closure in the interpreter with the
# right answer. Only the count shows the difference. A lowering change that
# stops stdlib references from reaching the LIR as `ValueConst` also leaves
# the count still, and this file then no longer drives the transfer path.

(let [before (lir/closure-value-const-count)]
  (sys/join (sys/spawn-vm (fn [] (inc 41))))
  (let [after (lir/closure-value-const-count)]
    (assert (> after before)
            "ClosureRef LIR-transfer path fires when a spawned closure calls a stdlib function")))
