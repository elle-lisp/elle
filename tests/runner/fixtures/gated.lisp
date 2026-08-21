(elle/epoch 12)
# Fixture: a form gated to the JIT tier (docs/test-runner.md § Gating).
# Expected runner behavior, per tier:
#   vm  -> status=skip,  reason="needs JIT"  (the loud gate emits :gated)
#   jit -> status=pass   (gate open, body runs)
# Uses the proposed compile-time gate `gate!` and predicate `backend?`.
(gate! (backend? :jit) "needs JIT" (assert true "runs only under JIT"))
