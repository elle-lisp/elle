## tests/elle/genserver.lisp — Tests for GenServer, Agent, and Supervisor
##
## Run: ./target/debug/elle tests/elle/genserver.lisp

(def process ((import-file "lib/process.lisp")))
(def backend (*io-backend*))

(defn process:start [init &named fuel]
  (process:start-raw init :fuel fuel :backend backend))
(def process:start-raw process:start)
(defn process:start [init &named fuel]
  (process:start-raw init :fuel fuel :backend backend))


# ============================================================================
# GenServer
# ============================================================================

# ── 1. Basic call/reply ───────────────────────────────────────────────

(process:start (fn []
  (let ([pid (process:gen-server-start-link
               {:init        (fn [arg] arg)
                :handle-call (fn [request _from state]
                  (match request
                    (:get     [:reply state state])
                    ([:set v] [:reply :ok v])
                    (_        [:reply :unknown state])))}
               42)])
    (assert (= 42 (process:gen-server-call pid :get)) "call: get initial state")
    (process:gen-server-call pid [:set 99])
    (assert (= 99 (process:gen-server-call pid :get)) "call: state updated"))))
(println "  1. basic call/reply: ok")


# ── 2. Named server ──────────────────────────────────────────────────

(process:start (fn []
  (process:gen-server-start-link
    {:init        (fn [_] 0)
     :handle-call (fn [request _from state]
       (case request
         :inc   [:reply (+ state 1) (+ state 1)]
         :get   [:reply state state]))}
    nil :name :counter)

  (process:gen-server-call :counter :inc)
  (process:gen-server-call :counter :inc)
  (process:gen-server-call :counter :inc)
  (assert (= 3 (process:gen-server-call :counter :get)) "named: counter is 3")))
(println "  2. named server: ok")


# ── 3. Cast (async) ──────────────────────────────────────────────────

(process:start (fn []
  (process:gen-server-start-link
    {:init        (fn [_] @[])
     :handle-call (fn [request _from state]
       (case request
         :get [:reply (freeze state) state]))
     :handle-cast (fn [request state]
       (match request
         ([:push val] (push state val) [:noreply state])
         (_           [:noreply state])))}
    nil :name :log)

  (process:gen-server-cast :log [:push :a])
  (process:gen-server-cast :log [:push :b])
  (process:gen-server-cast :log [:push :c])
  # call to sync — ensures casts have been processed
  (let ([result (process:gen-server-call :log :get)])
    (assert (= result [:a :b :c]) "cast: three items logged"))))
(println "  3. cast: ok")


# ── 4. Stop with terminate callback ──────────────────────────────────

(process:start (fn []
  (let ([me (process:self)])
    (process:gen-server-start-link
      {:init        (fn [_] :running)
       :handle-call (fn [request _from state]
         [:reply state state])
       :terminate   (fn [reason state]
         (process:send me [:terminated reason state]))}
      nil :name :stoppable)

    (assert (= :running (process:gen-server-call :stoppable :status))
            "stop: server running")
    (process:gen-server-stop :stoppable :reason :shutdown)
    (let ([msg (process:recv)])
      (match msg
        ([:terminated reason state]
          (assert (= reason :shutdown) "stop: reason is :shutdown")
          (assert (= state :running) "stop: state passed to terminate"))
        (_ (assert false "stop: expected terminated message")))))))
(println "  4. stop + terminate: ok")


# ── 5. handle-info for non-protocol messages ──────────────────────────

(process:start (fn []
  (let ([me (process:self)])
    (let ([pid (process:gen-server-start-link
                 {:init        (fn [_] @[])
                  :handle-call (fn [request _from state]
                    (case request
                      :get [:reply (freeze state) state]))
                  :handle-info (fn [msg state]
                    (push state msg)
                    [:noreply state])}
                 nil)])

      # Send raw messages (not $call/$cast)
      (process:send pid :hello)
      (process:send pid :world)
      # Sync via call
      (let ([result (process:gen-server-call pid :get)])
        (assert (= result [:hello :world]) "handle-info: captured messages"))))))
(println "  5. handle-info: ok")


# ── 6. Init with [:ok state] form ────────────────────────────────────

(process:start (fn []
  (let ([pid (process:gen-server-start-link
               {:init        (fn [arg] [:ok (* arg 10)])
                :handle-call (fn [request _from state]
                  [:reply state state])}
               5)])
    (assert (= 50 (process:gen-server-call pid :get)) "init [:ok state]: state is 50"))))
(println "  6. init [:ok state]: ok")


# ── 7. Deferred reply via gen-server-reply ────────────────────────────

(process:start (fn []
  (let ([pid (process:gen-server-start-link
               {:init        (fn [_] nil)
                :handle-call (fn [request from state]
                  # Stash the caller, reply later from handle-info
                  [:noreply from])
                :handle-info (fn [msg state]
                  # state is the stashed [pid ref] from the call
                  (process:gen-server-reply state msg)
                  [:noreply nil])}
               nil)])
    # Send a call, then poke the server with a raw message
    (process:spawn (fn []
      (process:send pid :the-answer)))
    (let ([result (process:gen-server-call pid :anything)])
      (assert (= result :the-answer) "deferred reply: got :the-answer")))))
(println "  7. deferred reply: ok")


# ── 8. Stop from handle-call ─────────────────────────────────────────

(process:start (fn []
  (process:trap-exit true)
  (let ([pid (process:gen-server-start-link
               {:init        (fn [_] :alive)
                :handle-call (fn [request _from state]
                  (case request
                    :die [:stop :killed :goodbye state]
                    [:reply state state]))}
               nil)])
    (let ([reply (process:gen-server-call pid :die)])
      (assert (= reply :goodbye) "stop-from-call: got goodbye"))
    (match (process:recv)
      ([:EXIT _ _] (assert true "stop-from-call: got EXIT"))
      (_ (assert false "stop-from-call: expected EXIT"))))))
(println "  8. stop from handle-call: ok")


# ── 9. Stop from handle-cast ─────────────────────────────────────────

(process:start (fn []
  (process:trap-exit true)
  (let ([pid (process:gen-server-start-link
               {:init        (fn [_] nil)
                :handle-cast (fn [request state]
                  [:stop :cast-shutdown state])}
               nil)])
    (process:gen-server-cast pid :bye)
    (match (process:recv)
      ([:EXIT _ _] (assert true "stop-from-cast: got EXIT"))
      (_ (assert false "stop-from-cast: expected EXIT"))))))
(println "  9. stop from handle-cast: ok")


# ============================================================================
# Actor
# ============================================================================

# ── 10. Actor get/update ──────────────────────────────────────────────

(process:start (fn []
  (process:actor-start-link (fn [] 0) :name :counter)
  (assert (= 0 (process:actor-get :counter (fn [s] s))) "actor: initial 0")
  (process:actor-update :counter (fn [s] (+ s 1)))
  (process:actor-update :counter (fn [s] (+ s 1)))
  (process:actor-update :counter (fn [s] (+ s 1)))
  (assert (= 3 (process:actor-get :counter (fn [s] s))) "actor: 3 after 3 incs")))
(println "  10. actor get/update: ok")


# ── 11. Actor async cast ─────────────────────────────────────────────

(process:start (fn []
  (process:actor-start-link (fn [] @[]) :name :items)
  (process:actor-cast :items (fn [s] (push s :x) s))
  (process:actor-cast :items (fn [s] (push s :y) s))
  # sync to drain
  (let ([result (process:actor-get :items (fn [s] (freeze s)))])
    (assert (= result [:x :y]) "actor-cast: items are [:x :y]"))))
(println "  11. actor cast: ok")


# ── 12. Actor derived read ───────────────────────────────────────────

(process:start (fn []
  (process:actor-start-link (fn [] {:name "elle" :version 1}) :name :meta)
  (assert (= "elle" (process:actor-get :meta (fn [s] (get s :name))))
          "actor: derived read :name")
  (assert (= 1 (process:actor-get :meta (fn [s] (get s :version))))
          "actor: derived read :version")))
(println "  12. actor derived read: ok")


# ============================================================================
# Supervisor
# ============================================================================

# ── 13. Supervisor starts children ───────────────────────────────────

(process:start (fn []
  (let ([me (process:self)])
    (process:supervisor-start-link
      [{:id :worker-a :start (fn []
         (process:register :worker-a)
         (process:send me [:started :a])
         (process:recv))}
       {:id :worker-b :start (fn []
         (process:register :worker-b)
         (process:send me [:started :b])
         (process:recv))}]
      :name :sup)

    (var started @||)
    (match (process:recv) ([:started id] (put started id)) (_ nil))
    (match (process:recv) ([:started id] (put started id)) (_ nil))
    (assert (has? started :a) "supervisor: worker-a started")
    (assert (has? started :b) "supervisor: worker-b started"))))
(println "  13. supervisor starts children: ok")


# ── 14. Supervisor restarts permanent child ──────────────────────────

(process:start (fn []
  (let ([me (process:self)])
    (var crash-count 0)
    (process:supervisor-start-link
      [{:id :fragile :restart :permanent
        :start (fn []
          (process:send me [:started (process:self)])
          (forever
            (match (process:recv)
              (:crash (error {:error :boom :message "crash"}))
              (:ping  (process:send me :pong))
              (_ nil))))}]
      :name :sup3)

    # Wait for first start
    (match (process:recv)
      ([:started child-pid]
        # Verify child is alive
        (process:send child-pid :ping)
        (assert (= :pong (process:recv)) "restart: child responds")

        # Crash it
        (process:send child-pid :crash)

        # Supervisor should restart — wait for new start
        (match (process:recv)
          ([:started new-pid]
            (assert (not (= new-pid child-pid)) "restart: new pid differs")
            (process:send new-pid :ping)
            (assert (= :pong (process:recv)) "restart: restarted child responds"))
          (_ (assert false "restart: expected [:started new-pid]"))))
      (_ (assert false "restart: expected [:started child-pid]"))))))
(println "  14. supervisor restart permanent: ok")


# ── 15. Supervisor does not restart temporary child ──────────────────

(process:start (fn []
  (let ([me (process:self)])
    (process:supervisor-start-link
      [{:id :temp :restart :temporary
        :start (fn []
          (process:send me [:started (process:self)])
          (process:recv))}]
      :name :sup4)

    (match (process:recv)
      ([:started child-pid]
        # Kill the temporary child
        (process:exit child-pid :kill)

        # Give supervisor a tick to process the DOWN
        (process:send me :sync)
        (process:recv)
        (process:send me :sync)
        (process:recv)

        # No restart expected — send ourselves proof
        (process:send me :no-restart)
        (let ([msg (process:recv)])
          (assert (= msg :no-restart) "temporary: not restarted")))
      (_ (assert false "temporary: expected [:started pid]"))))))
(println "  15. supervisor temporary child: ok")


# ── 16. Supervisor transient child — normal exit not restarted ───────

(process:start (fn []
  (let ([me (process:self)])
    (process:supervisor-start-link
      [{:id :trans :restart :transient
        :start (fn []
          (process:send me [:started (process:self)])
          # Exit normally after receiving :go
          (process:recv)
          :done)}]
      :name :sup5)

    (match (process:recv)
      ([:started child-pid]
        (process:send child-pid :go)

        # Give supervisor time to process
        (process:send me :sync)
        (process:recv)
        (process:send me :sync)
        (process:recv)

        (process:send me :no-restart)
        (let ([msg (process:recv)])
          (assert (= msg :no-restart) "transient-normal: not restarted")))
      (_ (assert false "transient: expected [:started pid]"))))))
(println "  16. supervisor transient normal exit: ok")


# ── 17. GenServer as supervised child ─────────────────────────────────

(process:start (fn []
  (let ([me (process:self)])
    (process:supervisor-start-link
      [{:id :kv :restart :permanent
        :start (fn []
          (process:send me :kv-ready)
          # Run a genserver loop inline
          (process:register :kv-sup)
          (var state @{})
          (forever
            (let ([msg (process:recv)])
              (match msg
                ([:$call caller ref request]
                  (match request
                    ([:get key]
                      (process:send caller [:$reply ref (get state key nil)]))
                    ([:put key val]
                      (put state key val)
                      (process:send caller [:$reply ref :ok]))
                    (_ nil)))
                (_ nil)))))}])

    (process:recv)  # :kv-ready

    (process:gen-server-call :kv-sup [:put :lang "elle"])
    (let ([val (process:gen-server-call :kv-sup [:get :lang])])
      (assert (= val "elle") "supervised genserver: got elle")))))
(println "  17. genserver under supervisor: ok")


# ============================================================================
# Task
# ============================================================================

# ── 18. Task async/await ──────────────────────────────────────────────

(process:start (fn []
  (let* ([task (process:task-async (fn [] (* 6 7)))]
         [result (process:task-await task)])
    (assert (= result 42) "task: 6*7 = 42"))))
(println "  18. task async/await: ok")


# ── 19. Multiple tasks ───────────────────────────────────────────────

(process:start (fn []
  (let* ([t1 (process:task-async (fn [] (+ 10 20)))]
         [t2 (process:task-async (fn [] (+ 30 40)))]
         [r1 (process:task-await t1)]
         [r2 (process:task-await t2)])
    (assert (= r1 30) "multi-task: t1 = 30")
    (assert (= r2 70) "multi-task: t2 = 70"))))
(println "  19. multiple tasks: ok")


# ============================================================================
# Supervisor strategies
# ============================================================================

# ── 20. one-for-all strategy ─────────────────────────────────────────

(process:start (fn []
  (let ([me (process:self)])
    (var starts @[])
    (process:supervisor-start-link
      [{:id :a :restart :permanent
        :start (fn []
          (process:send me [:started :a (process:self)])
          (forever (match (process:recv) (:crash (error {:error :boom :message "a"})) (_ nil))))}
       {:id :b :restart :permanent
        :start (fn []
          (process:send me [:started :b (process:self)])
          (forever (match (process:recv) (_ nil))))}]
      :name :ofa-sup :strategy :one-for-all)

    # Wait for both to start
    (match (process:recv) ([:started id pid] (push starts [id pid])) (_ nil))
    (match (process:recv) ([:started id pid] (push starts [id pid])) (_ nil))
    (assert (= (length starts) 2) "one-for-all: both started")

    # Crash child :a — both should restart
    (let ([a-pid (get (get starts 0) 1)])
      (when (= (get (get starts 0) 0) :a)
        (process:send a-pid :crash))
      (when (= (get (get starts 1) 0) :a)
        (process:send (get (get starts 1) 1) :crash)))

    # Wait for both restarts
    (var restarts @[])
    (match (process:recv) ([:started id pid] (push restarts id)) (_ nil))
    (match (process:recv) ([:started id pid] (push restarts id)) (_ nil))
    (assert (= (length restarts) 2) "one-for-all: both restarted"))))
(println "  20. one-for-all strategy: ok")


# ── 21. rest-for-one strategy ────────────────────────────────────────

(process:start (fn []
  (let ([me (process:self)])
    (process:supervisor-start-link
      [{:id :x :restart :permanent
        :start (fn []
          (process:send me [:started :x (process:self)])
          (forever (match (process:recv) (:crash (error {:error :b :message "x"})) (_ nil))))}
       {:id :y :restart :permanent
        :start (fn []
          (process:send me [:started :y (process:self)])
          (forever (match (process:recv) (_ nil))))}
       {:id :z :restart :permanent
        :start (fn []
          (process:send me [:started :z (process:self)])
          (forever (match (process:recv) (_ nil))))}]
      :name :rfo-sup :strategy :rest-for-one)

    # Wait for all 3 to start
    (var pids @{})
    (repeat 3
      (match (process:recv)
        ([:started id pid] (put pids id pid))
        (_ nil)))

    # Crash :x — :x, :y, :z should all restart (x is first, rest-for-one restarts everything after)
    (process:send (get pids :x) :crash)

    # Wait for 3 restarts
    (var restarts @||)
    (repeat 3
      (match (process:recv)
        ([:started id _pid] (put restarts id))
        (_ nil)))
    (assert (has? restarts :x) "rest-for-one: x restarted")
    (assert (has? restarts :y) "rest-for-one: y restarted")
    (assert (has? restarts :z) "rest-for-one: z restarted"))))
(println "  21. rest-for-one strategy: ok")


# ============================================================================
# DynamicSupervisor
# ============================================================================

# ── 22. Add/remove children at runtime ───────────────────────────────

(process:start (fn []
  (let ([me (process:self)])
    (process:supervisor-start-link [] :name :dyn-sup)

    # Start with no children
    (let ([kids (process:supervisor-which-children :dyn-sup)])
      (assert (= (length kids) 0) "dynamic: starts empty"))

    # Add a child
    (let ([pid (process:supervisor-start-child :dyn-sup
                 {:id :dyn-worker :restart :temporary
                  :start (fn []
                    (process:send me [:started (process:self)])
                    (forever (match (process:recv) (_ nil))))})])
      (match (process:recv)
        ([:started _pid] nil)
        (_ nil))

      (let ([kids (process:supervisor-which-children :dyn-sup)])
        (assert (= (length kids) 1) "dynamic: one child"))

      # Remove it
      (process:supervisor-stop-child :dyn-sup :dyn-worker)

      # Give time to process
      (process:send me :sync)
      (process:recv)

      (let ([kids (process:supervisor-which-children :dyn-sup)])
        (assert (= (length kids) 0) "dynamic: back to empty"))))))
(println "  22. dynamic supervisor: ok")


# ============================================================================
# EventManager
# ============================================================================

# ── 23. Add handler, notify, check state ─────────────────────────────

(process:start (fn []
  (let ([me (process:self)])
    (process:event-manager-start-link :name :events)

    # A handler that collects events
    (var collector-mod
      {:init         (fn [_] @[])
       :handle-event (fn [event state]
         (push state event)
         [:ok state])})

    (let ([ref (process:event-manager-add-handler :events collector-mod nil)])
      # Send some events
      (process:event-manager-sync-notify :events :hello)
      (process:event-manager-sync-notify :events :world)

      # Check handlers list
      (let ([handlers (process:event-manager-which-handlers :events)])
        (assert (= (length handlers) 1) "event: one handler"))

      # Remove handler
      (process:event-manager-remove-handler :events ref)
      (let ([handlers (process:event-manager-which-handlers :events)])
        (assert (= (length handlers) 0) "event: handler removed"))))))
(println "  23. event manager: ok")


# ── 24. Multiple handlers receive same event ─────────────────────────

(process:start (fn []
  (let ([me (process:self)])
    (process:event-manager-start-link :name :multi-events)

    # Two handlers that forward events to us
    (var forwarder (fn [tag]
      {:init         (fn [_] nil)
       :handle-event (fn [event _state]
         (process:send me [tag event])
         [:ok nil])}))

    (process:event-manager-add-handler :multi-events (forwarder :h1) nil)
    (process:event-manager-add-handler :multi-events (forwarder :h2) nil)

    (process:event-manager-sync-notify :multi-events :ping)

    (var got @||)
    (match (process:recv) ([tag _event] (put got tag)) (_ nil))
    (match (process:recv) ([tag _event] (put got tag)) (_ nil))
    (assert (has? got :h1) "multi-event: h1 received")
    (assert (has? got :h2) "multi-event: h2 received"))))
(println "  24. multiple event handlers: ok")


# ── 25. Handler self-removal via [:remove state] ─────────────────────

(process:start (fn []
  (process:event-manager-start-link :name :remove-events)

  # Handler that removes itself after seeing :done
  (var once-mod
    {:init         (fn [_] nil)
     :handle-event (fn [event state]
       (if (= event :done)
         [:remove state]
         [:ok state]))})

  (process:event-manager-add-handler :remove-events once-mod nil)
  (assert (= 1 (length (process:event-manager-which-handlers :remove-events)))
          "self-remove: handler present")

  (process:event-manager-sync-notify :remove-events :keep)
  (assert (= 1 (length (process:event-manager-which-handlers :remove-events)))
          "self-remove: still present after :keep")

  (process:event-manager-sync-notify :remove-events :done)
  (assert (= 0 (length (process:event-manager-which-handlers :remove-events)))
          "self-remove: removed after :done")))
(println "  25. handler self-removal: ok")


(println "")
(println "all genserver tests passed.")
