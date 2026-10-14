(elle/epoch 15)
# audited: 2026-10-14
## One gun database: its graph, store, clock, lock, subscriptions and peers; loading, the chain walk, the merges and notify.
## lib/gun.md
## lib/gun/mesh.md
## lib/gun/sea.md
##
## Loaded via:
##   (def core ((import "std/gun/core") :state state :value value :graph graph
##                                      :sea sea))
##
## Exports: {:make-core :report :load :field-of :link-at :nodes :walk :locked
##           :apply-writes :merge-remote :add-sub :notify}
##
## Every field passes sea:admit or sea:admitted before a merge sees it.
## Nothing here waits on the network. A walk takes a reach function over a
## list of souls, which the caller chooses: one that fetches from the peers,
## or one that does not.

(fn [&named state value graph sea]

  (def sync ((import "std/sync")))
  (def uuid ((import "std/uuid")))
  (def field ((import "std/gun/field")))

  ## ── The core ─────────────────────────────────────────────────────────

  (defn make-core [store clock wait on-error]
    "A database on store and clock. wait is how long a first read waits for
     the peers, in seconds, and on-error takes each error that has no caller
     to raise to."
    {:id (uuid:v4)
     :graph (graph:make-graph)
     :store store
     :clock clock
     :wait wait
     :on-error on-error
     :loaded @{}
     :fetched @{}
     :subs @{}
     :next-sub (box 0)
     :peers @{}
     :pending @{}
     :lock (sync:make-lock)})

  (defn report [core err]
    "Give err to the database's :on-error."
    (let [on-error core:on-error]
      (on-error err)))

  ## ── Reading local data ───────────────────────────────────────────────

  (defn load [core soul]
    "Read soul from the store once, drop each field that fails admission,
     observe the states of the rest, and merge them. Raises what
     sea:check-soul raises for soul."
    (unless (get core:loaded soul)
      (sea:check-soul soul)
      (let [store core:store
            stored (store:read soul)
            clock core:clock]
        (when stored
          (let [fields (sea:admitted soul stored (fn [err] (report core err)))]
            (each [_ f] in fields
              (clock:observe (field:state f)))
            (graph:merge core:graph soul fields (clock:now))))
        (put core:loaded soul true))))

  (defn field-of [core soul key]
    "The field of key in node soul, read from the store when needed."
    (load core soul)
    (graph:field core:graph soul key))

  (defn link-at [core soul key]
    "The soul that field key of node soul links to, or nil."
    (when-let [f (field-of core soul key)]
              (let [v (field:value f)]
                (when (value:link? v) (value:link-soul v)))))

  (defn node-value [core soul]
    "The live fields of node soul as a struct of key to value, or nil."
    (load core soul)
    (when-let [n (graph:node core:graph soul)]
              (let [out @{}]
                (each [key f] in n
                  (let [v (field:value f)]
                    (unless (nil? v) (put out key v))))
                (freeze out))))

  (defn nodes [core souls]
    "The graph of every node in souls that this database holds, read from the
     store when needed."
    (let [out @{}]
      (each soul in souls
        (load core soul)
        (when-let [n (graph:node core:graph soul)] (put out soul n)))
      (freeze out)))

  (defn walk-from [core start keys reach]
    "walk, from a node start that reach has already seen."
    (block :walk
      (def @soul start)
      (def @i 0)
      (while (< i (length keys))
        (let [k (get keys i)]
          (when (graph:key-range? k)
            (break :walk (when-let [n (node-value core soul)]
                                   (members core n k (slice keys (+ i 1)) reach))))
          (let [f (field-of core soul k)
                v (when f (field:value f))
                last? (= i (- (length keys) 1))]
            (cond
              (value:link? v)
                (begin
                  (assign soul (value:link-soul v))
                  (reach [soul]))
              last? (break :walk v)
              (break :walk nil))))
        (assign i (+ i 1)))
      (node-value core soul)))

  (defn members [core n r rest reach]
    "The members of the live node n under the range r, as a struct of key to
     the value that the keys rest reach from each. A member that links walks
     on from the linked node. Any other member is kept only when rest is
     empty. A member whose walk answers nil is left out. Reaches every linked
     member in one call."
    (let [out @{}
          linked @[]]
      (each [key v] in n
        (when (graph:in-range? r key)
          (cond
            (value:link? v)
              (push linked [key (value:link-soul v)])
            (empty? rest) (put out key v))))
      (unless (empty? linked)
        (reach (map (fn [[_ soul]] soul) linked)))
      (each [key soul] in linked
        (let [v (walk-from core soul rest reach)]
          (unless (nil? v) (put out key v))))
      (freeze out)))

  (defn walk [core start keys reach]
    "The value that keys reach from the node start: that node, a field's
     value, or the node a link reaches. A key range among keys answers a
     struct of each member's value, as members does. Calls (reach souls)
     with a list of the souls it is about to read, before it reads them."
    (reach [start])
    (walk-from core start keys reach))

  ## ── Merging ──────────────────────────────────────────────────────────

  (defn locked [core thunk]
    "Run thunk under the database lock."
    (let [lock core:lock]
      (lock:acquire)
      (defer
        (lock:release)
        (thunk))))

  (defn apply-writes [core writes]
    "Merge each [soul fields] of writes into memory, send what each merge
     accepted to the store, and answer the accepted graph. Raises, and merges
     nothing, when any field fails admission. The caller holds the lock."
    (let [store core:store
          clock core:clock
          accepted @{}]
      (sea:admit writes)
      (each [soul fields] in writes
        (load core soul)
        (let [diff (graph:merge core:graph soul fields (clock:now))]
          (unless (empty? diff)
            (store:write soul diff)
            (put accepted soul (merge (or (get accepted soul) {}) diff)))))
      (freeze accepted)))

  (defn merge-remote [core g]
    "Merge the graph g from a peer under the lock: observe and merge each due
     field, and store what the merge accepted. Answers [accepted later]: the
     accepted graph, and the graph of fields whose states are not due yet.
     Raises, and merges nothing of g, when any field fails admission."
    (locked core
            (fn []
              (let [store core:store
                    clock core:clock
                    accepted @{}
                    later @{}]
                (sea:admit g)
                (each [soul fields] in g
                  (load core soul)
                  (let [[due not-yet] (graph:due fields (clock:now))]
                    (each [_ f] in due
                      (clock:observe (field:state f)))
                    (let [diff (graph:merge core:graph soul due (clock:now))]
                      (unless (empty? diff)
                        (store:write soul diff)
                        (put accepted soul diff)))
                    (unless (empty? not-yet) (put later soul not-yet))))
                [(freeze accepted) (freeze later)]))))

  ## ── Subscriptions ────────────────────────────────────────────────────

  (defn add-sub [core start keys f v]
    "Watch what keys reach from the node start with f, whose last value was
     v. Answers the function that ends the watch."
    (let [id (unbox core:next-sub)]
      (rebox core:next-sub (+ id 1))
      (put core:subs id {:start start :keys keys :f f :last (box v)})
      (fn []
        (del core:subs id)
        nil)))

  (defn notify [core reach]
    "Call each subscription whose value changed since it was last called,
     walking with reach. Raises the first error a subscription raises."
    (each [id sub] in (freeze core:subs)
      (when (get core:subs id)
        (let [v (walk core sub:start sub:keys reach)]
          (unless (identical? v (unbox sub:last))
            (rebox sub:last v)
            (sub:f v))))))

  {:make-core make-core
   :report report
   :load load
   :field-of field-of
   :link-at link-at
   :nodes nodes
   :walk walk
   :locked locked
   :apply-writes apply-writes
   :merge-remote merge-remote
   :add-sub add-sub
   :notify notify})
