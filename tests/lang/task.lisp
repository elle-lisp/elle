(elle/epoch 12)
# audited: 2026-09-23
# Task: task-await returns a task's value or raises its crash, and leaves the mailbox as it found it.
# docs/behaviors.md

(def process ((import "std/process")))

(process:start (fn []
                 (let* [task (process:task-async (fn [] (* 6 7)))]
                   (assert (= (process:task-await task) 42)
                           "task-await returns the value"))))

(process:start (fn []
                 (let* [t1 (process:task-async (fn [] (+ 10 20)))
                        t2 (process:task-async (fn [] (+ 30 40)))]
                   (assert (= (process:task-await t2) 70)
                           "tasks await in any order: t2")
                   (assert (= (process:task-await t1) 30)
                           "tasks await in any order: t1"))))

# ── a crashed task raises ────────────────────────────────────────────
# The counter-factual: task-await matched :DOWN against a ref from the
# process dictionary's counter, not the monitor's, so it waited for good
# unless the two counters happened to agree. Moving the dictionary counter
# first makes them disagree.

(process:start (fn []
                 (process:put-dict :$gen-call-ref 7)
                 (let* [t (process:task-async (fn []
                          (error {:error :boom :message "task crash"})))
                        [ok? err] (protect (process:task-await t))]
                   (assert (not ok?) "awaiting a crashed task raises")
                   (assert (= (get err :error) :task-error)
                           "the raise is :task-error"))))

(process:start (fn []
                 (let* [t (process:task-async (fn []
                          (process:recv-timeout 3)
                          (error {:error :boom :message "late crash"})))
                        [ok? err] (protect (process:task-await t))]
                   (assert (not ok?) "a task that crashes while awaited raises")
                   (assert (= (get err :error) :task-error) "as :task-error"))))

# ── nothing is left behind ───────────────────────────────────────────
# The counter-factual: the monitor's :DOWN outlived task-await in the
# caller's mailbox, where the caller's next plain recv took it.

(process:start (fn []
                 (let [t (process:task-async (fn [] :value))]
                   (process:task-await t)
                   (assert (= (process:recv-timeout 10) :timeout)
                           "task-await leaves no message behind"))))

# ── a task is monitored, not linked ──────────────────────────────────

(process:start (fn []
                 (process:task-async (fn []
                                       (error {:error :boom :message "unawaited"})))
                 (assert (= (get (process:recv-timeout 10) 0) :DOWN)
                         "an unawaited task's crash arrives as :DOWN, and its caller runs on")))

(println "task: ok")
