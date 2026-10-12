(elle/epoch 14)
# audited: 2026-10-11
## Souls, the in-memory nodes, the merge that answers a diff, and the flattening of a nested struct.
## lib/gun.md
## lib/gun/overview.md
##
## Loaded via:
##   (def graph ((import "std/gun/graph") :state state :value value :ham ham))
##
## Exports: {:root-soul :child-soul :key-of :make-graph :node :field :merge
##           :flatten}
##
## A node's fields are a struct of key to [state value]. A graph maps each
## soul to a mutable struct of fields.

(fn [&named state value ham]

  ## ── Souls and keys ───────────────────────────────────────────────────

  (defn escape [part]
    "part with % written as %25 and / as %2F, so a / in a soul only joins."
    (string/replace (string/replace part "%" "%25") "/" "%2F"))

  (defn root-soul [name]
    "The soul of the root node called name."
    (escape name))

  (defn child-soul [parent key]
    "The soul derived for the node under key of the node parent."
    (string parent "/" (escape key)))

  (defn invalid-key [k path]
    (error {:error :gun-error
            :reason :invalid-key
            :key k
            :path path
            :message "a key is a string or a keyword, and not _"}))

  (defn key-of [k &opt path]
    "The string key that k names: a string is itself, a keyword its name.
     Raises :invalid-key for _ and for any other type."
    (let [key (cond
                (keyword? k) (string k)
                (= (type-of k) :string) k
                (invalid-key k path))]
      (when (= key "_") (invalid-key k path))
      key))

  ## ── Nodes ────────────────────────────────────────────────────────────

  (defn make-graph []
    "An empty graph."
    @{})

  (defn node [g soul]
    "The fields of the node soul in g, or nil when g has no such node."
    (when-let [n (get g soul)] (freeze n)))

  (defn field [g soul key]
    "The [state value] of key in node soul of g, or nil."
    (when-let [n (get g soul)] (get n key)))

  (defn merge-node [g soul fields now]
    "Merge fields into node soul of g by HAM, and answer the fields accepted.
     Raises :future-state, and changes nothing, when any field carries a
     state after now."
    (let [current (or (get g soul) {})
          accepted @{}]
      (each [key [st v]] in fields
        (let [[cur-st cur-v] (or (get current key) [nil nil])]
          (case (ham:verdict now st v cur-st cur-v)
            :future (error {:error :gun-error
                            :reason :future-state
                            :soul soul
                            :key key
                            :state st
                            :now now
                            :message "a write carries a state after now"})
            :accept (put accepted key [st v])
            nil)))
      (unless (empty? accepted)
        (let [n (or (get g soul)
                    (let [fresh @{}]
                      (put g soul fresh)
                      fresh))]
          (each [key f] in accepted
            (put n key f))))
      (freeze accepted)))

  ## ── Flattening ───────────────────────────────────────────────────────

  (defn node-value? [v]
    "True when v is a struct to write as a node of its own."
    (and (struct? v) (not (value:link? v))))

  (defn flatten [soul v st path link-of]
    "The writes that put the struct v at node soul with state st: an array of
     [soul fields], the node soul first. A nested struct becomes a node of its
     own, linked from its parent. Its soul is (link-of parent key) when that
     answers one, and the derived child soul otherwise. path holds the keys
     that reach soul, and an error names the full key path of what it rejects.
     Raises :invalid-key or :invalid-value before answering anything."
    (def queue @[[soul v path]])
    (def out @[])
    (def @i 0)
    (while (< i (length queue))
      (let [[at entries at-path] (get queue i)
            fields @{}]
        (each [k child] in entries
          (let [key (key-of k [;at-path k])
                child-path [;at-path key]]
            (cond
              (node-value? child)
                (let [target (or (link-of at key) (child-soul at key))]
                  (put fields key [st (value:link target)])
                  (push queue [target child child-path]))
              (value:valid? child) (put fields key [st child])
              (error {:error :gun-error
                      :reason :invalid-value
                      :value child
                      :path child-path
                      :message "not a gun value"}))))
        (push out [at (freeze fields)]))
      (assign i (+ i 1)))
    (freeze out))

  {:root-soul root-soul
   :child-soul child-soul
   :key-of key-of
   :make-graph make-graph
   :node node
   :field field
   :merge merge-node
   :flatten flatten})
