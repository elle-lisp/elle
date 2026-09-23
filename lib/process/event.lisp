(elle/epoch 12)
# audited: 2026-09-23
# EventManager: a GenServer that broadcasts each event to a list of handler modules.
# docs/behaviors.md

(fn [gs]
  (def gen-server-call gs:gen-server-call)
  (def gen-server-cast gs:gen-server-cast)

  (defn terminate [h]
    (let [term (get (get h :mod) :terminate)]
      (when (not (nil? term)) (term :remove (get h :state)))))

  (defn dispatch [event handlers]
    "Run every handler on event. Returns the handlers that stay."
    (def remaining @[])
    (each h in handlers
      (match ((get (get h :mod) :handle-event) event (get h :state))
        [:ok new-state] (begin
                          (put h :state new-state)
                          (push remaining h))
        [:remove _new-state] (terminate h)
        _ (push remaining h)))
    remaining)

  (defn handle-call [request _from handlers]
    (match request
      [:add-handler mod init-arg]
        (let [ref (gs:gen-make-ref)]
          (push handlers @{:id ref :mod mod :state ((get mod :init) init-arg)})
          [:reply ref handlers])
      [:remove-handler ref]
        (let [remaining @[]]
          (each h in handlers
            (if (= (get h :id) ref) (terminate h) (push remaining h)))
          [:reply :ok remaining])
      [:sync-notify event] [:reply :ok (dispatch event handlers)]
      [:which-handlers]
        [:reply
         (freeze (map (fn [h] {:id (get h :id) :mod (get h :mod)}) handlers))
         handlers]
      _ [:reply :unknown handlers]))

  (defn handle-cast [request handlers]
    (match request
      [:notify event] [:noreply (dispatch event handlers)]
      _ [:noreply handlers]))

  (defn event-manager-start-link [&named name]
    "Spawn a linked EventManager. Returns pid."
    (gs:gen-server-start-link {:init (fn [_] @[])
                               :handle-call handle-call
                               :handle-cast handle-cast} nil :name name))

  (defn event-manager-add-handler [manager mod init-arg]
    "Add a handler module to the event manager. Returns handler ref."
    (gen-server-call manager [:add-handler mod init-arg]))

  (defn event-manager-remove-handler [manager ref]
    "Remove a handler by ref."
    (gen-server-call manager [:remove-handler ref]))

  (defn event-manager-notify [manager event]
    "Broadcast an event to all handlers (async)."
    (gen-server-cast manager [:notify event]))

  (defn event-manager-sync-notify [manager event]
    "Broadcast an event to all handlers (sync — waits for processing)."
    (gen-server-call manager [:sync-notify event]))

  (defn event-manager-which-handlers [manager]
    "List registered handlers."
    (gen-server-call manager [:which-handlers]))

  {:event-manager-start-link event-manager-start-link
   :event-manager-add-handler event-manager-add-handler
   :event-manager-remove-handler event-manager-remove-handler
   :event-manager-notify event-manager-notify
   :event-manager-sync-notify event-manager-sync-notify
   :event-manager-which-handlers event-manager-which-handlers})
