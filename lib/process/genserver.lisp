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
        :monitor monitor
        :demonitor demonitor
        :exit exit
        :register register
        :whereis whereis
        :send-after send-after
        :cancel-timer cancel-timer} p)

  # ── helpers ─────────────────────────────────────────────────────────

  (defn gen-make-ref []
    "A ref that no other ref from this scheduler equals."
    (yield [:make-ref]))

  (defn gen-resolve [server]
    "Resolve server — pid passes through, keyword does whereis."
    (if (keyword? server)
      (let [pid (whereis server)]
        (when (nil? pid)
          (error {:error :noproc
                  :message (string "no process registered as " server)}))
        pid)
      server))

  (defn timeout-for? [ref]
    "A predicate for the [:$call-timeout ref] message a deadline sends."
    (fn [m]
      (and (array? m) (= (length m) 2) (= (get m 0) :$call-timeout)
           (= (get m 1) ref))))

  (defn reply-to? [ref]
    "A predicate for the [:$reply ref value] message that answers ref."
    (fn [m]
      (and (array? m) (= (length m) 3) (= (get m 0) :$reply) (= (get m 1) ref))))

  (defn down-for? [mref]
    "A predicate for the [:DOWN mref pid reason] message the monitor mref sends."
    (fn [m]
      (and (array? m) (= (length m) 4) (= (get m 0) :DOWN) (= (get m 1) mref))))

  (defn await-reply [ref timeout reply?]
    "The first message reply? matches, or :timeout once timeout ticks pass.
     No timeout waits for good. Either way, the deadline leaves no message."
    (let* [timed-out? (timeout-for? ref)
           timer (when (not (nil? timeout))
                   (send-after timeout (self) [:$call-timeout ref]))
           msg (recv-match (fn [m] (or (reply? m) (timed-out? m))))]
      (cond
        (timed-out? msg) :timeout
        # A timer that already fired left its message behind the reply.
        (and (not (nil? timer)) (= (cancel-timer timer) :not-found)) (begin
          (recv-match timed-out?)
          msg)
        msg)))

  (defn gen-call [server tag payload timeout]
    "Send [tag self ref payload] to server, and return the value it replies.
     The call monitors server while it waits, so an exit ends the wait."
    (let* [pid (gen-resolve server)
           mref (monitor pid)
           ref (gen-make-ref)
           reply? (reply-to? ref)
           down? (down-for? mref)]
      (send pid [tag (self) ref payload])
      (let [msg (await-reply ref timeout (fn [m] (or (reply? m) (down? m))))]
        (cond
          (down? msg)
            (error {:error :gen-server-down
                    :reason (get msg 3)
                    :message (string server " exited before it replied: "
                                     (get msg 3))})
          (begin
            (demonitor mref :flush true)
            (when (= msg :timeout)
              (error {:error :gen-server-timeout
                      :message (string "no reply from " server " within "
                                       timeout " ticks")}))
            (get msg 2))))))

  # ── client API ──────────────────────────────────────────────────────

  (defn gen-server-reply [from reply]
    "Send a reply to a pending call. from is the [pid ref] pair from handle-call."
    (send (get from 0) [:$reply (get from 1) reply]))

  (defn gen-server-call [server request &named timeout]
    "Synchronous request-response. Blocks until the server replies. Raises
     {:error :gen-server-down :reason r} when the server exits first, and
     {:error :gen-server-timeout} once :timeout ticks pass without a reply."
    (gen-call server :$call request timeout))

  (defn gen-server-cast [server request]
    "Asynchronous one-way message. Returns :ok immediately."
    (send (gen-resolve server) [:$cast request])
    :ok)

  (defn gen-server-stop [server &named reason timeout]
    "Request graceful shutdown. Blocks until the server acknowledges. Raises
     {:error :gen-server-down :reason r} when the server exits first, and
     {:error :gen-server-timeout} once :timeout ticks pass first."
    (gen-call server :$stop (or reason :normal) timeout))

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
   :gen-call gen-call
   :await-reply await-reply
   :down-for? down-for?
   :gen-server-start-link gen-server-start-link
   :gen-server-call gen-server-call
   :gen-server-cast gen-server-cast
   :gen-server-stop gen-server-stop
   :gen-server-reply gen-server-reply
   :actor-start-link actor-start-link
   :actor-get actor-get
   :actor-update actor-update
   :actor-cast actor-cast})
