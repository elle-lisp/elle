(elle/epoch 12)
# audited: 2026-09-23
# GenServer, a callback-driven server process, and Actor, a state holder built on it.
# docs/behaviors.md
#
# Message protocol (internal, $-prefixed):
#   [:$call caller-pid ref request]   client → server
#   [:$cast request]                  client → server
#   [:$stop caller-pid ref reason]    client → server
#   [:$reply ref value]               server → client
#   [:$call-timeout ref]              timer  → client (self)

(fn [p]
  (def {:send send
        :recv recv
        :recv-match recv-match
        :self self
        :spawn-link spawn-link
        :exit exit
        :register register
        :whereis whereis
        :send-after send-after
        :cancel-timer cancel-timer
        :put-dict put-dict
        :get-dict get-dict} p)

  # ── helpers ─────────────────────────────────────────────────────────

  (defn gen-make-ref []
    "Per-process monotonic ref for call correlation."
    (let [n (or (get-dict :$gen-call-ref) 0)]
      (put-dict :$gen-call-ref (+ n 1))
      n))

  (defn gen-resolve [server]
    "Resolve server — pid passes through, keyword does whereis."
    (if (keyword? server)
      (let [pid (whereis server)]
        (when (nil? pid)
          (error {:error :noproc
                  :message (string "no process registered as " server)}))
        pid)
      server))

  # ── client API ──────────────────────────────────────────────────────

  (defn gen-server-reply [from reply]
    "Send a reply to a pending call. from is the [pid ref] pair from handle-call."
    (send (get from 0) [:$reply (get from 1) reply]))

  (defn gen-server-call [server request &named timeout]
    "Synchronous request-response. Blocks until the server replies, or raises
     {:error :gen-server-timeout} once :timeout ticks pass without a reply."
    (let* [pid (gen-resolve server)
           ref (gen-make-ref)
           me (self)
           timer-ref (when (not (nil? timeout))
                       (send-after timeout me [:$call-timeout ref]))]
      (send pid [:$call me ref request])
      (let [reply (recv-match (fn [m]
                                (and (array? m) (>= (length m) 3)
                                     (or (and (= (get m 0) :$reply)
                                     (= (get m 1) ref))
                                     (and (= (get m 0) :$call-timeout)
                                     (= (get m 1) ref))))))]
        (when (not (nil? timer-ref)) (cancel-timer timer-ref))
        (when (= (get reply 0) :$call-timeout)
          (error {:error :gen-server-timeout
                  :message "gen-server call timed out"}))
        (get reply 2))))

  (defn gen-server-cast [server request]
    "Asynchronous one-way message. Returns :ok immediately."
    (send (gen-resolve server) [:$cast request])
    :ok)

  (defn gen-server-stop [server &named reason timeout]
    "Request graceful shutdown. Blocks until the server acknowledges, or raises
     {:error :gen-server-timeout} once :timeout ticks pass first."
    (let* [pid (gen-resolve server)
           ref (gen-make-ref)
           me (self)
           rsn (or reason :normal)
           timer-ref (when (not (nil? timeout))
                       (send-after timeout me [:$call-timeout ref]))]
      (send pid [:$stop me ref rsn])
      (let [reply (recv-match (fn [m]
                                (and (array? m) (>= (length m) 3)
                                     (or (and (= (get m 0) :$reply)
                                     (= (get m 1) ref))
                                     (and (= (get m 0) :$call-timeout)
                                     (= (get m 1) ref))))))]
        (when (not (nil? timer-ref)) (cancel-timer timer-ref))
        (when (= (get reply 0) :$call-timeout)
          (error {:error :gen-server-timeout
                  :message "gen-server stop timed out"}))
        (get reply 2))))

  # ── server loop ─────────────────────────────────────────────────────

  (defn gen-server-start-link [callbacks init-arg &named name]
    "Spawn a linked GenServer. Returns the pid."
    (let* [handle-call (get callbacks :handle-call)
           handle-cast (get callbacks :handle-cast)
           handle-info (or (get callbacks :handle-info)
                           (fn [_msg state] [:noreply state]))
           on-terminate (or (get callbacks :terminate) (fn [_reason _state] nil))
           init-fn (get callbacks :init)]
      (defn stop [reason state]
        (on-terminate reason state)
        (exit (self) reason))
      (spawn-link (fn []
                    (when (not (nil? name)) (register name))
                    (def @state
                      (let [result (init-fn init-arg)]
                        (if (and (array? result) (> (length result) 0))
                          (case (get result 0)
                            :ok (get result 1)
                            :stop
                              (begin
                                (stop (get result 1) nil)
                                nil)
                            result)
                          result)))
                    (forever
                      (let [msg (recv)]
                        (match msg
                          [:$call caller ref request]
                            (match (handle-call request [caller ref] state)
                              [:reply reply new-state]
                                (begin
                                  (send caller [:$reply ref reply])
                                  (assign state new-state))
                              [:noreply new-state] (assign state new-state)
                              [:stop reason reply new-state]
                                (begin
                                  (send caller [:$reply ref reply])
                                  (stop reason new-state))
                              _ (error {:error :gen-server-error
                                        :message "handle-call returned invalid result"}))
                          [:$cast request]
                            (match (handle-cast request state)
                              [:noreply new-state] (assign state new-state)
                              [:stop reason new-state] (stop reason new-state)
                              _ (error {:error :gen-server-error
                                        :message "handle-cast returned invalid result"}))
                          [:$stop caller ref reason]
                            (begin
                              (send caller [:$reply ref :ok])
                              (stop reason state))
                          _
                            (match (handle-info msg state)
                              [:noreply new-state] (assign state new-state)
                              [:stop reason new-state] (stop reason new-state)
                              _ (error {:error :gen-server-error
                                        :message "handle-info returned invalid result"})))))))))

  # ── Actor ───────────────────────────────────────────────────────────

  (defn actor-start-link [init-fn &named name]
    "Spawn a linked Actor. init-fn takes no args, returns initial state."
    (gen-server-start-link {:init (fn [_] (init-fn))
                            :handle-call (fn [request _from state]
                              (case (get request 0)
                                :get
                                  [:reply ((get request 1) state) state]
                                :update
                                  [:reply :ok ((get request 1) state)]))
                            :handle-cast (fn [request state]
                              (case (get request 0)
                                :update
                                  [:noreply ((get request 1) state)]))} nil
                           :name name))

  (defn actor-get [actor fun]
    "Read a value derived from the actor's state."
    (gen-server-call actor [:get fun]))

  (defn actor-update [actor fun]
    "Transform the actor's state synchronously. Returns :ok."
    (gen-server-call actor [:update fun]))

  (defn actor-cast [actor fun]
    "Transform the actor's state asynchronously. Returns :ok."
    (gen-server-cast actor [:update fun]))

  {:gen-make-ref gen-make-ref
   :gen-resolve gen-resolve
   :gen-server-start-link gen-server-start-link
   :gen-server-call gen-server-call
   :gen-server-cast gen-server-cast
   :gen-server-stop gen-server-stop
   :gen-server-reply gen-server-reply
   :actor-start-link actor-start-link
   :actor-get actor-get
   :actor-update actor-update
   :actor-cast actor-cast})
