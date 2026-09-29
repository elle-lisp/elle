(elle/epoch 12)
# audited: 2026-09-23
# A parameterize frame survives a process scheduler run that starts while a return-value handoff is pending.
# docs/parameters.md
#
# The trap: fiber.signal carries the (SIG_OK, value) return handoff between
# frames, so a signal can be pending while execution is healthy. A
# PushParamFrame that skipped its push on any pending signal would leave its
# balanced PopParamFrame to pop the frame below it: here, the outer
# parameterize's.
#
# The counter-factual: (*witness*) then reads :fallback after the run, while
# every other observable stays the same, so a status-only check would pass.

(def process ((import-file "lib/process.lisp")))

(def *witness* (make-parameter :fallback))

(parameterize ((*witness* :bound))
  # process:run enters the process scheduler's own parameterize (sched-run
  # binds *spawn*) while the caller's return handoff is still pending.
  (let [sched (process:make-scheduler)]
    (process:run sched (fn () nil)))
  (assert (= (*witness*) :bound)
          "a dynamic binding survives a process scheduler run"))
