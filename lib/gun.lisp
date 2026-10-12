(elle/epoch 14)
# audited: 2026-10-12
## A graph database in the shape of GUN: chains over nodes merged by HAM, on a memory or Redis store, joined to peers.
## lib/gun.md
## lib/gun/overview.md
##
## Memory store only:
##   (def gun ((import "std/gun")))
##
## With the Redis store, and with the peer mesh:
##   (def gun ((import "std/gun") :redis ((import "std/redis"))
##                                :websocket ws))
##
## Usage: (def db (gun:open))
##        (gun:put (gun:get db "cfg") {:theme "dark"})
##        (gun:once (gun:get db "cfg"))

(fn [&named redis websocket]

  ## ── Submodules ───────────────────────────────────────────────────────

  (def state ((import "std/gun/state")))
  (def value ((import "std/gun/value")))
  (def ham ((import "std/gun/ham") :state state :value value))
  (def graph ((import "std/gun/graph") :state state :value value :ham ham))
  (def stores ((import "std/gun/store") :state state :value value :redis redis))
  (def wire ((import "std/gun/wire") :value value :graph graph))
  (def core ((import "std/gun/core") :state state :value value :graph graph))
  (def peer ((import "std/gun/peer") :wire wire :websocket websocket))
  (def mesh ((import "std/gun/mesh") :core core :wire wire :peer peer))
  (def uuid ((import "std/uuid")))

  (defn gun-error [reason message &named path]
    (error {:error :gun-error :reason reason :path path :message message}))

  (defn print-error [err]
    (port/write (port/stderr) (string/format "gun: {}\n" err)))

  ## ── Databases and chains ─────────────────────────────────────────────

  ## A chain is a database and a path of keys. The first key names a root.
  (defn make-chain [db path]
    {:core db :path path})

  (defn open [&named @store @clock @wait @on-error]
    "A database: the chain with no keys. :store defaults to a fresh memory
     store, and :clock to a clock on the wall clock. :wait is how long a first
     read waits for the peers, 1 second by default. :on-error takes each error
     that a write from a peer raises; the default writes it to stderr."
    (default store (stores:memory-store))
    (default clock (state:make-clock))
    (default wait 1)
    (default on-error print-error)
    (make-chain (core:make-core store clock wait on-error) []))

  (defn chain-get [chain key]
    "The chain one key further than chain. key is a string or a keyword."
    (make-chain chain:core [;chain:path (graph:key-of key chain:path)]))

  (defn require-soul [chain]
    (when (empty? chain:path)
      (gun-error :no-soul "the database itself names no node")))

  (defn fetching [db]
    (fn [soul] (mesh:fetch db soul)))

  (defn chain-value [chain]
    "What once answers for chain: the value it reaches, read from the peers
     the first time a read reaches each node."
    (require-soul chain)
    (core:walk chain:core chain:path (fetching chain:core)))

  ## ── Writing ──────────────────────────────────────────────────────────

  (defn nest [keys v]
    "v under keys, innermost last: (nest [a b] v) is {a {b v}}."
    (def @out v)
    (def @i (- (length keys) 1))
    (while (>= i 0)
      (assign out {(get keys i) out})
      (assign i (- i 1)))
    out)

  (defn plan [db path v st link-of]
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
                  (not (nil? (core:link-at db soul (get keys i)))))
        (assign soul (core:link-at db soul (get keys i)))
        (assign i (+ i 1)))
      (let [reached (slice path 0 (+ i 1))
            rest (slice keys i)]
        (graph:flatten soul (nest rest v) st reached link-of))))

  (defn write [chain v link-of ack]
    "Read the path from the peers, merge v under the lock, send what the
     merge accepted to the peers, and notify. With ack, wait for an ack."
    (require-soul chain)
    (let [db chain:core]
      (core:walk db chain:path (fetching db))
      (let [accepted (core:locked db
                                  (fn []
                                    (let [clock db:clock
                                      st (clock:tick)]
                                      (core:apply-writes db
                                      (plan db chain:path v st link-of)))))
            id (mesh:broadcast db accepted ack)]
        (core:notify db (fn [soul] (mesh:touch db soul)))
        (when ack (mesh:await-ack db id ack)))))

  (defn chain-put [chain v &named ack]
    "Write v at chain. A struct becomes a node and the rest become field
     values; see lib/gun.md for what a value may be. With :ack seconds, also
     wait for the first peer to ack."
    (write chain v (fn [soul key] (core:link-at chain:core soul key)) ack))

  (defn chain-set [chain v &named ack]
    "Add the struct v to the collection at chain under a random soul, and
     answer the chain of the new node. :ack is as for put."
    (unless (graph:node-value? v)
      (gun-error :not-a-node "a set member is a node, not a value"
                 :path chain:path))
    (let [db chain:core
          soul (graph:root-soul (uuid:v4))]
      (write (chain-get chain soul) v
             (fn [at key] (if (= key soul) soul (core:link-at db at key))) ack)
      (make-chain db [soul])))

  ## ── Subscriptions ────────────────────────────────────────────────────

  (defn chain-on [chain f]
    "Call f with the value of chain now, and again after each write that
     changes it. Answers a function that ends the subscription."
    (let [v (chain-value chain)
          stop (core:add-sub chain:core chain:path f v)]
      (let [[ok? err] (protect (f v))]
        (unless ok?
          (stop)
          (error err)))
      stop))

  ## ── Peers ────────────────────────────────────────────────────────────

  (defn serve [chain listener]
    "Accept peers for the database chain on the TCP listener, until the
     listener closes. Run it in a fiber of its own."
    (mesh:serve chain:core listener))

  (defn connect [chain url]
    "Join the database chain to the peer at the ws:// url. Answers a function
     that closes the connection."
    (mesh:connect chain:core url))

  (defn peers [chain]
    "The ids of the peers connected to the database chain."
    (mesh:peers chain:core))

  ## ── Exports ──────────────────────────────────────────────────────────

  {:open open
   :get chain-get
   :put chain-put
   :once chain-value
   :on chain-on
   :set chain-set
   :serve serve
   :connect connect
   :peers peers
   :link value:link
   :link? value:link?
   :link-soul value:link-soul
   :ham ham:verdict
   :clock state:make-clock
   :memory-store stores:memory-store
   :redis-store stores:redis-store})
