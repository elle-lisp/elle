(elle/epoch 14)
# audited: 2026-10-12
## The std/gun mesh rules: fetch, forward, acks, deferral and resync, over the connections peer.lisp runs.
## lib/gun/mesh.md
## lib/gun/overview.md
##
## Loaded via:
##   (def mesh ((import "std/gun/mesh") :core core :wire wire :peer peer))
##
## Exports: {:fetch :touch :broadcast :await-ack :serve :connect :peers}
##
## A wait is a record of the peers a get or a put went to, and a latch that
## opens when every one answered or left (a get), or when the first one
## acked (a put).

(fn [&named core wire peer]

  (def sync ((import "std/sync")))
  (def uuid ((import "std/uuid")))

  (defn gun-error [reason message &named peer-id peer-reason]
    (error {:error :gun-error
            :reason reason
            :peer peer-id
            :peer-reason peer-reason
            :message message}))

  (defn reporting [c thunk]
    "Run thunk, and give any error it raises to the database's :on-error."
    (let [[ok? err] (protect (thunk))]
      (unless ok? (core:report c err))))

  ## ── Peers ────────────────────────────────────────────────────────────

  (defn peers-except [c except]
    "Every connected peer but the one whose key is except."
    (filter (fn [p] (not (= p:key except))) (values c:peers)))

  (defn send-to [p msg]
    "Send msg to p. A send that fails closes p."
    (let [[ok? _] (protect (p:send msg))]
      (unless ok? (p:close))))

  ## ── Waits ────────────────────────────────────────────────────────────

  (defn open-wait [c id targets until]
    "Wait for the peers in targets to answer the message id. until is :all
     for a get and :first for a put."
    (let [waiting @{}]
      (each p in targets
        (put waiting p:key true))
      (put c:pending id
           {:waiting waiting
            :until until
            :latch (sync:make-latch)
            :first (box nil)})))

  (defn settle [c id key answer]
    "Record that the peer with key answered the message id, with answer."
    (when-let [w (get c:pending id)] (del w:waiting key)
              (when (and answer (nil? (unbox w:first))) (rebox w:first answer))
              (when (or (empty? w:waiting) (and answer (= w:until :first)))
                (let [latch w:latch]
                  (latch:open)))))

  (defn await [c id seconds]
    "Wait for the message id to settle, for seconds at most, and answer the
     first answer it recorded, or nil."
    (let [w (get c:pending id)
          latch w:latch]
      (defer
        (del c:pending id)
        (unless (empty? w:waiting)
          (ev/timeout seconds
                      (fn []
                        (latch:wait)
                        true)))
        (unbox w:first))))

  ## ── Fetching ─────────────────────────────────────────────────────────

  (defn fetch-souls [c souls except]
    "Fetch each soul in souls: load it, ask every peer but except, and wait
     for their answers, :wait seconds at most. A soul another fiber is
     fetching is waited for instead. A soul is pending from before its get
     leaves, which is what ends a get cycle. A fetch that skipped except
     leaves its souls unfetched, so a fiber that waited for it fetches again."
    (let [fresh @[]
          others @[]]
      (each soul in souls
        (let [latch (get c:fetched soul)]
          (if latch
            (push others [soul latch])
            (begin
              (put c:fetched soul (sync:make-latch))
              (push fresh soul)))))
      (defer
        (each soul in fresh
          (let [latch (get c:fetched soul)]
            (when except (del c:fetched soul))
            (latch:open)))
        (each soul in fresh
          (core:load c soul))
        (let [targets (peers-except c except)]
          (unless (or (empty? fresh) (empty? targets))
            (let [id (uuid:v4)
                  msg (wire:get id (freeze fresh))]
              (open-wait c id targets :all)
              (each p in targets
                (send-to p msg))
              (await c id c:wait)))))
      (each [_ latch] in others
        (latch:wait))
      (let [again (filter (fn [soul] (nil? (get c:fetched soul)))
                          (map first others))]
        (unless (empty? again) (fetch-souls c again except)))))

  (defn fetch [c soul]
    "Fetch soul from the peers the first time a read reaches it."
    (fetch-souls c [soul] nil))

  ## ── Merging what a peer sends ────────────────────────────────────────

  (defn touch [c soul]
    "Load soul from the store, and start a fetch of it in a new fiber when
     nobody has fetched it. Never waits on the network."
    (core:load c soul)
    (unless (get c:fetched soul)
      (ev/spawn (fn [] (reporting c (fn [] (fetch c soul))))))
    nil)

  (defn notify [c]
    (reporting c (fn [] (core:notify c (fn [soul] (touch c soul))))))

  (defn forward [c g except]
    "Send the graph g, as a new put, to every peer but except."
    (let [msg (wire:put (uuid:v4) g)]
      (each p in (peers-except c except)
        (send-to p msg))))

  (defn earliest-due [c later]
    (let [clock c:clock
          @soonest nil]
      (each [_ fields] in later
        (each [_ [st _]] in fields
          (let [d (clock:due-in st)]
            (when (or (nil? soonest) (< d soonest)) (assign soonest d)))))
      soonest))

  (defn absorb [c g from]
    "Merge the graph g from the peer whose key is from: merge and store what
     is due, forward what was accepted, notify, and give what is not due yet
     to a fiber that sleeps until it is."
    (let [[accepted later] (core:merge-remote c g)]
      (unless (empty? accepted) (forward c accepted from))
      (notify c)
      (unless (empty? later)
        (ev/spawn (fn []
                    (ev/sleep (earliest-due c later))
                    (reporting c (fn [] (absorb c later from))))))))

  ## ── Handling a message ───────────────────────────────────────────────

  (defn reason-of [err]
    (let [r (when (struct? err) (get err :reason))]
      (if (keyword? r) r :error)))

  (defn handle-put [c p msg]
    (let [[ok? err] (protect (absorb c msg:graph p:key))]
      (if ok?
        (send-to p (wire:ack msg:id))
        (begin
          (send-to p (wire:nack msg:id (reason-of err)))
          (core:report c err)))))

  (defn handle-get [c p msg]
    "Fetch from every other peer each soul of the get this database has not
     started to fetch, then answer with the nodes it holds. A pending soul
     answers from local data at once."
    (fetch-souls c (filter (fn [soul] (nil? (get c:fetched soul))) msg:souls)
                 p:key)
    (send-to p (wire:answer msg:id (core:nodes c msg:souls))))

  (defn handlers [c]
    "The functions peer.lisp calls for the database c."
    {:up (fn [p] (put c:peers p:key p))
     :ready (fn [p]
              (let [souls [;(keys c:loaded)]]
                (unless (empty? souls)
                  (send-to p (wire:get (uuid:v4) souls)))))
     :message (fn [p msg]
                (case msg:kind
                  :put (handle-put c p msg)
                  :get
                    (ev/spawn (fn [] (reporting c (fn [] (handle-get c p msg)))))
                  :answer
                    (begin
                      (reporting c (fn [] (absorb c msg:graph p:key)))
                      (settle c msg:ack p:key nil))
                  :ack (settle c msg:ack p:key [p:id msg])
                  (core:report c
                               {:error :gun-error
                                :reason :bad-message
                                :why "a second hello"
                                :message "not a gun message: a second hello"})))
     :down (fn [p]
             (del c:peers p:key)
             (each id in [;(keys c:pending)]
               (settle c id p:key nil)))
     :error (fn [err] (core:report c err))})

  ## ── A local put ──────────────────────────────────────────────────────

  (defn broadcast [c g ack?]
    "Send the graph g that a local put accepted to every peer. When ack?,
     answer the id of the wait for its first ack."
    (let [targets (peers-except c nil)
          id (uuid:v4)]
      (when ack?
        (open-wait c id (if (empty? g) [] targets) :first))
      (unless (empty? g)
        (let [msg (wire:put id g)]
          (each p in targets
            (send-to p msg))))
      id))

  (defn await-ack [c id seconds]
    "Wait for the first ack of the put id. Raises :no-ack when none comes
     within seconds, and :peer-error when the first ack says the merge failed."
    (let [answer (await c id seconds)]
      (when (nil? answer) (gun-error :no-ack "no peer acked the put"))
      (let [[peer-id msg] answer]
        (unless msg:ok?
          (gun-error :peer-error "a peer failed to merge the put"
                     :peer-id peer-id :peer-reason msg:reason)))))

  ## ── Joining ──────────────────────────────────────────────────────────

  (defn serve [c listener]
    "Accept peers for c on listener until it closes."
    (peer:serve listener c:id (handlers c)))

  (defn connect [c url]
    "Dial the peer at url for c, and answer a function that closes the
     connection."
    (let [p (peer:dial url c:id c:wait (handlers c))]
      p:close))

  (defn peers [c]
    "The database ids of the peers connected to c."
    (map (fn [p] p:id) (values c:peers)))

  {:fetch fetch
   :touch touch
   :broadcast broadcast
   :await-ack await-ack
   :serve serve
   :connect connect
   :peers peers})
