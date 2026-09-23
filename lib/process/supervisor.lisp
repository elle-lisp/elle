(elle/epoch 12)
# audited: 2026-09-23
# Supervisor: a process that starts child processes and restarts them by policy.
# docs/behaviors.md

(fn [p gs]
  (def {:send send
        :recv recv
        :recv-match recv-match
        :self self
        :spawn-link spawn-link
        :monitor monitor
        :exit exit
        :register register
        :trap-exit trap-exit
        :put-dict put-dict
        :get-dict get-dict} p)
  (def gen-make-ref gs:gen-make-ref)
  (def gen-resolve gs:gen-resolve)

  (defn
    supervisor-start-link
    [children &named name strategy max-restarts max-ticks logger]
    "Spawn a linked Supervisor managing the given child specs."
    (let [parent (self)
          strat (or strategy :one-for-one)
          intensity-max max-restarts
          intensity-period (or max-ticks 5)
          log (or logger (fn [_] nil))]
      (spawn-link (fn []
                    (when (not (nil? name)) (register name))
                    (trap-exit true)

                    (def child-order @[])  # ids in start order, for rest-for-one
                    (def @kids @{})  # id → @{:pid :ref :spec}
                    (def restart-history @{})  # id → @[tick ...]
                    (def @sup-tick 0)  # incremented each supervision loop iteration
                    (def sup-self (self))

                    (defn start-child [spec]
                      (let* [start-fn (get spec :start)
                             needs-ready (get spec :ready)
                             wrapped (if needs-ready
                                       (fn []
                                         (put-dict :$supervisor-pid sup-self)
                                         (start-fn))
                                       start-fn)
                             child-pid (spawn-link wrapped)
                             ref (monitor child-pid)]
                        (put kids (get spec :id)
                             @{:pid child-pid :ref ref :spec spec})
                        (log {:event :child-started
                              :id (get spec :id)
                              :pid child-pid})
                        # A child that declares readiness is waited for before proceeding
                        (when needs-ready
                          (let [signal (recv-match (fn [m]
                                  (and (array? m) (>= (length m) 2)
                                       (or (and (= (get m 0) :$sup-ready)
                                       (= (get m 1) child-pid))
                                       (and (>= (length m) 3)
                                       (= (get m 0) :DOWN)
                                       (= (get m 2) child-pid))))))]
                            (if (= (get signal 0) :$sup-ready)
                              (log {:event :child-ready
                                    :id (get spec :id)
                                    :pid child-pid})
                              (begin
                                (log {:event :child-exited
                                      :id (get spec :id)
                                      :pid child-pid
                                      :reason (get signal 3)})
                                (del kids (get spec :id))))))
                        child-pid))

                    (defn stop-child [id]
                      (when (has? kids id)
                        (exit (get (get kids id) :pid) :shutdown)
                        (del kids id)))

                    (defn should-restart? [policy reason]
                      (cond
                        (= policy :permanent) true
                        (= policy :transient) (match reason
                          [:normal _] false
                          _ true)
                        false))

                    # Check restart intensity — returns true if restart is allowed
                    (defn check-intensity [id]
                      (if (nil? intensity-max)
                        true
                        (let [recent @[]]
                          (each t in (or (get restart-history id) @[])
                            (when (>= t (- sup-tick intensity-period))
                              (push recent t)))
                          (push recent sup-tick)
                          (put restart-history id recent)
                          (if (> (length recent) intensity-max)
                            (begin
                              (log {:event :max-restarts-reached
                                    :id id
                                    :shutting-down true})
                              false)
                            true))))

                    (defn static-spec [id]
                      (find (fn [s] (= (get s :id) id)) children))

                    (defn restart [dead-id spec]
                      (case strat
                        :one-for-one (start-child spec)
                        :one-for-all
                          (begin
                            (each [id info] in (pairs kids)
                              (when (not (= id dead-id))
                                (exit (get info :pid) :shutdown)))
                            (assign kids @{})
                            (each id in child-order
                              (let [s (static-spec id)]
                                (when (not (nil? s)) (start-child s)))))
                        :rest-for-one
                          (begin
                            (def @pos 0)
                            (def @found false)
                            (each id in child-order
                              (when (not found)
                                (if (= id dead-id)
                                  (assign found true)
                                  (assign pos (+ pos 1)))))
                            (def @i (+ pos 1))
                            (while (< i (length child-order))
                              (stop-child (get child-order i))
                              (assign i (+ i 1)))
                            (del kids dead-id)
                            (assign i pos)
                            (while (< i (length child-order))
                              (let [s (static-spec (get child-order i))]
                                (when (not (nil? s)) (start-child s)))
                              (assign i (+ i 1))))))

                    (each spec in children
                      (push child-order (get spec :id))
                      (start-child spec))

                    (defn find-dead-id [dead-pid]
                      (def @found nil)
                      (each [id info] in (pairs kids)
                        (when (= (get info :pid) dead-pid) (assign found id)))
                      found)

                    (forever
                      (assign sup-tick (+ sup-tick 1))
                      (match (recv)
                        [:DOWN _ref dead-pid reason]
                          (let [dead-id (find-dead-id dead-pid)]
                            (when (not (nil? dead-id))
                              (log {:event :child-exited
                                    :id dead-id
                                    :pid dead-pid
                                    :reason reason})
                              (let [spec (get (get kids dead-id) :spec)]
                                (if (and (should-restart? (or (get spec :restart)
                                  :permanent) reason) (check-intensity dead-id))
                                  (begin
                                    (log {:event :child-restarting
                                    :id dead-id
                                    :attempt (length (or (get restart-history
                                    dead-id) @[]))})
                                    (restart dead-id spec))
                                  (del kids dead-id)))))
                        [:$sup-start-child caller ref spec]
                          (let [child-pid (start-child spec)]
                            (push child-order (get spec :id))
                            (send caller [:$reply ref child-pid]))
                        [:$sup-stop-child caller ref id]
                          (begin
                            (stop-child id)
                            (send caller [:$reply ref :ok]))
                        [:$sup-which-children caller ref]
                          (let [result @[]]
                            (each [id info] in (pairs kids)
                              (push result {:id id :pid (get info :pid)}))
                            (send caller [:$reply ref (freeze result)]))
                        [:EXIT from-pid _reason]
                          (when (= from-pid parent)
                            (each [_id info] in (pairs kids)
                              (exit (get info :pid) :shutdown))
                            (exit (self) :shutdown))
                        _ nil))))))

  # ── client API ──────────────────────────────────────────────────────

  (defn sup-call [sup msg-tag & args]
    "Send a request to a supervisor and wait for its reply."
    (let* [pid (gen-resolve sup)
           ref (gen-make-ref)]
      (send pid [msg-tag (self) ref ;args])
      (get (recv-match (fn [m]
                         (and (array? m) (= (get m 0) :$reply) (= (get m 1) ref))))
           2)))

  (defn supervisor-start-child [sup spec]
    "Add a child to a running supervisor. Returns the child pid."
    (sup-call sup :$sup-start-child spec))

  (defn supervisor-stop-child [sup id]
    "Remove and stop a child by id."
    (sup-call sup :$sup-stop-child id))

  (defn supervisor-which-children [sup]
    "List active children as [{:id :pid} ...]."
    (sup-call sup :$sup-which-children))

  (defn supervisor-notify-ready []
    "Signal to the supervisor that this child is ready to serve.
     Call from within a child process whose spec has :ready true.
     The supervisor blocks on this signal before starting the next child."
    (let [sup-pid (get-dict :$supervisor-pid)]
      (when (not (nil? sup-pid))
        (send sup-pid [:$sup-ready (self)]))))

  # ── subprocess child ────────────────────────────────────────────────

  (defn make-subprocess-child [id bin args &named opts restart]
    "Create a child spec that manages an OS subprocess under a supervisor.
     The child process spawns the subprocess, blocks on subprocess/wait,
     then crashes to trigger supervisor restart on unexpected exit.

     Options:
       :opts     — options hash passed to subprocess/exec (env, cwd, etc.)
       :restart  — :permanent (default), :transient, or :temporary"
    {:id id
     :restart (or restart :permanent)
     :start (fn []
              (let [code (subprocess/wait (subprocess/exec bin args
                    (or opts @{})))]
                (when (not (= code 0))
                  (error {:error :subprocess-exit
                          :message (string id " exited with code " code)
                          :code code}))))})

  {:supervisor-start-link supervisor-start-link
   :supervisor-start-child supervisor-start-child
   :supervisor-stop-child supervisor-stop-child
   :supervisor-which-children supervisor-which-children
   :supervisor-notify-ready supervisor-notify-ready
   :make-subprocess-child make-subprocess-child})
