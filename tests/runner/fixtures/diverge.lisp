(elle/epoch 12)
# Fixture: a bare form whose RETURN VALUE differs across tiers, so the runner's
# cross-tier value comparison records a `diverge` row capturing each tier's value
# (docs/test-runner.md § Tiers). Constructed with the proposed `backend?` so the
# divergence is deterministic — a real cross-tier value disagreement would be a bug.
(if (backend? :jit) :jit-value :vm-value)
