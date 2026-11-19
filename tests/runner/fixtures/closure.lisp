(elle/epoch 15)
# audited: 2026-11-18
# Fixture: a single form whose value is a closure, which each tier builds anew.
# docs/test-runner.md
#
# `=` compares closures by identity, so no two tiers' closures are `=`. Two
# values of one reference type agree, so the run records no divergence.
(fn [] 42)
