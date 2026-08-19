(elle/epoch 12)
## tests/elle/process-select.lisp — ev/select and ev/timeout inside processes.
##
## The process scheduler routes a :wait signal two ways: a request from the
## process fiber goes through handle-wait, one from a sub-fiber through the
## sub-fiber dispatch. Both must implement :select, and neither may resume a
## fiber that is parked on I/O or a futex just to "pump" it — a parked fiber
## resumed with nil reads that nil as its operation's result. A timer fiber
## parked in ev/sleep that is pumped this way completes instantly, so every
## ev/timeout inside a process would report its deadline at once.
##
## Run: elle tests/elle/process-select.lisp

(def process ((import "std/process")))

(println "tests/elle/process-select.lisp:")

## ── 1. ev/select from the process fiber ───────────────────────────────

(process:start (fn []
                 (let* [f1 (ev/spawn (fn [] :one))
                        f2 (ev/spawn (fn [] :two))
                        [done remaining] (ev/select [f1 f2])]
                   (assert (not (nil? done)) "select: got a fiber")
                   (assert (= (length remaining) 1) "select: one remains"))))
(println "  1. ev/select from process fiber: ok")

## ── 2. ev/select from a sub-fiber ─────────────────────────────────────

(def @sub-select-done nil)
(process:start (fn []
                 (ev/join (ev/spawn (fn []
                                      (let* [f1 (ev/spawn (fn [] :one))
                                        f2 (ev/spawn (fn [] :two))
                                        [done _] (ev/select [f1 f2])]
                                        (assign sub-select-done done)))))))
(assert (not (nil? sub-select-done)) "sub-fiber select: got a fiber")
(println "  2. ev/select from sub-fiber: ok")

## ── 3. ev/timeout from a sub-fiber returns the body's value ───────────

(def @sub-timeout-result nil)
(process:start (fn []
                 (ev/join (ev/spawn (fn []
                                      (assign
                                        sub-timeout-result
                                        (ev/timeout 5 (fn [] :done))))))))
(assert (= sub-timeout-result :done) "sub-fiber timeout: body value")
(println "  3. ev/timeout from sub-fiber: ok")

## ── 4. ev/timeout waits out its deadline before reporting it ──────────
## The body parks in a long sleep, so the timer decides. A pumped —
## spuriously resumed — timer completes at once and reports the deadline
## after microseconds; a working one holds it for the full 0.3 s.

(def @deadline-result :unset)
(def @deadline-elapsed 0)
(process:start (fn []
                 (let [t0 (clock/monotonic)]
                   (assign
                     deadline-result
                     (ev/timeout 0.3
                                 (fn []
                                   (ev/sleep 30)
                                   :late)))
                   (assign deadline-elapsed (- (clock/monotonic) t0)))))
(assert (= deadline-result nil) "deadline: reports nil")
(assert (>= deadline-elapsed 0.15)
        (concat "deadline: waited for the timer, elapsed "
                (string deadline-elapsed)))
(println "  4. ev/timeout waits out its deadline in a process: ok")

## ── 5. ev/timeout body wins while the timer is parked ─────────────────
## The body parks briefly and still wins; the parked timer must neither
## fire early nor block the body's completion.

(def @race-result nil)
(process:start (fn []
                 (assign
                   race-result
                   (ev/timeout 30
                               (fn []
                                 (ev/sleep 0.05)
                                 :won)))))
(assert (= race-result :won) "body wins: got the body's value")
(println "  5. ev/timeout body wins over a parked timer: ok")

## ── 6. ev/timeout from a sub-fiber waits out its deadline ─────────────

(def @sub-deadline-result :unset)
(def @sub-deadline-elapsed 0)
(process:start (fn []
                 (ev/join (ev/spawn (fn []
                                      (let [t0 (clock/monotonic)]
                                        (assign
                                          sub-deadline-result
                                          (ev/timeout 0.3
                                          (fn []
                                            (ev/sleep 30)
                                            :late)))
                                        (assign
                                          sub-deadline-elapsed
                                          (- (clock/monotonic) t0))))))))
(assert (= sub-deadline-result nil) "sub deadline: reports nil")
(assert (>= sub-deadline-elapsed 0.15)
        (concat "sub deadline: waited for the timer, elapsed "
                (string sub-deadline-elapsed)))
(println "  6. ev/timeout from sub-fiber waits out its deadline: ok")

(println "")
(println "tests/elle/process-select.lisp: all tests passed")
