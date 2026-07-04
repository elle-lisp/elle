(elle/epoch 12)
# Shared setup binds an UNSENDABLE value (a fiber). A per-form worker receives
# each test thunk by deep-copying it across os/spawn; a thunk that captures the
# fiber can't serialize, so the spawn raises a :thread-error and the form could
# never run in a worker. The runner must detect that and re-run the form
# IN-PROCESS (main VM) rather than record a spurious fail.
#
# Contract (docs/test-runner.md § Isolation, "Unsendable captures fall back to
# in-process"): a form capturing the fiber still PASSES (run in-process); a form
# that does NOT capture it keeps the normal isolated worker path and also passes.
(def f (fiber/new (fn [] 42) 1))

# Captures `f` (unsendable) → worker spawn fails → in-process fallback → pass.
(assert (fiber/new? f) "captured fiber is usable (runs in-process)")

# Does not capture `f` → ordinary sendable worker path → pass.
(assert (= 3 (+ 1 2)) "a sendable form alongside still runs in a worker")
