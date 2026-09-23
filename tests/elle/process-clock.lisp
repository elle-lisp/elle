(elle/epoch 12)
# audited: 2026-09-23
# The process clock while I/O is in flight: a timer fires during another fiber's I/O, and the wait counts in milliseconds.
# docs/processes.md
# docs/process-scheduler.md
#
# The subject is wall time, so this file reads the clock. Each bound leaves a
# wide margin in the direction a slow machine moves it.

(def process ((import "std/process")))

(defn within [seconds body]
  "Run body in a process scheduler, and return how long it took. Raises when
   the scheduler has not returned after seconds."
  (let [started (clock/monotonic)
        done (ev/timeout seconds
                         (fn []
                           (process:start body)
                           :returned))]
    (assert (= done :returned) "the scheduler returns before its guard")
    (- (clock/monotonic) started)))

# ── a timer fires while another process sleeps ───────────────────────
# The counter-factual: with I/O in flight and no process ready, the scheduler
# waited for the I/O and its clock stood still, so the one-tick timeout
# returned only when the 30-second sleep did.

(def @got nil)
(assert (< (within 20
                   (fn []
                     (let [sleeper (process:spawn (fn [] (ev/sleep 30)))]
                       (assign got (process:recv-timeout 1))
                       (process:exit sleeper :kill)))) 5)
        "a one-tick timeout does not wait for another process's sleep")
(assert (= got :timeout) "the timeout returns :timeout")

# The sleeper is a sub-fiber of the waiting process itself.
(assign got nil)
(assert (< (within 20
                   (fn []
                     (let [sleeper (ev/spawn (fn [] (ev/sleep 30)))]
                       (assign got (process:recv-timeout 1))
                       (ev/abort sleeper)))) 5)
        "a timeout does not wait for the process's own sub-fiber")
(assert (= got :timeout) "the sub-fiber case returns :timeout")

# send-after is a timer too.
(assign got nil)
(within 20
        (fn []
          (let [sleeper (process:spawn (fn [] (ev/sleep 30)))]
            (process:send-after 3 (process:self) :ping)
            (assign got (process:recv))
            (process:exit sleeper :kill))))
(assert (= got :ping) "send-after delivers while another process sleeps")

# ── inside a nested scheduler ────────────────────────────────────────
# The inner scheduler's wait crosses the outer one, which relays both the
# sleep and the scheduler's own wake-up for the timer.

(assign got nil)
(within 20
        (fn []
          (process:start (fn []
                           (let [sleeper (process:spawn (fn [] (ev/sleep 30)))]
                             (assign got (process:recv-timeout 1))
                             (process:exit sleeper :kill))))))
(assert (= got :timeout) "a nested scheduler's timer fires during its I/O")

# ── the clock counts the wait ────────────────────────────────────────
# PID 0 waits 50 ms on its own sleep. The clock counts whole milliseconds from
# when the scheduler starts to wait, which is a little after the sleep starts,
# so the bound sits below 50. The counter-factual: the wait counted nothing,
# and the clock moved by the two rounds around it.

(def @waited nil)
(within 20
        (fn []
          (let [before (process:now)]
            (ev/sleep 0.05)
            (assign waited (- (process:now) before)))))
(assert (>= waited 40)
        (string "a 50 ms sleep moves the clock about 50 ticks, moved " waited))

# Completions that arrive before the timer do not restart its wait. The
# sleeper wakes the scheduler every 10 ms for 200 ms, and PID 0's 100-tick
# timeout falls near the middle. The counter-factual: each completion ended
# the wait without counting it, the clock moved two ticks per sleep, and the
# sleeper's :done arrived first.

(assign got nil)
(within 20
        (fn []
          (let [me (process:self)
                sleeper (process:spawn (fn []
                                         (repeat 20 (ev/sleep 0.01))
                                         (process:send me :done)))]
            (assign got (process:recv-timeout 100))
            (process:exit sleeper :kill))))
(assert (= got :timeout) "a timer counts waits that other completions end")

(println "process-clock: ok")
