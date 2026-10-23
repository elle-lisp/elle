(elle/epoch 15)
# audited: 2026-10-21
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
  (def wait ((import "std/gun/wait")))
  (def mesh ((import "std/gun/mesh") :core core :wire wire :wait wait))
  (def join ((import "std/gun/join") :core core :peer peer :mesh mesh))
  # Each function here names its chain argument chain, so the module is chains.
  (def chains
    ((import "std/gun/chain") :value value :graph graph :core core :mesh mesh))
  (def write
    ((import "std/gun/write") :value value :graph graph :core core :sea sea
                              :mesh mesh :chain chains))

  (defn print-error [err]
    (port/write (port/stderr) (string/format "gun: {}\n" err)))

  ## ── Databases ────────────────────────────────────────────────────────

  (defn open [&named @store @clock @wait @on-error @peers @redial]
    "A database: the chain with no keys. :store defaults to a fresh memory
     store, and :clock to a clock on the wall clock. :wait is how long a first
     read waits for the peers, 1 second by default. :on-error takes each error
     that a write from a peer, or a dial to one, raises; the default writes it
     to stderr. :peers lists ws:// urls to keep a connection to, redialing
     with the [min max] seconds of :redial, [0.1 30] by default."
    (default store (stores:memory-store))
    (default clock (state:make-clock))
    (default wait 1)
    (default on-error print-error)
    (default peers [])
    (default redial [0.1 30])
    (join:check-redial redial)
    (let [db (core:make-core store clock wait on-error)
          [lo hi] redial]
      (each url in peers
        (join:keep db url lo hi))
      (chains:make-chain db nil [])))

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
    (let [v (chains:value chain)
          g (if (chains:map-chain? chain) (member-calls f) f)
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
    (join:serve chain:core listener))

  (defn connect [chain url]
    "Join the database chain to the peer at the ws:// url. Answers a function
     that closes the connection."
    (join:connect chain:core url))

  (defn peers [chain]
    "The ids of the peers connected to the database chain."
    (join:peers chain:core))

  (defn close [chain]
    "End the keepers of the database chain, close its peers, and close each
     peer that registers after this. Local reads and writes still work."
    (join:close chain:core))

  ## ── User spaces ──────────────────────────────────────────────────────

  (defn user [chain pair]
    "The chain at the root of the space of pair, on the database chain. What
     it writes into that space, it signs with pair. Writes nothing itself."
    (let [root (string "~" pair:pub)]
      (sea:check-soul root)
      (chains:make-chain chain:core root [] pair)))

  ## ── Exports ──────────────────────────────────────────────────────────

  {:open open
   :get chains:get
   :put write:put
   :once chains:value
   :on chain-on
   :set write:set
   :map chains:map
   :soul chains:soul
   :unset write:unset
   :content-key sea:content-key
   :put-content write:put-content
   :serve serve
   :connect connect
   :peers peers
   :close close
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
