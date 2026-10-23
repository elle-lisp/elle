(elle/epoch 15)
# audited: 2026-10-21
## The writes through a std/gun chain: put, set, put-content and unset, each planned, merged under the lock and sent.
## lib/gun/chains.md
## lib/gun.md
## lib/gun/overview.md
##
## Loaded via:
##   (def write ((import "std/gun/write") :value value :graph graph :core core
##                                        :sea sea :mesh mesh :chain chain))
##
## Exports: {:put :set :put-content :unset}

(fn [&named value graph core sea mesh chain]

  (def uuid ((import "std/uuid")))
  # Each function here names its chain argument chain, so the module is chains.
  (def chains chain)

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
      (chains:chain? v) (value:link (chains:soul v))
      (graph:node-value? v)
        (let [@out (freeze v)]
          (each [k child] in v
            (when (or (chains:chain? child) (graph:node-value? child))
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
        (chains:fail :not-a-node "a root holds a node, not a value"
                     :path (chains:path chain)))
      (let [[soul i] (chains:follow-links chain:core chain:soul keys
            (if node? n (- n 1)))
            reached [chain:soul ;(slice keys 0 i)]
            rest (slice keys i)]
        (graph:flatten soul (nest rest v) st reached link-of))))

  (defn write [chain v link-of ack]
    "Replace each chain in v with a link to its soul, read the path from the
     peers, merge v under the lock, send what the merge accepted to the
     peers, and notify. With ack, wait for an ack."
    (chains:require-soul chain)
    (chains:require-plain chain)
    (let [db chain:core
          v (linked v)]
      (core:walk db chain:soul chain:keys (chains:fetching db))
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
      (chains:fail :not-a-node "a set member is a node, not a value"
                   :path (chains:path chain)))
    (let [db chain:core
          space (sea:space-of chain:soul)
          soul (if space
                 (graph:child-soul space (uuid:v4))
                 (graph:root-soul (uuid:v4)))]
      (write (chains:get chain soul) v
             (fn [at key] (if (= key soul) soul (core:link-at db at key))) ack)
      (chains:make-chain db soul [] chain:signer)))

  (defn put-content [chain v &named ack]
    "Write v into the content node of the database of chain, under the key
     that content-key answers for v, and answer that key. :ack is as for
     put. See lib/gun/sea.md."
    (let [k (sea:content-key v)]
      (chain-put (chains:make-chain chain:core sea:content-soul []) {k v}
                 :ack ack)
      k))

  (defn chain-unset [chain member &named ack]
    "Remove member, a chain or a link, from the collection at chain: write a
     tombstone to each field of the collection's node that links to the
     member's soul, as a put does, and answer nil. When no field links to
     it, write nothing and wait for no ack."
    (chains:require-soul chain)
    (chains:require-plain chain)
    (let [soul (if (chains:chain? member)
                 (chains:soul member)
                 (value:link-soul member))
          node (chains:value chain)
          @gone {}]
      (when (graph:node-value? node)
        (each [key v] in node
          (when (and (value:link? v) (= (value:link-soul v) soul))
            (assign gone (put gone key nil)))))
      (unless (empty? gone) (chain-put chain gone :ack ack))
      nil))

  {:put chain-put :set chain-set :put-content put-content :unset chain-unset})
