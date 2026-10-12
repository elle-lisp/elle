(elle/epoch 14)
# audited: 2026-10-11
## A graph database in the shape of GUN: chains over nodes merged by HAM, on a memory or Redis store.
## lib/gun.md
## lib/gun/overview.md
##
## Memory store only:
##   (def gun ((import "std/gun")))
##
## With the Redis store:
##   (def gun ((import "std/gun") :redis ((import "std/redis"))))
##
## Usage: (def db (gun:open))
##        (gun:put (gun:get db "cfg") {:theme "dark"})
##        (gun:once (gun:get db "cfg"))

(fn [&named redis]

  ## ── Submodules ───────────────────────────────────────────────────────

  (def state ((import "std/gun/state")))
  (def value ((import "std/gun/value")))
  (def ham ((import "std/gun/ham") :state state :value value))
  (def graph ((import "std/gun/graph") :state state :value value :ham ham))
  (def stores ((import "std/gun/store") :state state :value value :redis redis))
  (def sync ((import "std/sync")))
  (def uuid ((import "std/uuid")))

  (defn gun-error [reason message &named path]
    (error {:error :gun-error :reason reason :path path :message message}))

  ## ── Databases and chains ─────────────────────────────────────────────

  ## A core is one database: the in-memory graph, the souls already read from
  ## the store, the clock, the subscriptions, and the lock that a put holds.
  (defn make-core [store clock]
    {:graph (graph:make-graph)
     :store store
     :clock clock
     :loaded @{}
     :subs @{}
     :next-sub (box 0)
     :lock (sync:make-lock)})

  ## A chain is a core and a path of keys. The first key names a root node.
  (defn make-chain [core path]
    {:core core :path path})

  (defn open [&named @store @clock]
    "A database: the chain with no keys. :store defaults to a fresh memory
     store, and :clock to a clock on the wall clock."
    (default store (stores:memory-store))
    (default clock (state:make-clock))
    (make-chain (make-core store clock) []))

  (defn chain-get [chain key]
    "The chain one key further than chain. key is a string or a keyword."
    (make-chain chain:core [;chain:path (graph:key-of key chain:path)]))

  (defn require-soul [chain]
    (when (empty? chain:path)
      (gun-error :no-soul "the database itself names no node")))

  ## ── Reading ──────────────────────────────────────────────────────────

  (defn load [core soul]
    "Read soul from the store once, observe its states, and merge it."
    (unless (get core:loaded soul)
      (let [store core:store
            fields (store:read soul)
            clock core:clock]
        (when fields
          (each [_ [st _]] in fields
            (clock:observe st))
          (graph:merge core:graph soul fields (clock:now)))
        (put core:loaded soul true))))

  (defn field-of [core soul key]
    "The [state value] of key in node soul, read from the store when needed."
    (load core soul)
    (graph:field core:graph soul key))

  (defn link-at [core soul key]
    "The soul that field key of node soul links to, or nil."
    (when-let [f (field-of core soul key)]
              (let [v (get f 1)]
                (when (value:link? v) (value:link-soul v)))))

  (defn node-value [core soul]
    "The live fields of node soul as a struct of key to value, or nil."
    (load core soul)
    (when-let [n (graph:node core:graph soul)]
              (let [out @{}]
                (each [key [_ v]] in n
                  (unless (nil? v) (put out key v)))
                (freeze out))))

  (defn chain-value [chain]
    "What once answers for chain."
    (require-soul chain)
    (let [core chain:core
          root (first chain:path)
          keys (slice chain:path 1)]
      (block :walk
        (def @soul (graph:root-soul root))
        (def @i 0)
        (while (< i (length keys))
          (let [f (field-of core soul (get keys i))
                v (when f (get f 1))
                last? (= i (- (length keys) 1))]
            (cond
              (value:link? v) (assign soul (value:link-soul v))
              last? (break :walk v)
              (break :walk nil)))
          (assign i (+ i 1)))
        (node-value core soul))))

  ## ── Writing ──────────────────────────────────────────────────────────

  (defn nest [keys v]
    "v under keys, innermost last: (nest [a b] v) is {a {b v}}."
    (def @out v)
    (def @i (- (length keys) 1))
    (while (>= i 0)
      (assign out {(get keys i) out})
      (assign i (- i 1)))
    out)

  (defn plan [core path v st link-of]
    "The writes that put v at path with state st. The walk follows each link
     that exists, and so rewrites no link on the path that it can reach.
     From the first key with no link, the rest of the path nests v, and
     flatten writes it with link-of choosing the soul of each nested node."
    (let [root (first path)
          keys (slice path 1)
          n (length keys)
          node? (graph:node-value? v)]
      (when (and (= n 0) (not node?))
        (gun-error :not-a-node "a root holds a node, not a value" :path path))
      (def @soul (graph:root-soul root))
      (def @i 0)
      (while (and (< i n) (or (< i (- n 1)) node?)
                  (not (nil? (link-at core soul (get keys i)))))
        (assign soul (link-at core soul (get keys i)))
        (assign i (+ i 1)))
      (let [reached (slice path 0 (+ i 1))
            rest (slice keys i)]
        (graph:flatten soul (nest rest v) st reached link-of))))

  (defn apply-writes [core writes]
    "Merge each write into memory and send what it accepted to the store."
    (let [store core:store
          clock core:clock]
      (each [soul fields] in writes
        (load core soul)
        (let [diff (graph:merge core:graph soul fields (clock:now))]
          (unless (empty? diff) (store:write soul diff))))))

  (defn locked [core thunk]
    (let [lock core:lock]
      (lock:acquire)
      (defer
        (lock:release)
        (thunk))))

  (defn notify [core]
    "Call each subscription whose value changed since it was last called."
    (each [id sub] in (freeze core:subs)
      (when (get core:subs id)
        (let [v (chain-value sub:chain)]
          (unless (identical? v (unbox sub:last))
            (rebox sub:last v)
            (sub:f v))))))

  (defn write [chain v link-of]
    (require-soul chain)
    (let [core chain:core]
      (locked core
              (fn []
                (let [clock core:clock
                      st (clock:tick)
                      writes (plan core chain:path v st link-of)]
                  (apply-writes core writes))))
      (notify core)))

  (defn chain-put [chain v]
    "Write v at chain. A struct becomes a node and the rest become field
     values; see lib/gun.md for what a value may be."
    (write chain v (fn [soul key] (link-at chain:core soul key))))

  (defn chain-set [chain v]
    "Add the struct v to the collection at chain under a random soul, and
     answer the chain of the new node."
    (unless (graph:node-value? v)
      (gun-error :not-a-node "a set member is a node, not a value"
                 :path chain:path))
    (let [core chain:core
          soul (graph:root-soul (uuid:v4))]
      (write (chain-get chain soul) v
             (fn [at key] (if (= key soul) soul (link-at core at key))))
      (make-chain core [soul])))

  ## ── Subscriptions ────────────────────────────────────────────────────

  (defn make-sub [chain f v]
    {:chain chain :f f :last (box v)})

  (defn chain-on [chain f]
    "Call f with the value of chain now, and again after each write that
     changes it. Answers a function that ends the subscription."
    (let [core chain:core
          v (chain-value chain)
          id (unbox core:next-sub)
          stop (fn []
                 (del core:subs id)
                 nil)]
      (rebox core:next-sub (+ id 1))
      (put core:subs id (make-sub chain f v))
      (let [[ok? err] (protect (f v))]
        (unless ok?
          (stop)
          (error err)))
      stop))

  ## ── Exports ──────────────────────────────────────────────────────────

  {:open open
   :get chain-get
   :put chain-put
   :once chain-value
   :on chain-on
   :set chain-set
   :link value:link
   :link? value:link?
   :link-soul value:link-soul
   :ham ham:verdict
   :clock state:make-clock
   :memory-store stores:memory-store
   :redis-store stores:redis-store})
