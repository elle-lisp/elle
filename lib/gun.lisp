(elle/epoch 15)
# audited: 2026-10-14
## A graph database in the shape of GUN: chains over nodes merged by HAM, on a memory or Redis store, joined to peers.
## lib/gun.md
## lib/gun/sea.md
## lib/gun/overview.md
##
## Memory store only:
##   (def gun ((import "std/gun")))
##
## With the Redis store, the peer mesh, and user spaces:
##   (def gun ((import "std/gun") :redis ((import "std/redis"))
##                                :websocket ws
##                                :crypto (import "plugin/crypto")
##                                :random (import "plugin/random")))
##
## Usage: (def db (gun:open))
##        (gun:put (gun:get db "cfg") {:theme "dark"})
##        (gun:once (gun:get db "cfg"))

(fn [&named redis websocket crypto random]

  ## ── Submodules ───────────────────────────────────────────────────────

  (def state ((import "std/gun/state")))
  (def value ((import "std/gun/value")))
  (def ham ((import "std/gun/ham") :state state :value value))
  (def graph ((import "std/gun/graph") :state state :value value :ham ham))
  (def stores ((import "std/gun/store") :state state :value value :redis redis))
  (def wire ((import "std/gun/wire") :value value :graph graph))
  (def sea ((import "std/gun/sea") :value value :crypto crypto :random random))
  (def core
    ((import "std/gun/core") :state state :value value :graph graph :sea sea))
  (def peer ((import "std/gun/peer") :wire wire :websocket websocket))
  (def mesh ((import "std/gun/mesh") :core core :wire wire :peer peer))
  (def uuid ((import "std/uuid")))

  (defn gun-error [reason message &named path]
    (error {:error :gun-error :reason reason :path path :message message}))

  (defn print-error [err]
    (port/write (port/stderr) (string/format "gun: {}\n" err)))

  ## ── Databases and chains ─────────────────────────────────────────────

  ## A chain is a database, the soul of the node it starts at, the keys it
  ## follows from there, and the pair that signs what it writes, or nil. The
  ## database itself starts at no soul.
  (defn make-chain [db soul keys &opt signer]
    {:core db :soul soul :keys keys :signer signer})

  (defn chain-path [chain]
    "The start soul and the keys of chain, as an error names them."
    (if (nil? chain:soul) [] [chain:soul ;chain:keys]))

  (defn open [&named @store @clock @wait @on-error]
    "A database: the chain with no keys. :store defaults to a fresh memory
     store, and :clock to a clock on the wall clock. :wait is how long a first
     read waits for the peers, 1 second by default. :on-error takes each error
     that a write from a peer raises; the default writes it to stderr."
    (default store (stores:memory-store))
    (default clock (state:make-clock))
    (default wait 1)
    (default on-error print-error)
    (make-chain (core:make-core store clock wait on-error) nil []))

  (defn chain-get [chain key]
    "The chain one key further than chain. key is a string or a keyword. On
     the database, key names a root node."
    (let [k (graph:key-of key (chain-path chain))]
      (if (nil? chain:soul)
        (make-chain chain:core (graph:root-soul k) [] chain:signer)
        (make-chain chain:core chain:soul [;chain:keys k] chain:signer))))

  (defn require-soul [chain]
    (when (nil? chain:soul)
      (gun-error :no-soul "the database itself names no node")))

  (defn fetching [db]
    (fn [souls] (mesh:fetch db souls)))

  (defn chain-value [chain]
    "What once answers for chain: the value it reaches, read from the peers
     the first time a read reaches each node."
    (require-soul chain)
    (core:walk chain:core chain:soul chain:keys (fetching chain:core)))

  ## ── Writing ──────────────────────────────────────────────────────────

  (defn nest [keys v]
    "v under keys, innermost last: (nest [a b] v) is {a {b v}}."
    (def @out v)
    (def @i (- (length keys) 1))
    (while (>= i 0)
      (assign out {(get keys i) out})
      (assign i (- i 1)))
    out)

  (defn plan [chain v st link-of]
    "The writes that put v at chain with state st. The walk follows each link
     that exists, and so rewrites no link on the path that it can reach.
     From the first key with no link, the rest of the keys nest v, and
     flatten writes it with link-of choosing the soul of each nested node."
    (let [db chain:core
          keys chain:keys
          n (length keys)
          node? (graph:node-value? v)]
      (when (and (= n 0) (not node?))
        (gun-error :not-a-node "a root holds a node, not a value"
                   :path (chain-path chain)))
      (def @soul chain:soul)
      (def @i 0)
      (while (and (< i n) (or (< i (- n 1)) node?)
                  (not (nil? (core:link-at db soul (get keys i)))))
        (assign soul (core:link-at db soul (get keys i)))
        (assign i (+ i 1)))
      (let [reached [chain:soul ;(slice keys 0 i)]
            rest (slice keys i)]
        (graph:flatten soul (nest rest v) st reached link-of))))

  (defn write [chain v link-of ack]
    "Read the path from the peers, merge v under the lock, send what the
     merge accepted to the peers, and notify. With ack, wait for an ack."
    (require-soul chain)
    (let [db chain:core]
      (core:walk db chain:soul chain:keys (fetching db))
      (let [accepted (core:locked db
                                  (fn []
                                    (let [clock db:clock
                                      st (clock:tick)]
                                      (core:apply-writes db
                                      (sea:sign chain:signer
                                      (plan chain v st link-of))))))
            id (mesh:broadcast db accepted ack)]
        (let [[ok? err] (protect (core:notify db
                                 (fn [souls] (mesh:touch db souls))))]
          (unless ok?
            (when ack (mesh:forget db id))
            (error err)))
        (when ack (mesh:await-ack db id ack)))))

  (defn chain-put [chain v &named ack]
    "Write v at chain. A struct becomes a node and the rest become field
     values; see lib/gun.md for what a value may be. With :ack seconds, also
     wait for the first peer to ack."
    (write chain v (fn [soul key] (core:link-at chain:core soul key)) ack))

  (defn chain-set [chain v &named ack]
    "Add the struct v to the collection at chain under a random soul, and
     answer the chain of the new node. In a user space, the new node's soul
     is in the same space. :ack is as for put."
    (unless (graph:node-value? v)
      (gun-error :not-a-node "a set member is a node, not a value"
                 :path (chain-path chain)))
    (let [db chain:core
          space (sea:space-of chain:soul)
          soul (if space
                 (graph:child-soul space (uuid:v4))
                 (graph:root-soul (uuid:v4)))]
      (write (chain-get chain soul) v
             (fn [at key] (if (= key soul) soul (core:link-at db at key))) ack)
      (make-chain db soul [] chain:signer)))

  ## ── Subscriptions ────────────────────────────────────────────────────

  (defn chain-on [chain f]
    "Call f with the value of chain now, and again after each write that
     changes it. Answers a function that ends the subscription."
    (let [v (chain-value chain)
          stop (core:add-sub chain:core chain:soul chain:keys f v)]
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

  ## ── User spaces ──────────────────────────────────────────────────────

  (defn user [chain pair]
    "The chain at the root of the space of pair, on the database chain. What
     it writes into that space, it signs with pair. Writes nothing itself."
    (let [root (string "~" pair:pub)]
      (sea:check-soul root)
      (make-chain chain:core root [] pair)))

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
   :redis-store stores:redis-store
   :pair sea:pair
   :user user
   :secret sea:secret
   :encrypt sea:encrypt
   :decrypt sea:decrypt})
