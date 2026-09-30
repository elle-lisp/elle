(elle/epoch 13)
# audited: 2026-09-29
# Fixture: a single form whose value depends on the tier it runs on.
# docs/test-runner.md
#
# The runner compares the values its tiers returned, so this form records a
# `diverge` row holding each tier's value. `backend?` makes the disagreement
# deterministic; a real one would be a defect in a tier.
(if (backend? :jit) :jit-value :vm-value)
