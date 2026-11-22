(elle/epoch 18)
# audited: 2026-11-22
# Glean's names: the value each register holds, by the slot that first receives it, its capture or its constant.
# tools/glean/overview.md

(fn []
  (def {:immediate? immediate?} ((import "./kinds")))

  (defn naming [fun view]
    "The names of FUN's values, over VIEW. A slot holds a value once a
  StoreLocal stores a register that names one, and a load reads its slot, so
  the slots that hold a value are a fixpoint seeded with the parameters'.
  Answers {:name f :slot-name f :capture-name f :written f}: the value a
  register names or nil, a slot's and a capture's name, and the node that
  writes a register."
    (def slots (get fun :slots))
    (def captures (get fun :captures))
    (def ncaps (length captures))
    (def nodes (get view :nodes))
    (def writes @{})
    (def first-store @{})
    (def stores @[])
    (each p in (range (length nodes))
      (let [{:node node :term? term?} (get nodes p)]
        (unless term?
          (unless (nil? (get node :dst)) (put writes (get node :dst) p))
          (when (= (get node :kind) :store-local)
            (let [s (get node :slot)
                  r (get node :src)]
              (push stores [s r])
              (when (nil? (get first-store r)) (put first-store r s)))))))

    (defn binding-name [entry compound]
      (if (or (nil? entry) (= entry :synthetic)) compound entry))
    (defn slot-name [s]
      (binding-name (get slots s) [:slot s]))
    (defn capture-name [i]
      (binding-name (get captures i) [:capture i]))

    (def holds @{})
    (each s in (range (get fun :params))
      (put holds s true))
    (defn slot-value [s]
      (if (get holds s) (slot-name s) nil))
    (defn env-value [i]
      (if (< i ncaps) (capture-name i) (slot-value (- i ncaps))))

    (defn written [r]
      (def p (get writes r))
      (when (nil? p)
        (error {:error :glean-error
                :reason :unwritten
                :register r
                :message (string "glean: no instruction writes register " r)}))
      (get (get nodes p) :node))

    (defn name [r]
      (def p (get writes r))
      (def node (written r))
      (match (get node :kind)
        :const nil
        :value-const
          (let [v (get node :value)]
            (if (or (fn? v) (immediate? v)) nil [:const p]))
        :load-local (slot-value (get node :slot))
        :load-capture (env-value (get node :index))
        :load-capture-raw (env-value (get node :index))
        _
          (let [s (get first-store r)]
            (if (nil? s) [:reg r] (slot-name s)))))

    (var changed true)
    (while changed
      (assign changed false)
      (each store in stores
        (let [s (get store 0)]
          (when (and (not (get holds s)) (not (nil? (name (get store 1)))))
            (put holds s true)
            (assign changed true)))))

    {:name name :slot-name slot-name :capture-name capture-name :written written})

  {:naming naming})
