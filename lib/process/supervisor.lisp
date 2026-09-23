(elle/epoch 12)
# audited: 2026-09-23
# Supervisor: a process that starts child processes and restarts them by policy.
# docs/behaviors.md
#
# The supervisor traps exits and links to every child, so each child's exit
# reaches it as [:EXIT pid reason], even from a child that dies before the
# supervisor runs again. It never monitors a child.
#
# Messages (internal):
#   [:$sup-started ref]                   supervisor → parent, children started
#   [:$sup-ready pid]                     child → supervisor, from notify-ready
#   [:$sup-start-child caller ref spec]   client → supervisor
#   [:$sup-stop-child caller ref id]      client → supervisor
#   [:$sup-which-children caller ref nil] client → supervisor
#   [:$reply ref value]                   supervisor → client

(fn [p gs]
  (def {:send send
        :recv recv
        :recv-match recv-match
        :self self
        :spawn-link spawn-link
        :link link
        :exit exit
        :register register
        :trap-exit trap-exit
        :now now
        :put-dict put-dict
        :get-dict get-dict} p)

  # ── child specs ─────────────────────────────────────────────────────

  (defn invalid [message]
    {:error :invalid-child-spec :message message})

  (defn spec-problem [spec]
    "The :invalid-child-spec error for a malformed spec, or nil."
    (let* [id (get spec :id)
           start? (not (nil? (get spec :start)))
           start-link? (not (nil? (get spec :start-link)))
           which (string "child " id)]
      (cond
        (nil? id) (invalid "a child spec needs an :id")
        (= start? start-link?) (invalid (string which
                                        " needs exactly one of :start and :start-link"))
        (and start-link? (get spec :ready)) (invalid (string which
        ": :ready goes with :start only"))
        nil)))

  (defn check-specs [specs]
    "Raise for the first malformed spec in specs, or for an id used twice."
    (def seen @||)
    (each spec in specs
      (let [problem (spec-problem spec)]
        (when problem (error problem)))
      (when (has? seen (get spec :id))
        (error (invalid (string "child id " (get spec :id) " appears twice"))))
      (put seen (get spec :id))))

  (defn should-restart? [policy reason]
    (case policy
      :permanent true
      :transient
        (not (or (= reason :normal)
                 (and (array? reason) (= (first reason) :normal))))
      false))

  # ── the supervisor process ──────────────────────────────────────────

  (defn
    supervise
    [parent started-ref children strategy max-restarts max-ticks log]
    (def specs @{})  # id → spec, for every child the supervisor has
    (def @order @[])  # ids in start order
    (def kids @{})  # id → pid, for every live child
    (def history @{})  # id → @[tick ...], its restarts inside the window
    (def me (self))

    (defn await-ready [id pid]
      "Wait for the child to report ready. An exit that comes first goes
       back to the mailbox, where the loop handles it like any other."
      (match (recv-match (fn [m]
                           (and (array? m) (= (get m 1) pid)
                                (or (= (get m 0) :$sup-ready)
                                    (= (get m 0) :EXIT)))))
        [:$sup-ready _] (log {:event :child-ready :id id :pid pid})
        exit-message (send me exit-message)))

    (defn start-child [id]
      (let* [spec (get specs id)
             start (get spec :start)
             start-link (get spec :start-link)
             pid (if start-link
                   (let [pid (start-link)]
                     (when (not (integer? pid))
                       (error (invalid (string "child " id
                                       ": :start-link returned " pid
                                       ", not a pid"))))
                     (link pid)
                     pid)
                   (spawn-link (if (get spec :ready)
                                 (fn []
                                   (put-dict :$supervisor-pid me)
                                   (start))
                                 start)))]
        (put kids id pid)
        (log {:event :child-started :id id :pid pid})
        (when (get spec :ready) (await-ready id pid))
        pid))

    (defn terminate [id]
      (when (has? kids id)
        (exit (get kids id) :shutdown)
        (del kids id)))

    (defn forget [id]
      (terminate id)
      (del specs id)
      (del history id)
      (assign order (->array (filter (fn [x] (not (= x id))) (->list order)))))

    (defn from [id]
      "The ids in start order, from id to the last."
      (def @found false)
      (def acc @[])
      (each x in order
        (when (= x id) (assign found true))
        (when found (push acc x)))
      acc)

    (defn restart [id]
      (let [affected (case strategy
                       :one-for-all (->array order)
                       :rest-for-one (from id)
                       [id])]
        (each x in affected
          (terminate x))
        (each x in affected
          (start-child x))))

    (defn allow-restart? [id]
      "Record a restart of id now. False once the restarts inside the last
       max-ticks ticks outnumber max-restarts."
      (let [t (now)
            recent @[]]
        (each s in (or (get history id) [])
          (when (>= s (- t max-ticks)) (push recent s)))
        (push recent t)
        (put history id recent)
        (or (nil? max-restarts) (<= (length recent) max-restarts))))

    (defn shutdown []
      (each id in (->list order)
        (terminate id))
      (exit me :shutdown))

    (defn child-exited [pid reason]
      (let [id (find (fn [id] (= (get kids id) pid)) (keys kids))]
        (when (not (nil? id))
          (del kids id)
          (log {:event :child-exited :id id :pid pid :reason reason})
          (let [policy (or (get (get specs id) :restart) :permanent)]
            (cond
              (not (should-restart? policy reason)) (when (= policy :temporary)
                (forget id))
              (allow-restart? id)
                (begin
                  (log {:event :child-restarting
                        :id id
                        :attempt (length (get history id))})
                  (restart id))
              (begin
                (log {:event :max-restarts-reached :id id :shutting-down true})
                (shutdown)))))))

    (defn add-child [spec]
      "Start a child added at runtime. Returns its pid, or an error struct."
      (let [id (get spec :id)]
        (if (has? specs id)
          (invalid (string "child id " id " is taken"))
          (begin
            (put specs id spec)
            (push order id)
            (start-child id)))))

    (defn which-children []
      (freeze (->array (map (fn [id] {:id id :pid (get kids id)})
                            (filter (fn [id] (has? kids id)) (->list order))))))

    (each spec in children
      (put specs (get spec :id) spec)
      (push order (get spec :id)))
    (each id in (->list order)
      (start-child id))
    (send parent [:$sup-started started-ref])

    (forever
      (match (recv)
        [:EXIT from reason] (if (= from parent)
                              (shutdown)
                              (child-exited from reason))
        [:$sup-start-child caller ref spec]
          (send caller [:$reply ref (add-child spec)])
        [:$sup-stop-child caller ref id]
          (begin
            (forget id)
            (send caller [:$reply ref :ok]))
        [:$sup-which-children caller ref _]
          (send caller [:$reply ref (which-children)])
        _ nil)))

  (defn
    supervisor-start-link
    [children &named name strategy max-restarts max-ticks logger]
    "Spawn a linked Supervisor managing the given child specs, and return its
     pid once it has started them all. Raises {:error :invalid-child-spec} for
     a malformed spec."
    (check-specs children)
    (let* [parent (self)
           ref (gs:gen-make-ref)
           sup (spawn-link (fn []
                             (when (not (nil? name)) (register name))
                             (trap-exit true)
                             (supervise parent ref children
                                        (or strategy :one-for-one) max-restarts
                                        (or max-ticks 100)
                                        (or logger (fn [_] nil)))))]
      # A supervisor that dies while it starts its children reaches a
      # trapping parent as :EXIT, which goes back to the mailbox.
      (match (recv-match (fn [m]
                           (and (array? m)
                                (or (and (= (get m 0) :$sup-started)
                                    (= (get m 1) ref))
                                    (and (= (get m 0) :EXIT) (= (get m 1) sup))))))
        [:$sup-started _] sup
        exit-message (begin
                       (send parent exit-message)
                       sup))))

  # ── client API ──────────────────────────────────────────────────────

  (defn supervisor-start-child [sup spec]
    "Add a child to a running supervisor. Returns the child pid. Raises
     {:error :invalid-child-spec} for a malformed spec or an id in use."
    (let [problem (spec-problem spec)]
      (when problem (error problem)))
    (let [reply (gs:gen-call sup :$sup-start-child spec nil)]
      (if (integer? reply) reply (error reply))))

  (defn supervisor-stop-child [sup id]
    "Stop a child by id, and forget its spec so no strategy restarts it."
    (gs:gen-call sup :$sup-stop-child id nil))

  (defn supervisor-which-children [sup]
    "List live children as [{:id :pid} ...], in start order."
    (gs:gen-call sup :$sup-which-children nil nil))

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
       :opts     — options passed to subprocess/exec (env, cwd, etc.); default {}
       :restart  — :permanent (default), :transient, or :temporary"
    {:id id
     :restart (or restart :permanent)
     :start (fn []
              (let [code (subprocess/wait (subprocess/exec bin args (or opts {})))]
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
