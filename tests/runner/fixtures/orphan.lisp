(elle/epoch 13)
## audited: 2026-09-29
## Fixture: a whole-file script whose value is a fiber, which no worker can
## hand back.
## docs/test-runner.md
##
## The worker runs the script and cannot send its value back through os/join,
## so the runner runs the script again in its own process.
(assert (= (+ 1 2) 3) "a script whose value cannot leave its worker")
(ev/spawn (fn [] 1))
