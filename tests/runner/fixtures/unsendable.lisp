(elle/epoch 13)
## audited: 2026-09-30
## Fixture: one form whose value is a fiber, which no worker can hand back.
## docs/test-runner.md
##
## One form takes the per-form path. The worker runs it and cannot send the
## fiber back through os/join, so the runner runs the form again in its own
## process.
(begin
  (assert (= (+ 1 2) 3) "a form whose value cannot leave its worker")
  (fiber/new (fn [] 42) 1))
