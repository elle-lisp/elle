(elle/epoch 15)
# audited: 2026-10-21
## A std/gun chain: the database, soul and keys it names, and get, map, soul and once over it.
## lib/gun/chains.md
## lib/gun/overview.md
##
## Loaded via:
##   (def chain ((import "std/gun/chain") :value value :graph graph :core core
##                                        :mesh mesh))
##
## Exports: {:make-chain :chain? :path :require-soul :require-plain :map-chain?
##           :fetching :follow-links :soul :get :map :value :fail}
##
## A chain is a database, the soul of the node it starts at, the keys it
## follows from there, and the pair that signs what it writes, or nil. The
## database itself starts at no soul. A key is a string, or a key range that
## map adds, and a chain that holds a range only reads. A chain carries the
## :Gun protocol, whose :soul is chain-soul, so the functions up to
## chain-soul take a chain but build none. :fail raises a gun-error that
## names a key path, as write.lisp raises it too.

(fn [&named value graph core mesh]

  (defn gun-error [reason message &named path]
    (error {:error :gun-error :reason reason :path path :message message}))

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

  {:make-chain make-chain
   :chain? chain?
   :path chain-path
   :require-soul require-soul
   :require-plain require-plain
   :map-chain? map-chain?
   :fetching fetching
   :follow-links follow-links
   :soul chain-soul
   :get chain-get
   :map chain-map
   :value chain-value
   :fail gun-error})
