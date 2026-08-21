(elle/epoch 12)
# Fixture for the agent-first test runner (docs/test-runner.md § CAS asset
# capture). A single test form that writes to BOTH stdout and stderr and then
# passes — exercises per-(form × tier) stdout/stderr capture. The output is
# captured under a worker-side (ev/run ...) with *stdout*/*stderr* rebound to
# temp files; non-empty output lands as `stdout`/`stderr` assets in the CAS.
(begin
  (println "hello stdout")
  (eprintln "hello stderr")
  (assert (= (+ 1 1) 2) "prints then passes"))
