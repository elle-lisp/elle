(elle/epoch 15)
# audited: 2026-11-18
# Fixture: a single form whose array holds a closure and a keyword that names its tier.
# docs/test-runner.md
#
# The closures agree, and the keywords do not, so this form records a
# `diverge` row on a build that carries the JIT.
[(fn [] 1) (if (backend? :jit) :jit-value :vm-value)]
