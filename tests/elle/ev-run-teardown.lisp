(elle/epoch 11)
## tests/elle/ev-run-teardown.lisp
##
## Regression test for ev/run "program-completion teardown".
##
## The program is its thunks.  Once the top-level program completes,
## ev/run shuts the scheduler down and aborts EVERY remaining fiber
## uniformly (do-shutdown) — it does NOT wait for the scheduler to go
## globally idle.  A background fiber that never completes on its own
## (a futex-parked fiber that is never woken, or a reader parked on a
## socket the program never closes — e.g. an unclosed grpc/h2
## connection) must therefore NOT keep the process alive past the
## program.
##
## Pre-fix, ev/run pumped until `step` reported :done, which requires
## `pending` (in-flight I/O) and `park-queues` to be empty.  An orphan
## fiber parked forever kept them non-empty, so pump looped forever and
## the process hung at teardown (only killable via SIGTERM).
##
## How this test asserts: it spawns orphan fibers that can never
## complete on their own, then the top-level completes.  If ev/run
## reaps them, the process exits and the harness sees a clean pass.
## If it regresses, the process hangs and the harness times the test
## out — a failure.  We also self-bound with ev/timeout so the failure
## mode is a crisp assertion rather than a wall-clock hang.

## ── 1. A futex-parked orphan must not block program teardown ─────────────
##
## Run a sub-program (via ev/run) that spawns a fiber parked forever on
## a futex key that is never woken, then returns.  ev/run must return.

(let* [start (clock/monotonic)
       result (ev/timeout 5.0
                          (fn []
                            (ev/run (fn []
                                      (let [b (box 0)]
                                        (ev/spawn (fn []
                                          (ev/futex-wait :never-woken b 0)
                                          (println "  BUG: futex orphan resumed")))
                                        :program-done)))))
       elapsed (- (clock/monotonic) start)]
  (assert (not (nil? result))
          (concat "1a: ev/run with a forever-futex-parked orphan must "
                  "return (teardown), not hang; ev/timeout fired after "
                  (string elapsed) "s"))
  (assert (= result :program-done)
          (concat "1b: ev/run must return its thunk's value; got "
                  (string result)))
  (assert (< elapsed 4.0)
          (concat "1c: teardown must be prompt, not ride the timeout; took "
                  (string elapsed) "s")))

(println "  PASS: futex-parked orphan reaped at program completion")

## ── 2. An I/O-parked orphan must not block program teardown ──────────────
##
## The real-world shape: a fiber parked on a socket read/accept that no
## one will ever satisfy (the connection's reader the program forgot to
## close).  Park a fiber on accept against a listener nobody connects
## to, then complete the program.

(let* [start (clock/monotonic)
       result (ev/timeout 5.0
                          (fn []
                            (ev/run (fn []
                                      (let [listener (tcp/listen "127.0.0.1" 0)]
                                        (ev/spawn (fn []
                                          (protect (tcp/accept listener))
                                          (println "  (accept orphan unparked)")))  ## Give the orphan a tick to actually park on accept.
                                        (ev/sleep 0.05)
                                        :program-done)))))
       elapsed (- (clock/monotonic) start)]
  (assert (not (nil? result))
          (concat "2a: ev/run with an I/O-parked (accept) orphan must "
                  "return (teardown), not hang; ev/timeout fired after "
                  (string elapsed) "s"))
  (assert (= result :program-done)
          (concat "2b: ev/run must return its thunk's value; got "
                  (string result)))
  (assert (< elapsed 4.0)
          (concat "2c: teardown must be prompt; took " (string elapsed) "s")))

(println "  PASS: I/O-parked orphan reaped at program completion")

## ── 3. Explicitly-joined work still completes (no premature kill) ────────
##
## Teardown must only reap UN-awaited fibers.  A fiber the program joins
## must run to completion and deliver its value before the program ends.

(let [result (ev/run (fn []
                       (let [f (ev/spawn (fn []
                               (ev/sleep 0.05)
                               41))]
                         (+ 1 (ev/join f)))))]
  (assert (= result 42)
          (concat "3a: joined fiber must complete and deliver its value "
                  "(teardown must not kill awaited work); got " (string result))))

(println "  PASS: explicitly-joined work completes before teardown")

## ── 4. A timer-parked orphan's injected :shutdown must not re-raise ──────
##
## do-shutdown reaps a fiber by injecting {:error :shutdown}.  A fiber
## parked on a TIMER (ev/sleep) is in `pending` (SIG_IO), so it is aborted
## in Phase 1 — distinct from a futex-parked fiber (Phase 1b) or one whose
## body protects the abort point (test 2 wraps accept in `protect`, so its
## fiber completes :ok and is never re-raised).  Here the orphan does NOT
## protect: the injected :shutdown propagates and the fiber ends :error.
##
## Every teardown kill site must record the fiber as scheduler-killed so
## the unjoined-error tail excludes it; otherwise ev/run re-surfaces our
## own :shutdown signal as a spurious user error.  Pre-fix, Phase 1 did
## not record, so this orphan crashed ev/run with {:error :shutdown}.
##
## Counter to test 2 (joined-work) below: a genuine unjoined *user* error
## must still crash — that invariant is covered by ev-unjoined-error.lisp;
## here we assert only that OUR injected teardown signal is suppressed.

(let* [outcome (protect (ev/timeout 5.0
                                    (fn []
                                      (ev/run (fn []
                                        (ev/spawn (fn []
                                          (ev/sleep 10.0)  ## parks on a timer far past program end
                                          (println "  BUG: timer orphan resumed")))  ## Let the orphan submit its timer (land in `pending`).
                                        (ev/sleep 0.02)
                                        :program-done)))))
       [ok? result] outcome]
  (assert ok?
          (concat "4a: a timer-parked (pending I/O) orphan aborted at "
                  "teardown must not re-raise its injected :shutdown as a "
                  "user error; ev/run errored with " (string result)))
  (assert (= result :program-done)
          (concat "4b: ev/run must return its thunk's value after reaping a "
                  "timer-parked orphan; got " (string result))))

(println "  PASS: timer-parked orphan's injected :shutdown suppressed")

(println "ev-run-teardown: all tests passed")
