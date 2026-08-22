(elle/epoch 12)
## tests/elle/param-frame-balance.lisp — a `parameterize` frame survives work
## that completes with the interpreter's return-value handoff signal still
## parked in `fiber.signal`.
##
## The trap: `PushParamFrame` used to skip its push whenever ANY signal was
## pending, reading the ambient `(SIG_OK, value)` return handoff as its own
## type-error flag. The body then ran without its frame, and the balanced
## `PopParamFrame` at scope end popped the frame BELOW — here the outer
## `parameterize`'s, in a spawned fiber its seeded parameter baseline, whose
## recorded fiber → value edges then trip the free-time edge oracle
## (the `tests/elle/process.lisp` teardown drift).
##
## The counter-factual: with the buggy guard, `(*witness*)` reads :fallback
## after the scheduler run — the binding silently vanished — while every
## other observable behavior stays green. A status-only check would pass.

(def process ((import-file "lib/process.lisp")))
(def backend (*io-backend*))

(def *witness* (make-parameter :fallback))

(parameterize ((*witness* :bound))
  # `process:run` enters lib/process's own `parameterize` (sched-run rebinds
  # *spawn*) at a moment when the caller's frame-return handoff signal is
  # still parked — the distilled shape of the external-API scheduler run in
  # tests/elle/process.lisp.
  (let [sched (process:make-scheduler :backend backend)]
    (process:run sched (fn () nil)))
  (assert (= (*witness*) :bound)
          "a dynamic binding survives a process scheduler run"))
