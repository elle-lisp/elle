(elle/epoch 15)
# audited: 2026-11-18
# Fixture: a single form whose value is a closure on the JIT and a parameter elsewhere.
# docs/test-runner.md
#
# Two reference values agree only when `type-of` names one type for both, so
# this form records a `diverge` row on a build that carries the JIT.
(if (backend? :jit) (fn [] 1) (make-parameter 1))
