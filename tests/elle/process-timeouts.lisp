(elle/epoch 12)
# audited: 2026-09-23
# The :timeout of gen-server-call, gen-server-stop and task-await: a deadline raises, a reply in time wins.
# docs/behaviors.md

(def process ((import "std/process")))

(defn start-server [name]
  "A server that never answers a call, and that blocks for good on a cast."
  (process:gen-server-start-link {:init (fn [_] nil)
                                  :handle-call (fn [_req _from state]
                                    [:noreply state])
                                  :handle-cast (fn [_req state]
                                    (process:recv-match (fn [m] (= m :never)))
                                    [:noreply state])} nil :name name))

(defn start-slow-server [name delay]
  "A server that answers each call after delay ticks."
  (process:gen-server-start-link {:init (fn [_] nil)
                                  :handle-call (fn [_req _from state]
                                    (process:recv-timeout delay)
                                    [:reply delay state])} nil :name name))

# ── a deadline raises ────────────────────────────────────────────────
# The counter-factual: the timeout message never matched the reply
# predicate, so an unanswered call deadlocked the scheduler.

(process:start (fn []
                 (start-server :silent)
                 (let [[ok? err] (protect (process:gen-server-call :silent :ping
                       :timeout 5))]
                   (assert (not ok?) "an unanswered call times out")
                   (assert (= (get err :error) :gen-server-timeout)
                           "with :gen-server-timeout"))))

(process:start (fn []
                 (start-server :busy)
                 (process:gen-server-cast :busy :block)
                 (let [[ok? err] (protect (process:gen-server-stop :busy
                       :timeout 5))]
                   (assert (not ok?) "an unacknowledged stop times out")
                   (assert (= (get err :error) :gen-server-timeout)
                           "with :gen-server-timeout"))))

(process:start (fn []
                 (let* [t (process:task-async (fn [] (process:recv)))
                        [ok? err] (protect (process:task-await t :timeout 5))]
                   (assert (not ok?) "an unfinished task times out")
                   (assert (= (get err :error) :task-timeout)
                           "with :task-timeout")
                   (process:exit (get t 0) :kill))))

# ── the reply that beats the deadline wins, and leaves nothing behind ─
# Each delay runs in a scheduler of its own, so nothing one call leaves in
# the mailbox can reach the next. Across the sweep, the reply lands on both
# sides of the deadline, and on the tick where the timer has fired but the
# reply is ahead of it in the mailbox. The counter-factual for that tick: the
# fired timeout stayed in the mailbox after the call returned.

(def deadline 4)
(def @replies 0)
(def @timeouts 0)
(each delay in (range 0 10)
  (process:start (fn []
                   (start-slow-server :slow delay)
                   (let [[ok? got] (protect (process:gen-server-call :slow :ping
                         :timeout deadline))]
                     (if ok?
                       (begin
                         (assign replies (+ replies 1))
                         (assert (= got delay)
                                 "the call returns the server's reply")
                         (assert (= (process:recv-timeout 20) :timeout)
                                 "a call that got its reply leaves no message behind"))
                       (begin
                         (assign timeouts (+ timeouts 1))
                         (assert (= (get got :error) :gen-server-timeout)
                                 "a late reply is a :gen-server-timeout")))))))
(assert (> replies 0) "some calls in the sweep beat the deadline")
(assert (> timeouts 0) "some calls in the sweep missed it")

(assign replies 0)
(assign timeouts 0)
(each delay in (range 0 10)
  (process:start (fn []
                   (let* [t (process:task-async (fn []
                            (process:recv-timeout delay)
                            delay))
                          [ok? got] (protect (process:task-await t
                          :timeout deadline))]
                     (if ok?
                       (begin
                         (assign replies (+ replies 1))
                         (assert (= got delay)
                                 "task-await returns the task's value"))
                       (begin
                         (assign timeouts (+ timeouts 1))
                         (assert (= (get got :error) :task-timeout)
                                 "a late task is a :task-timeout")))
                     (assert (= (process:recv-timeout 20) :timeout)
                             "task-await leaves no message behind, whether it returned or raised")))))
(assert (> replies 0) "some tasks in the sweep beat the deadline")
(assert (> timeouts 0) "some tasks in the sweep missed it")

# A stop inside the deadline returns :ok.
(process:start (fn []
                 (start-slow-server :stoppable 0)
                 (assert (= (process:gen-server-stop :stoppable :timeout 50) :ok)
                         "a stop inside the deadline returns :ok")))

(println "process-timeouts: ok")
