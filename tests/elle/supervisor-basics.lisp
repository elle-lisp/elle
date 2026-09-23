(elle/epoch 12)
# audited: 2026-09-23
# Supervisor basics: starting children, restart policies, strategies, and children added at runtime.
# docs/behaviors.md

(def process ((import "std/process")))


# ============================================================================
# Supervisor
# ============================================================================

# ── 13. Supervisor starts children ───────────────────────────────────

(process:start (fn []
                 (let [me (process:self)]
                   (process:supervisor-start-link [{:id :worker-a
                   :start (fn []
                            (process:register :worker-a)
                            (process:send me [:started :a])
                            (process:recv))}
                   {:id :worker-b
                    :start (fn []
                             (process:register :worker-b)
                             (process:send me [:started :b])
                             (process:recv))}] :name :sup)

                   (def @started @||)
                   (match (process:recv)
                     [:started id] (put started id)
                     _ nil)
                   (match (process:recv)
                     [:started id] (put started id)
                     _ nil)
                   (assert (has? started :a) "supervisor: worker-a started")
                   (assert (has? started :b) "supervisor: worker-b started"))))
(println "  13. supervisor starts children: ok")


# ── 14. Supervisor restarts permanent child ──────────────────────────

(process:start (fn []
                 (let [me (process:self)]
                   (def @crash-count 0)
                   (process:supervisor-start-link [{:id :fragile
                   :restart :permanent
                   :start (fn []
                            (process:send me [:started (process:self)])
                            (forever
                              (match (process:recv)
                                :crash (error {:error :boom :message "crash"})
                                :ping (process:send me :pong)
                                _ nil)))}] :name :sup3)

                   # Wait for first start
                   (match (process:recv)
                     [:started child-pid]
                       (begin  # Verify child is alive
                         (process:send child-pid :ping)
                         (assert (= :pong (process:recv))
                                 "restart: child responds")

                         # Crash it
                         (process:send child-pid :crash)

                         # Supervisor should restart — wait for new start
                         (match (process:recv)
                           [:started new-pid]
                             (begin
                               (assert (not (= new-pid child-pid))
                                       "restart: new pid differs")
                               (process:send new-pid :ping)
                               (assert (= :pong (process:recv))
                                       "restart: restarted child responds"))
                           _ (assert false
                                     "restart: expected [:started new-pid]")))
                     _ (assert false "restart: expected [:started child-pid]")))))
(println "  14. supervisor restart permanent: ok")


# ── 15. Supervisor does not restart temporary child ──────────────────

(process:start (fn []
                 (let [me (process:self)]
                   (process:supervisor-start-link [{:id :temp
                   :restart :temporary
                   :start (fn []
                            (process:send me [:started (process:self)])
                            (process:recv))}] :name :sup4)

                   (match (process:recv)
                     [:started child-pid]
                       (begin  # Kill the temporary child
                         (process:exit child-pid :kill)

                         # Give supervisor a tick to process the DOWN
                         (process:send me :sync)
                         (process:recv)
                         (process:send me :sync)
                         (process:recv)

                         # No restart expected — send ourselves proof
                         (process:send me :no-restart)
                         (let [msg (process:recv)]
                           (assert (= msg :no-restart)
                                   "temporary: not restarted")))
                     _ (assert false "temporary: expected [:started pid]")))))
(println "  15. supervisor temporary child: ok")


# ── 16. Supervisor transient child — normal exit not restarted ───────

(process:start (fn []
                 (let [me (process:self)]
                   (process:supervisor-start-link [{:id :trans
                   :restart :transient
                   :start (fn []
                            (process:send me [:started (process:self)])  # Exit normally after receiving :go
                            (process:recv)
                            :done)}] :name :sup5)

                   (match (process:recv)
                     [:started child-pid]
                       (begin
                         (process:send child-pid :go)

                         # Give supervisor time to process
                         (process:send me :sync)
                         (process:recv)
                         (process:send me :sync)
                         (process:recv)

                         (process:send me :no-restart)
                         (let [msg (process:recv)]
                           (assert (= msg :no-restart)
                                   "transient-normal: not restarted")))
                     _ (assert false "transient: expected [:started pid]")))))
(println "  16. supervisor transient normal exit: ok")


# ── 17. GenServer as supervised child ─────────────────────────────────

(process:start (fn []
                 (let [me (process:self)]
                   (process:supervisor-start-link [{:id :kv
                   :restart :permanent
                   :start (fn []
                            (process:send me :kv-ready)  # Run a genserver loop inline
                            (process:register :kv-sup)
                            (def @state @{})
                            (forever
                              (let [msg (process:recv)]
                                (match msg
                                  [:$call caller ref request]
                                    (match request
                                      [:get key]
                                        (process:send caller
                                        [:$reply ref (get state key nil)])
                                      [:put key val]
                                        (begin
                                          (put state key val)
                                          (process:send caller [:$reply ref :ok]))
                                      _ nil)
                                  _ nil))))}])

                   (process:recv)  # :kv-ready

                   (process:gen-server-call :kv-sup [:put :lang "elle"])
                   (let [val (process:gen-server-call :kv-sup [:get :lang])]
                     (assert (= val "elle") "supervised genserver: got elle")))))
(println "  17. genserver under supervisor: ok")


# ============================================================================
# Supervisor strategies
# ============================================================================

# ── 20. one-for-all strategy ─────────────────────────────────────────

(process:start (fn []
                 (let [me (process:self)]
                   (def @starts @[])
                   (process:supervisor-start-link [{:id :a
                   :restart :permanent
                   :start (fn []
                            (process:send me [:started :a (process:self)])
                            (forever
                              (match (process:recv)
                                :crash (error {:error :boom :message "a"})
                                _ nil)))}
                   {:id :b
                    :restart :permanent
                    :start (fn []
                             (process:send me [:started :b (process:self)])
                             (forever
                               (match (process:recv)
                                 _ nil)))}] :name :ofa-sup :strategy
                   :one-for-all)

                   # Wait for both to start
                   (match (process:recv)
                     [:started id pid] (push starts [id pid])
                     _ nil)
                   (match (process:recv)
                     [:started id pid] (push starts [id pid])
                     _ nil)
                   (assert (= (length starts) 2) "one-for-all: both started")

                   # Crash child :a — both should restart
                   (let [a-pid (get (get starts 0) 1)]
                     (when (= (get (get starts 0) 0) :a)
                       (process:send a-pid :crash))
                     (when (= (get (get starts 1) 0) :a)
                       (process:send (get (get starts 1) 1) :crash)))

                   # Wait for both restarts
                   (def @restarts @[])
                   (match (process:recv)
                     [:started id pid] (push restarts id)
                     _ nil)
                   (match (process:recv)
                     [:started id pid] (push restarts id)
                     _ nil)
                   (assert (= (length restarts) 2) "one-for-all: both restarted"))))
(println "  20. one-for-all strategy: ok")


# ── 21. rest-for-one strategy ────────────────────────────────────────

(process:start (fn []
                 (let [me (process:self)]
                   (process:supervisor-start-link [{:id :x
                   :restart :permanent
                   :start (fn []
                            (process:send me [:started :x (process:self)])
                            (forever
                              (match (process:recv)
                                :crash (error {:error :b :message "x"})
                                _ nil)))}
                   {:id :y
                    :restart :permanent
                    :start (fn []
                             (process:send me [:started :y (process:self)])
                             (forever
                               (match (process:recv)
                                 _ nil)))}
                   {:id :z
                    :restart :permanent
                    :start (fn []
                             (process:send me [:started :z (process:self)])
                             (forever
                               (match (process:recv)
                                 _ nil)))}] :name :rfo-sup :strategy
                   :rest-for-one)

                   # Wait for all 3 to start
                   (def @pids @{})
                   (repeat 3
                           (match (process:recv)
                             [:started id pid] (put pids id pid)
                             _ nil))

                   # Crash :x — :x, :y, :z should all restart (x is first, rest-for-one restarts everything after)
                   (process:send (get pids :x) :crash)

                   # Wait for 3 restarts
                   (def @restarts @||)
                   (repeat 3
                           (match (process:recv)
                             [:started id _pid] (put restarts id)
                             _ nil))
                   (assert (has? restarts :x) "rest-for-one: x restarted")
                   (assert (has? restarts :y) "rest-for-one: y restarted")
                   (assert (has? restarts :z) "rest-for-one: z restarted"))))
(println "  21. rest-for-one strategy: ok")


# ============================================================================
# DynamicSupervisor
# ============================================================================

# ── 22. Add/remove children at runtime ───────────────────────────────

(process:start (fn []
                 (let [me (process:self)]
                   (process:supervisor-start-link [] :name :dyn-sup)

                   # Start with no children
                   (let [kids (process:supervisor-which-children :dyn-sup)]
                     (assert (= (length kids) 0) "dynamic: starts empty"))

                   # Add a child
                   (let [pid (process:supervisor-start-child :dyn-sup {:id :dyn-worker
                         :restart :temporary
                         :start (fn []
                                  (process:send me [:started (process:self)])
                                  (forever
                                    (match (process:recv)
                                      _ nil)))})]
                     (match (process:recv)
                       [:started _pid] nil
                       _ nil)

                     (let [kids (process:supervisor-which-children :dyn-sup)]
                       (assert (= (length kids) 1) "dynamic: one child"))

                     # Remove it
                     (process:supervisor-stop-child :dyn-sup :dyn-worker)

                     # Give time to process
                     (process:send me :sync)
                     (process:recv)

                     (let [kids (process:supervisor-which-children :dyn-sup)]
                       (assert (= (length kids) 0) "dynamic: back to empty"))))))
(println "  22. dynamic supervisor: ok")


(println "supervisor-basics: ok")
