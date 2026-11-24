(elle/epoch 18)
# audited: 2026-11-23
# Glean's dead stores: each StoreLocal whose slot no load reads before the next store into it.
# tools/glean/overview.md

(fn []
  (def {:edges edges} ((import "./points")))

  (defn access [ncaps node]
    "[:load s] or [:store s] where NODE reads or writes slot S, or nil. An
  environment load or a StoreCapture at index i, at or past the NCAPS
  captures, reaches slot i less NCAPS."
    (def kind (get node :kind))
    (defn at [tag]
      (def i (get node :index))
      (when (>= i ncaps) [tag (- i ncaps)]))
    (cond
      (= kind :load-local) [:load (get node :slot)]
      (= kind :store-local) [:store (get node :slot)]
      (or (= kind :load-capture) (= kind :load-capture-raw)) (at :load)
      (= kind :store-capture) (at :store)
      nil))

  (defn dead-stores [fun view]
    "The points of FUN's dead StoreLocals over VIEW, as a set. A slot is live at
  a point where some path from it loads the slot before a store into it, so
  the live slots are a backward fixpoint over the next edges; an exit has
  none. A StoreLocal is dead where its slot is live at no point after it."
    (def nodes (get view :nodes))
    (def n (length nodes))
    (def ncaps (length (get fun :captures)))
    (def accesses
      (->array (map (fn [entry]
                      (unless (get entry :term?)
                        (access ncaps (get entry :node)))) nodes)))
    (def succs
      (->array (map (fn [p]
                      (filter number? (map (fn [e] (get e 1)) (edges view p))))
                    (range n))))
    (def live-in @[])
    (repeat n (push live-in ||))
    (defn live-out [p]
      (fold (fn [acc q] (union acc (get live-in q))) || (get succs p)))
    (var changed true)
    (while changed
      (assign changed false)
      (each p in (reverse (range n))
        (let [a (get accesses p)
              out (live-out p)
              in (cond
                   (nil? a) out
                   (= (get a 0) :load)
                     (union out (set (get a 1)))
                   true
                     (difference out (set (get a 1))))]
          (unless (= in (get live-in p))
            (put live-in p in)
            (assign changed true)))))
    (set ;(filter (fn [p]
                    (def a (get accesses p))
                    (and (not (nil? a))
                         (= :store-local (get (get (get nodes p) :node) :kind))
                         (not (contains? (live-out p) (get a 1))))) (range n))))

  {:dead-stores dead-stores})
