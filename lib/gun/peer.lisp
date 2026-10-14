(elle/epoch 15)
# audited: 2026-10-14
## One std/gun WebSocket connection: the hello, the reader loop, the send lock, and close.
## lib/gun/mesh.md
## lib/gun/overview.md
##
## Loaded via:
##   (def peer ((import "std/gun/peer") :wire wire :websocket ws))
##
## Exports: {:serve :dial :require-websocket}
##
## Both take an `on` struct of five functions, which the mesh supplies:
## :up and :ready take the new peer, :message takes the peer and a message,
## :down takes the peer that left, and :error takes an error with no caller.
## A peer is {:key :id :send :close :gone}: :key is unique to the connection,
## :id is the database id the other side sent in its hello, and :gone is a
## latch that close opens. A peer that :up closes is not answered or readied.

(fn [&named wire websocket]

  (def sync ((import "std/sync")))

  (defn require-websocket []
    "Raise :mesh-not-configured when this module has no :websocket."
    (when (nil? websocket)
      (error {:error :gun-error
              :reason :mesh-not-configured
              :message ":websocket was not supplied to (import \"std/gun\")"})))

  (defn make-sender [conn]
    "A function that sends one message on conn. One lock orders the frames of
     every fiber that sends, because std/websocket does not."
    (let [lock (sync:make-lock)]
      (fn [msg]
        (lock:acquire)
        (defer
          (lock:release)
          (websocket:send conn (wire:encode msg))))))

  (defn not-a-hello []
    {:error :gun-error
     :reason :bad-message
     :why "the first message is not a hello"
     :message "not a gun message: the first message is not a hello"})

  (defn run [conn my-id dialer? on]
    "Read conn until it closes, handing each message to on. The dialer sends
     its hello first; the server registers the dialer before it answers.
     Raises nothing: an error goes to :error, and the connection closes."
    (let [send (make-sender conn)
          transport (get conn :transport)
          @peer nil
          gone (sync:make-latch)
          close (fn []
                  (unless (gone:open?)
                    (gone:open)
                    (when peer (on:down peer)))
                  (protect (transport:close))
                  nil)
          hello (fn [msg]
                  (assign
                    peer
                    {:key (sys/unique)
                     :id msg:peer
                     :send send
                     :close close
                     :gone gone})
                  (on:up peer)
                  (unless (gone:open?)
                    (unless dialer? (send (wire:hi my-id)))
                    (on:ready peer)))
          handle (fn [text]
                   (let [[ok? msg] (protect (wire:decode text))]
                     (cond
                       (not ok?) (on:error msg)
                       peer (on:message peer msg)
                       (= msg:kind :hi) (hello msg)
                       (on:error (not-a-hello)))))
          read-all (fn []
                     (when dialer? (send (wire:hi my-id)))
                     (forever
                       (let [[ok? frame] (protect (websocket:recv conn))]
                         (when (or (not ok?) (= frame:type :close)) (break nil))
                         (when (= frame:type :text) (handle (string frame:data))))))]
      (defer
        (close)
        (let [[ok? err] (protect (read-all))]
          (unless ok? (on:error err))))))

  (defn guarded [on]
    "on, with every function but :error giving its own errors to :error, so
     no error ends a reader silently."
    (let [report on:error
          guard (fn [f]
                  (fn [& args]
                    (let [[ok? err] (protect (apply f args))]
                      (unless ok? (report err)))))]
      {:up (guard on:up)
       :ready (guard on:ready)
       :message (guard on:message)
       :down (guard on:down)
       :error report}))

  (defn serve [listener my-id on]
    "Accept peers on listener until it closes."
    (require-websocket)
    (let [on (guarded on)]
      (websocket:serve listener (fn [conn] (run conn my-id false on)))))

  (defn dial [url my-id wait on]
    "Dial the peer at url, and answer it once both sides have registered each
     other. Raises :peer-timeout when no hello comes back within wait seconds."
    (require-websocket)
    (let [conn (websocket:connect url)
          ready (sync:make-latch)
          @peer nil
          on (guarded on)
          on-ready on:ready
          inner {:up on:up
                 :ready (fn [p]
                          (assign peer p)
                          (on-ready p)
                          (ready:open))
                 :message on:message
                 :down on:down
                 :error on:error}]
      (ev/spawn (fn [] (run conn my-id true inner)))
      (unless (ev/timeout wait
                          (fn []
                            (ready:wait)
                            true))
        (let [transport (get conn :transport)]
          (protect (transport:close)))
        (error {:error :gun-error
                :reason :peer-timeout
                :url url
                :message (string "no hello from " url)}))
      peer))

  {:serve serve :dial dial :require-websocket require-websocket})
