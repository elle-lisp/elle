(elle/epoch 18)
# audited: 2026-11-24
# The registers and slots a later step reads, after each point of a function, and a state pruned to them.
# tools/glean/present.md

(fn []
  (def kinds ((import "./kinds")))
  (def {:edges edges} ((import "./points")))

  # The fields of each count instruction that hold a register.
  (def count-fields
    {:incref-value-region [:src]
     :decref-value-region [:src]
     :decref-cell-region [:src]
     :adopt-region [:parent :child]
     :adopt-cell-region [:parent :child]
     :adopt-into-activation [:child]
     :free-region-group [:members]
     :assert-region-matches [:src]})

  (defn env-slot [ncaps node]
    "The slot an environment step NODE reaches, or nil for a capture."
    (def i (get node :index))
    (when (>= i ncaps) (- i ncaps)))

  (defn reads [ncaps node]
    "The registers and slots NODE reads, as [:r n] and [:s n]."
    (def kind (get node :kind))
    # Glean's used lists no register for a StoreLocal, which names its value
    # and uses none; here it reads its source.
    (def regs
      (cond
        (has? kinds:counts kind)
          (mapcat (fn [f]
                    (def held (get node f))
                    (if (array? held) held [held])) (get count-fields kind []))
        (= kind :store-local) [(get node :src)]
        (kinds:used node)))
    (def slot
      (cond
        (= kind :load-local) (get node :slot)
        (or (= kind :load-capture) (= kind :load-capture-raw))
          (env-slot ncaps node)
        (= kind :decref-postponed) (get node :slot)))
    (def items (map (fn [r] [:r r]) regs))
    (if (nil? slot) (set ;items) (set [:s slot] ;items)))

  (defn writes [ncaps node]
    "The registers and slots NODE writes, as [:r n] and [:s n]."
    (def kind (get node :kind))
    (def slots
      (cond
        (= kind :store-local) [(get node :slot)]
        (= kind :store-capture)
          (let [s (env-slot ncaps node)]
            (if (nil? s) [] [s]))
        (= kind :decref-postponed) [(get node :slot) (get node :route)]
        []))
    (def items (map (fn [s] [:s s]) slots))
    (if (nil? (get node :dst))
      (set ;items)
      (set [:r (get node :dst)] ;items)))

  (defn live-after [fun view]
    "For each point of VIEW, the registers and slots some later step reads
  before a step writes them: a backward fixpoint over the next edges, and an
  exit has none."
    (def nodes (get view :nodes))
    (def n (length nodes))
    (def ncaps (length (get fun :captures)))
    (def uses (->array (map (fn [e] (reads ncaps (get e :node))) nodes)))
    (def defs (->array (map (fn [e] (writes ncaps (get e :node))) nodes)))
    (def succs
      (->array (map (fn [p]
                      (filter number? (map (fn [e] (get e 1)) (edges view p))))
                    (range n))))
    (def live-in @[])
    (repeat n (push live-in ||))
    (defn out [p]
      (fold (fn [acc q] (union acc (get live-in q))) || (get succs p)))
    (var changed true)
    (while changed
      (assign changed false)
      (each p in (reverse (range n))
        (let [in (union (get uses p) (difference (out p) (get defs p)))]
          (unless (= in (get live-in p))
            (put live-in p in)
            (assign changed true)))))
    (->array (map out (range n))))

  (defn prune [state live]
    "STATE with only the registers and slots of LIVE."
    (defn kept [m tag]
      (from-pairs (filter (fn [[k _]] (contains? live [tag k])) (pairs m))))
    (merge state
           {:regs (kept (get state :regs) :r)
            :slots (kept (get state :slots) :s)}))

  {:live-after live-after :prune prune})
