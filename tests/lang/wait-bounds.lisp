(elle/epoch 14)
# audited: 2026-09-30
## io/wait, ev/step and ev/shutdown take :timeout and :deadline: no bound waits, :timeout 0 polls.
## docs/io/timeout.md
##
## Each case runs a loop of its own, driven one step at a time the way an
## embedding host drives one (docs/embedding.md), so the step under test is the
## only thing that waits.

(def late 2.0)  # s, above this a wait did not end at its own bound

(defn timed [thunk]
  "Run `thunk` under protect. Returns [ok? result elapsed-seconds]."
  (let* [started (clock/monotonic)
         [ok? result] (protect (thunk))
         elapsed (- (clock/monotonic) started)]
    [ok? result elapsed]))

(defn sleep-request [backend seconds]
  "Submit a sleep of `seconds` to `backend` from a fiber of no scheduler."
  (let [f (fiber/new (fn [] (ev/sleep seconds)) 512)]
    (fiber/resume f)
    (io/submit backend (fiber/value f))))

(defn with-own-loop [body]
  "Run `body` with a fresh scheduler of its own installed as the current one."
  (let [sched (make-async-scheduler)]
    (parameterize ((*scheduler* sched)
                   (*spawn* (get sched :spawn))
                   (*shutdown* (get sched :shutdown))
                   (*io-backend* (get sched :backend)))
      (body sched))))

## ── 1. io/wait ───────────────────────────────────────────────────────

(let [backend (io/backend :async)]
  (sleep-request backend 1)
  (let [[ok? got elapsed] (timed (fn [] (io/wait backend :timeout 0)))]
    (assert ok? (concat "io/wait takes :timeout, got " (string got)))
    (assert (= (length got) 0) ":timeout 0 polls: the sleep has not finished")
    (assert (< elapsed late) "and does not wait"))
  (assert (= (length (io/wait backend :deadline (- (clock/monotonic) 1))) 0)
          "a past deadline polls")
  (assert (= (length (io/wait backend :timeout 0.05)) 0)
          "a short :timeout ends the wait before the sleep finishes")
  (assert (= (length (io/wait backend :deadline (+ (clock/monotonic) 0.05))) 0)
          "so does a near :deadline")
  (let [[ok? got elapsed] (timed (fn [] (io/wait backend)))]
    (assert ok? (concat "io/wait takes no bound, got " (string got)))
    (assert (= (length got) 1) "io/wait with no bound waits for the completion")))

(let [backend (io/backend :async)]
  (assert (let [[ok? _] (protect (io/wait backend :timeout -1))]
            (not ok?)) "io/wait refuses a negative :timeout")
  (assert (let [[ok? _] (protect (io/wait backend :deadline "now"))]
            (not ok?)) "io/wait refuses a :deadline that is not a number"))

(println "  1. io/wait: no bound waits, :timeout 0 polls")

## ── 2. ev/step ───────────────────────────────────────────────────────
##
## The fiber sleeps. A step that polls finds it still asleep and answers
## :pending at once; a step with no bound waits for the sleep, so the loop
## finishes in a few steps rather than spinning.

(with-own-loop (fn [sched]
                 (ev/spawn (fn []
                             (ev/sleep 0.2)
                             :woke))
                 (let [[ok? status elapsed] (timed (fn [] (ev/step :timeout 0)))]
                   (assert ok?
                           (concat "ev/step takes :timeout, got "
                                   (string status)))
                   (assert (= status :pending)
                           "a polling step leaves the sleeper pending")
                   (assert (< elapsed late) "and does not wait"))
                 (assert (= (ev/step :deadline (- (clock/monotonic) 1)) :pending)
                         "a step past its deadline polls")
                 (let [started (clock/monotonic)
                       @steps 0
                       @status :pending]
                   (while (and (= status :pending) (< steps 10))
                     (assign status (ev/step))
                     (assign steps (+ steps 1)))
                   (assert (= status :done)
                           (concat "steps with no bound finish the loop, got "
                                   (string status) " after " (string steps)
                                   " steps"))
                   (assert (>= (- (clock/monotonic) started) 0.15)
                           "by waiting for the sleep"))))

(with-own-loop (fn [sched]
                 (ev/spawn (fn [] (ev/sleep 0.2)))
                 (assert (= ((get sched :step) :timeout 0) :pending)
                         "the scheduler's own step takes the same bounds")
                 (let [@status :pending]
                   (while (= status :pending)
                     (assign
                       status
                       ((get sched :step) :deadline (+ (clock/monotonic) 5))))
                   (assert (= status :done)
                           "a step bounded by a deadline also finishes"))))

(println "  2. ev/step: no bound waits, :timeout 0 polls")

## ── 3. ev/shutdown ───────────────────────────────────────────────────
##
## The fiber catches the :shutdown error its abort injects, then does one more
## wait before it records that it unwound. A bound gives that wait room to
## finish. With no bound there is no grace, and the loop cancels the fiber in
## the middle of it.

(defn unwound-under [& bounds]
  "Whether a fiber aborted by (ev/shutdown ;bounds) finishes unwinding."
  (let [log @[:running]]
    (with-own-loop (fn [sched]
                     (ev/spawn (fn []
                                 (protect (ev/sleep 10))
                                 (ev/sleep 0.05)
                                 (put log 0 :unwound)))
                     (ev/step :timeout 0)
                     (ev/shutdown ;bounds)
                     (let [@status :pending]
                       (while (= status :pending)
                         (assign status (ev/step :timeout 0))))))
    (= (get log 0) :unwound)))

(assert (unwound-under :timeout 2)
        "a :timeout gives aborted fibers time to unwind")
(assert (unwound-under :deadline (+ (clock/monotonic) 2)) "so does a :deadline")
(assert (not (unwound-under)) "with no bound, the loop cancels them at once")

(println "  3. ev/shutdown: the bound is the grace period")

(println "wait-bounds: all tests passed")
