(elle/epoch 13)
# audited: 2026-09-29
# Fixture: a form gated on a fact about the machine.
# docs/test-runner.md
#
# The runner never sets the variable the gate reads, so the gate is shut, the
# runner records one skip with the reason "needs a widget", and the body never
# runs.
(gate! (get (sys/env) "ELLE_RUNNER_FIXTURE_WIDGET") "needs a widget"
       (assert false "the gate ran its body"))
