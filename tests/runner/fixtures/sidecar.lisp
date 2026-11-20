(elle/epoch 18)
# audited: 2026-11-20
# Fixture: a passing form with a sidecar, which elle-rig test refuses in-process.
# docs/test-runner.md
(assert (= (+ 1 1) 2) "one plus one is two, under the sidecar")
