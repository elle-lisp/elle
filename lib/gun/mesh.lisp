(elle/epoch 15)
# audited: 2026-10-22
## The std/gun mesh rules: fetch, forward, acks, deferral and resync, over the connections peer.lisp runs.
## lib/gun/mesh.md
## lib/gun/overview.md
##
## Loaded via:
##   (def mesh ((import "std/gun/mesh") :core core :wire wire :wait wait))
##
## Exports: {:fetch :touch :handlers :broadcast :forget :await-ack :closed?
##           :fail}
##
## :handlers answers the functions peer.lisp calls for a database, which
## join.lisp hands it. :fail raises the gun-error that join.lisp raises too.

(fn [&named core wire wait]

  (def sync ((import "std/sync")))
  (def uuid ((import "std/uuid")))
  (def field ((import "std/gun/field")))

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

  (defn closed? [c]
    "True once c has closed."
    (let [latch c:closed]
      (latch:open?)))

  ## ── Peers ────────────────────────────────────────────────────────────

  (defn peers-except [c except]
    "Every connected peer but the one whose key is except."
    (filter (fn [p] (not (= p:key except))) (values c:peers)))

  (defn send-to [p msg]
    "Send msg to p. A send that fails closes p."
    (let [[ok? _] (protect (p:send msg))]
      (unless ok? (p:close))))

  ## ── Fetching ─────────────────────────────────────────────────────────

  (defn unfetched [c souls]
    "The souls in souls that nobody has fetched or started to fetch."
    (filter (fn [soul] (nil? (get c:fetched soul))) souls))

  (defn fetch-souls [c souls except]
    "Fetch each soul in souls: load it, ask every peer but except, and wait
     for their answers, :wait seconds at most. A soul another fiber is
     fetching is waited for instead. A soul is pending from before its get
     leaves, which is what ends a get cycle. A fetch that skipped except
     leaves its souls unfetched, so a fiber that waited for it fetches again.
     A soul's entry in :fetched holds its :latch, the peer key it :skips,
     and whether that peer was :asked for it."
    (let [fresh @[]
          others @[]]
      (each soul in souls
        (let [f (get c:fetched soul)]
          (if f
            (push others [soul f:latch])
            (begin
              (put c:fetched soul @{:latch (sync:make-latch) :skips except})
              (push fresh soul)))))
      (defer
        (each soul in fresh
          (let [f (get c:fetched soul)]
            (when except (del c:fetched soul))
            (f:latch:open)))
        (each soul in fresh
          (core:load c soul))
        (let [targets (peers-except c except)]
          (unless (or (empty? fresh) (empty? targets))
            (let [id (uuid:v4)
                  msg (wire:get id (freeze fresh))]
              (wait:open-wait c id targets :all)
              (each p in targets
                (send-to p msg))
              (wait:await c id c:wait)))))
      (each [_ latch] in others
        (latch:wait))
      (let [again (unfetched c (map first others))]
        (unless (empty? again) (fetch-souls c again except)))))

  (defn fetch [c souls]
    "Fetch each soul in souls from the peers the first time a read reaches
     it, with one get for all of them."
    (fetch-souls c souls nil))

  ## ── Merging what a peer sends ────────────────────────────────────────

  (defn touch [c souls]
    "Load each soul in souls from the store, and start one fetch, in a new
     fiber, of those that nobody has fetched. Never waits on the network."
    (each soul in souls
      (core:load c soul))
    (let [fresh (unfetched c souls)]
      (unless (empty? fresh)
        (ev/spawn (fn [] (reporting c (fn [] (fetch c fresh)))))))
    nil)

  (defn notify [c]
    (reporting c (fn [] (core:notify c (fn [souls] (touch c souls))))))

  (defn forward [c g except]
    "Send the graph g, as a new put, to every peer but except."
    (let [msg (wire:put (uuid:v4) g)]
      (each p in (peers-except c except)
        (send-to p msg))))

  (defn earliest-due [c later]
    (let [clock c:clock
          @soonest nil]
      (each [_ fields] in later
        (each [_ f] in fields
          (let [d (clock:due-in (field:state f))]
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

  (defn ask-skipped [c p souls]
    "Send one get to each peer other than p that a pending fetch of a soul in
     souls skipped, for those souls, once per fetch. Its answer reaches p as
     gossip."
    (let [asks @{}]
      (each soul in souls
        (let [f (get c:fetched soul)]
          (when (and f f:skips (not (= f:skips p:key)) (not f:asked))
            (put f :asked true)
            (put asks f:skips [;(or (get asks f:skips) []) soul]))))
      (each [k skipped] in asks
        (when-let [q (get c:peers k)] (send-to q (wire:get (uuid:v4) skipped))))))

  (defn handle-get [c p msg]
    "Fetch from every other peer each soul of the get this database has not
     started to fetch, then answer with the nodes it holds. A pending soul
     answers from local data at once, and the peer its fetch skipped is
     asked for it."
    (ask-skipped c p msg:souls)
    (fetch-souls c (unfetched c msg:souls) p:key)
    (send-to p (wire:answer msg:id (core:nodes c msg:souls))))

  (defn handlers [c]
    "The functions peer.lisp calls for the database c."
    {:up (fn [p] (if (closed? c) (p:close) (put c:peers p:key p)))
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
                      (wait:settle c msg:ack p:key nil))
                  :ack (wait:settle c msg:ack p:key [p:id msg])
                  (core:report c
                               {:error :gun-error
                                :reason :bad-message
                                :why "a second hello"
                                :message "not a gun message: a second hello"})))
     :down (fn [p]
             (del c:peers p:key)
             (each id in [;(keys c:pending)]
               (wait:settle c id p:key nil)))
     :error (fn [err] (core:report c err))})

  ## ── A local put ──────────────────────────────────────────────────────

  (defn broadcast [c g ack?]
    "Send the graph g that a local put accepted to every peer. When ack?,
     answer the id of the wait for its first ack."
    (let [targets (peers-except c nil)
          id (uuid:v4)]
      (when ack?
        (wait:open-wait c id (if (empty? g) [] targets) :first))
      (unless (empty? g)
        (let [msg (wire:put id g)]
          (each p in targets
            (send-to p msg))))
      id))

  (defn forget [c id]
    "Drop the wait for the put id, which nobody will await."
    (del c:pending id)
    nil)

  (defn await-ack [c id seconds]
    "Wait for the first ack of the put id. Raises :no-ack when none comes
     within seconds, and :peer-error when the first ack says the merge failed."
    (let [answer (wait:await c id seconds)]
      (when (nil? answer) (gun-error :no-ack "no peer acked the put"))
      (let [[peer-id msg] answer]
        (unless msg:ok?
          (gun-error :peer-error "a peer failed to merge the put"
                     :peer-id peer-id :peer-reason msg:reason)))))

  {:fetch fetch
   :touch touch
   :handlers handlers
   :broadcast broadcast
   :forget forget
   :await-ack await-ack
   :closed? closed?
   :fail gun-error})
