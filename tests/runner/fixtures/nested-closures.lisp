(elle/epoch 15)
# audited: 2026-11-18
# Fixture: a single form whose value holds a closure in an array, a struct and a list.
# docs/test-runner.md
#
# Each tier builds its own three closures. The containers agree element by
# element, so the run records no divergence.
[(fn [] 1) {:f (fn [] 2)} (list (fn [] 3))]
