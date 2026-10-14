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
  ## database itself starts at no soul. A key is a string, or a key range
  ## that gun:map adds, and a chain that holds a range only reads. A chain
  ## carries the :Gun protocol, whose :soul is chain-soul, so the functions
  ## up to chain-soul take a chain but build none.

  (defn chain-path [chain]
    "The start soul and the keys of chain, as an error names them."
    (if (nil? chain:soul) [] [chain:soul ;chain:keys]))

  (defn require-soul [chain]
    (when (nil? chain:soul)
      (gun-error :no-soul "the database itself names no node")))

  (defn map-chain? [chain]
    "True when chain holds a key range."
    (not (nil? (find graph:key-range? chain:keys))))

  (defn require-plain [chain]
    (when (map-chain? chain)
      (gun-error :map-chain "a map chain only reads" :path (chain-path chain))))

  (defn fetching [db]
    (fn [souls] (mesh:fetch db souls)))

  (defn follow-links [db start keys limit]
    "Follow the links that the first limit keys hold, from the node start,
     up to the first key that holds no link. Answers [soul i]: the node
     reached, and how many keys led there."
    (def @soul start)
    (def @i 0)
    (while (and (< i limit) (not (nil? (core:link-at db soul (get keys i)))))
      (assign soul (core:link-at db soul (get keys i)))
      (assign i (+ i 1)))
    [soul i])

  (defn chain-soul [chain]
    "The soul that a put of a struct at chain writes to. Follows each link
     on the path, read from the peers as a put reads it, and derives a child
     soul for each key after the last link."
    (require-soul chain)
    (require-plain chain)
    (let [db chain:core
          keys chain:keys]
      (core:walk db chain:soul keys (fetching db))
      (let [[soul i] (follow-links db chain:soul keys (length keys))]
        (fold graph:child-soul soul (slice keys i)))))

  (def chain-traits
    {:Gun {:kind (fn [self] :chain) :soul (fn [self] (chain-soul self))}})

  (defn make-chain [db soul keys &opt signer]
    (with-traits {:core db :soul soul :keys keys :signer signer} chain-traits))

  (defn chain? [v]
    "True when v is a chain."
    (= (value:kind-of v) :chain))

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

  (defn chain-map [chain &named prefix from to]
    "The map chain over the members of the node that chain reaches: its
     fields whose keys start with :prefix, are at least :from, and are before
     :to. get extends it past the members. See lib/gun/chains.md."
    (require-soul chain)
    (make-chain chain:core chain:soul
                [;chain:keys (graph:key-range prefix from to)] chain:signer))

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

  (defn linked [v]
    "v with each chain in it, at the top or nested in a struct, replaced by a
     link to the chain's soul."
    (cond
      (chain? v) (value:link (chain-soul v))
      (graph:node-value? v)
        (let [@out (freeze v)]
          (each [k child] in v
            (when (or (chain? child) (graph:node-value? child))
              (assign out (put out k (linked child)))))
          out)
      v))

  (defn plan [chain v st link-of]
    "The writes that put v at chain with state st. The walk follows each link
     that exists, and so rewrites no link on the path that it can reach. A
     value that is not a node replaces the last field, link or not. From the
     first key with no link, the rest of the keys nest v, and flatten writes
     it with link-of choosing the soul of each nested node."
    (let [keys chain:keys
          n (length keys)
          node? (graph:node-value? v)]
      (when (and (= n 0) (not node?))
        (gun-error :not-a-node "a root holds a node, not a value"
                   :path (chain-path chain)))
      (let [[soul i] (follow-links chain:core chain:soul keys
                                   (if node? n (- n 1)))
            reached [chain:soul ;(slice keys 0 i)]
            rest (slice keys i)]
        (graph:flatten soul (nest rest v) st reached link-of))))

  (defn write [chain v link-of ack]
    "Replace each chain in v with a link to its soul, read the path from the
     peers, merge v under the lock, send what the merge accepted to the
     peers, and notify. With ack, wait for an ack."
    (require-soul chain)
    (require-plain chain)
    (let [db chain:core
          v (linked v)]
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

  (defn chain-unset [chain member &named ack]
    "Remove member, a chain or a link, from the collection at chain: write a
     tombstone to each field of the collection's node that links to the
     member's soul, as a put does, and answer nil. When no field links to
     it, write nothing and wait for no ack."
    (require-soul chain)
    (require-plain chain)
    (let [soul (if (chain? member) (chain-soul member) (value:link-soul member))
          node (chain-value chain)
          @gone {}]
      (when (graph:node-value? node)
        (each [key v] in node
          (when (and (value:link? v) (= (value:link-soul v) soul))
            (assign gone (put gone key nil)))))
      (unless (empty? gone) (chain-put chain gone :ack ack))
      nil))

  ## ── Subscriptions ────────────────────────────────────────────────────

  (defn member-calls [f]
    "The function of a map chain's value that calls (f v key) for each
     member whose value differs from the last value it was given, and
     (f nil key) for each member that left."
    (let [seen (box {})]
      (fn [m]
        (let [before (unbox seen)
              after (or m {})]
          (rebox seen after)
          (each [key v] in after
            (unless (identical? v (get before key)) (f v key)))
          (each [key _] in before
            (when (nil? (get after key)) (f nil key)))))))

  (defn chain-on [chain f]
    "Call f with the value of chain now, and again after each write that
     changes it. On a map chain, call (f v key) for each member now, and
     after each write for each member it changed, with nil for a member that
     left. Answers a function that ends the subscription."
    (let [v (chain-value chain)
          g (if (map-chain? chain) (member-calls f) f)
          stop (core:add-sub chain:core chain:soul chain:keys g v)]
      (let [[ok? err] (protect (g v))]
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
   :map chain-map
   :soul chain-soul
   :unset chain-unset
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
