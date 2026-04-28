(elle/epoch 9)
## tests/elle/supervisor.lisp — Tests for supervisor improvements
##
## Tests for: max-restarts, logger, make-subprocess-child,
## and edge cases from process-criticisms.md.
##
## Run: ./target/debug/elle tests/elle/supervisor.lisp

(def process ((import-file "lib/process.lisp")))
(def backend (*io-backend*))

(def process:start-raw process:start)
(defn process:start [init &named fuel]
  (process:start-raw init :fuel fuel :backend backend))


# ============================================================================
# 1. Supervisor logger receives lifecycle events
# ============================================================================

(process:start (fn []
                 (let [me (process:self)
                       events @[]]
                   (process:supervisor-start-link [{:id :logged-child
                     :restart :temporary
                     :start (fn []
                              (process:send me :started)
                              (process:recv))}]
                     :name
                     :log-sup
                     :logger (fn [event] (process:send me [:log event])))

                   # Wait for child start
                   (process:recv)  # :started

                   # Collect the :child-started log event
                   (let [msg (process:recv)]
                     (match msg
                       [:log event]
                         (begin
                           (assert (= (get event :event) :child-started)
                                   "logger: got child-started event")
                           (assert (= (get event :id) :logged-child)
                                   "logger: correct child id"))
                       _ (assert false "logger: expected log event"))))))
(println "  1. supervisor logger: ok")


# ============================================================================
# 2. Logger receives exit and restart events
# ============================================================================

(process:start (fn []
                 (let [me (process:self)]
                   (process:supervisor-start-link [{:id :crasher
                     :restart :permanent
                     :start (fn []
                              (process:send me :started)
                              (match (process:recv)
                                :crash (error {:error :boom :message "crash"})
                                _ nil))}]
                     :name
                     :log-sup2
                     :logger (fn [event] (process:send me [:log event])))

                   # Initial start
                   (process:recv)  # :started
                   (process:recv)  # [:log {:event :child-started ...}]

                   # Crash the child
                   (let [pid (process:whereis :log-sup2)]
                     (let [kids (process:supervisor-which-children :log-sup2)]
                       (process:send (get (get kids 0) :pid) :crash)))

                   # Should get: exit log, restarting log, started log, then :started msg
                   (def @got-exit false)
                   (def @got-restarting false)
                   (def @count 0)
                   (while (< count 10)
                     (let [msg (process:recv)]
                       (match msg
                         [:log event]
                           (begin
                             (when (= (get event :event) :child-exited)
                               (assign got-exit true))
                             (when (= (get event :event) :child-restarting)
                               (assign got-restarting true)))
                         :started (assign count 10)  # break
                         _ nil))
                     (assign count (+ count 1)))
                   (assert got-exit "logger: got child-exited event")
                   (assert got-restarting "logger: got child-restarting event"))))
(println "  2. logger exit/restart events: ok")


# ============================================================================
# 3. Max restarts — supervisor stops restarting after limit
# ============================================================================

(process:start (fn []
                 (let [me (process:self)]
                   (def @start-count 0)
                   (process:supervisor-start-link [{:id :fragile
                     :restart :permanent
                     :start (fn []
                              (process:send me :started)  # Crash immediately
                              (error {:error :always-crash :message "always"}))}]
                     :name
                     :max-sup
                     :max-restarts 3
                     :max-ticks 100  # wide window so all restarts count
                     :logger (fn [event]
                               (when (= (get event :event) :max-restarts-reached)
                                 (process:send me [:max-reached]))))

                   # Count starts — should get initial + up to 3 restarts = 4 total max
                   (def @starts 0)
                   (def @max-reached false)
                   (def @done false)
                   (while (not done)
                     (match (process:recv-timeout 5)
                       :started (assign starts (+ starts 1))
                       [:max-reached] (begin
                                        (assign max-reached true)
                                        (assign done true))
                       :timeout (assign done true)
                       _ nil))
                   (assert max-reached
                           "max-restarts: intensity limit was reached")
                   (assert (<= starts 5) "max-restarts: did not spin forever"))))
(println "  3. max-restarts limit: ok")


# ============================================================================
# 4. Permanent child with normal exit — should restart
# ============================================================================

(process:start (fn []
                 (let [me (process:self)]
                   (process:supervisor-start-link [{:id :normal-exiter
                     :restart :permanent
                     :start (fn []
                              (process:send me [:started (process:self)])
                              (match (process:recv)
                                :exit-normally :done  # return normally
                                _ nil))}]
                     :name
                     :perm-normal-sup)

                   # Wait for first start
                   (match (process:recv)
                     [:started pid1]
                       (begin  # Tell it to exit normally
                         (process:send pid1 :exit-normally)

                         # Permanent child should restart even on normal exit
                         (match (process:recv)
                           [:started pid2]
                             (assert (not (= pid1 pid2))
                                     "permanent-normal: restarted with new pid")
                           _ (assert false "permanent-normal: expected restart")))
                     _ (assert false "permanent-normal: expected initial start")))))
(println "  4. permanent child normal exit restarts: ok")


# ============================================================================
# 5. Transient child with crash — should restart
# ============================================================================

(process:start (fn []
                 (let [me (process:self)]
                   (process:supervisor-start-link [{:id :trans-crash
                     :restart :transient
                     :start (fn []
                              (process:send me [:started (process:self)])
                              (match (process:recv)
                                :crash (error {:error :boom :message "crash"})
                                _ nil))}]
                     :name
                     :trans-crash-sup)
                   (match (process:recv)
                     [:started pid1]
                       (begin
                         (process:send pid1 :crash)
                         (match (process:recv)
                           [:started pid2]
                             (assert (not (= pid1 pid2))
                                     "transient-crash: restarted")
                           _ (assert false "transient-crash: expected restart")))
                     _ (assert false "transient-crash: expected initial start")))))
(println "  5. transient child crash restarts: ok")


# ============================================================================
# 6. Child start function that throws — supervisor handles gracefully
# ============================================================================

(process:start (fn []
                 (let [me (process:self)]
                   (def @attempts 0)
                   (process:supervisor-start-link [{:id :bad-start
                     :restart :permanent
                     :start (fn []
                              (process:send me :attempt)
                              (error {:error :start-failed :message "bad config"}))}]
                     :name
                     :bad-start-sup
                     :max-restarts 2
                     :max-ticks 100
                     :logger (fn [event]
                               (when (= (get event :event) :max-restarts-reached)
                                 (process:send me :max-reached))))

                   # Child crashes on startup, supervisor retries, hits max-restarts
                   (def @attempt-count 0)
                   (def @got-max false)
                   (def @done false)
                   (while (not done)
                     (match (process:recv-timeout 5)
                       :attempt (assign attempt-count (+ attempt-count 1))
                       :max-reached (begin
                                      (assign got-max true)
                                      (assign done true))
                       :timeout (assign done true)
                       _ nil))
                   (assert got-max "bad-start: max-restarts triggered")
                   (assert (<= attempt-count 4) "bad-start: bounded attempts"))))
(println "  6. child start failure + max-restarts: ok")


# ============================================================================
# 7. Two children crash simultaneously under one-for-all
# ============================================================================

(process:start (fn []
                 (let [me (process:self)]
                   (process:supervisor-start-link [{:id :a
                     :restart :permanent
                     :start (fn []
                              (process:send me [:started :a (process:self)])
                              (forever
                                (match (process:recv)
                                  :crash (error {:error :a :message "a"})
                                  _ nil)))}
                     {:id :b
                      :restart :permanent
                      :start (fn []
                               (process:send me [:started :b (process:self)])
                               (forever
                                 (match (process:recv)
                                   :crash (error {:error :b :message "b"})
                                   _ nil)))}]
                     :name
                     :dual-crash-sup
                     :strategy
                     :one-for-all)

                   # Wait for both to start
                   (def @pids @{})
                   (repeat 2
                           (match (process:recv)
                             [:started id pid] (put pids id pid)
                             _ nil))

                   # Crash both rapidly
                   (process:send (get pids :a) :crash)
                   (process:send (get pids :b) :crash)

                   # Should get restarts for both (one-for-all restarts all)
                   (def @restarts @||)
                   (def @count 0)
                   (while (< count 4)
                     (match (process:recv-timeout 5)
                       [:started id _pid] (put restarts id)
                       :timeout (assign count 4)
                       _ nil)
                     (assign count (+ count 1)))
                   (assert (has? restarts :a) "dual-crash: a restarted")
                   (assert (has? restarts :b) "dual-crash: b restarted"))))
(println "  7. simultaneous crashes one-for-all: ok")


# ============================================================================
# 8. Logger with no restarts configured (default: unbounded)
# ============================================================================

(process:start (fn []
                 (let [me (process:self)]
                   (def @crash-count 0)
                   (process:supervisor-start-link [{:id :unlimited
                     :restart :permanent
                     :start (fn []
                              (process:send me :started)
                              (if (< crash-count 5)
                                (begin
                                  (assign crash-count (+ crash-count 1))
                                  (error {:error :crash :message "crash"}))
                                (process:recv)))}]  # stay alive after 5 crashes
                     :name
                     :no-limit-sup)

                   # Should restart 5 times without hitting any limit
                   (def @starts 0)
                   (def @done false)
                   (while (not done)
                     (match (process:recv-timeout 5)
                       :started
                         (begin
                           (assign starts (+ starts 1))
                           (when (>= starts 6) (assign done true)))
                       :timeout (assign done true)
                       _ nil))
                   (assert (= starts 6)
                           "no-limit: all 6 starts happened (1 initial + 5 restarts)")))
               :fuel 5000)
(println "  8. unbounded restarts (default): ok")


# ============================================================================
# 9. Readiness protocol — supervisor waits for child to signal ready
# ============================================================================

(process:start (fn []
                 (let [me (process:self)]
                   (process:trap-exit true)
                   (process:supervisor-start-link [{:id :bridge
                     :restart :permanent
                     :ready true
                     :start (fn []
                              (process:send me [:starting :bridge])
                              (process:supervisor-notify-ready)
                              (process:send me [:ready :bridge])
                              (forever (process:recv)))}
                     {:id :client
                      :restart :permanent
                      :start (fn []
                               (process:send me [:starting :client])
                               (forever (process:recv)))}]
                     :name
                     :ready-sup)

                   # Collect events — wait for all 3 messages
                   (def @events @[])
                   (def @count 0)
                   (while (< count 3)
                     (match (process:recv-timeout 50)
                       [:starting id] (push events [:starting id])
                       [:ready id] (push events [:ready id])
                       :timeout (assign count 3)  # break on timeout
                       _ nil)
                     (assign count (+ count 1)))

                   # Verify ordering: bridge starts, bridge ready, THEN client starts
                   (assert (>= (length events) 3) "ready: got all 3 events")
                   (assert (= (get events 0) [:starting :bridge])
                           "ready: bridge starts first")
                   (assert (= (get events 1) [:ready :bridge])
                           "ready: bridge becomes ready")
                   (assert (= (get events 2) [:starting :client])
                           "ready: client starts after bridge ready")

                   # Cleanup
                   (let [sup-pid (process:whereis :ready-sup)]
                     (process:exit sup-pid :shutdown))))
               :fuel 5000)
(println "  9. readiness protocol: ok")


# ============================================================================
# 10. Readiness — child crashes before ready, supervisor doesn't deadlock
# ============================================================================

(process:start (fn []
                 (let [me (process:self)]
                   (process:supervisor-start-link [{:id :crash-before-ready
                     :restart :temporary
                     :ready true
                     :start (fn []  # Crash before signaling ready
                            (error {:error :init-failed :message "can't start"}))}]
                     :name
                     :crash-ready-sup
                     :logger (fn [event] (process:send me [:log event])))

                   # Supervisor should not deadlock — it should detect the child death
                   # and proceed. Collect some events to prove it didn't hang.
                   (def @got-exit false)
                   (def @count 0)
                   (while (< count 5)
                     (match (process:recv-timeout 3)
                       [:log event]
                         (begin
                           (when (= (get event :event) :child-exited)
                             (assign got-exit true)
                             (assign count 5)))  # break
                         :timeout (assign count 5)
                       _ nil)
                     (assign count (+ count 1)))
                   (assert got-exit
                           "crash-before-ready: supervisor detected death, no deadlock"))))
(println "  10. crash before ready (no deadlock): ok")


(println "")
(println "all supervisor tests passed.")
