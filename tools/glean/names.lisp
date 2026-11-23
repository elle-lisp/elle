(elle/epoch 18)
# audited: 2026-11-23
# Glean's names: the value each register holds, by its first slot, its capture, its constant, or its point for an immediate.
# tools/glean/overview.md

(fn []
  (def {:immediate? immediate?
        :immediate-name? immediate-name?
        :computed computed} ((import "./kinds")))

  (defn naming [fun view]
    "The names of FUN's values, over VIEW. An immediate is named by the point
  that writes it. A slot holds a value once a StoreLocal stores a register that
  fills it, and a load reads its slot, so the slots that hold a value are a
  fixpoint seeded with the parameters'. Answers {:name f :fills? f :slot-name f
  :capture-name f :written f}: the value a register names or nil, whether a
  store of a register fills its slot, a slot's and a capture's name, and the
  node that writes a register."
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
      (def kind (get node :kind))
      (cond
        (= kind :const) [:imm p]
        (= kind :value-const)
          (let [v (get node :value)]
            (cond
              (fn? v) nil
              (immediate? v) [:imm p]
              true [:const p]))
        (= kind :load-local) (slot-value (get node :slot))
        (= kind :load-capture) (env-value (get node :index))
        (= kind :load-capture-raw) (env-value (get node :index))
        (has? computed kind) [:imm p]
        true
          (let [s (get first-store r)]
            (if (nil? s) [:reg r] (slot-name s)))))

    (defn fills? [r]
      "Whether a StoreLocal of register R fills its slot: R names a value, and
  until the flip an immediate fills none."
      (def v (name r))
      (not (or (nil? v) (immediate-name? v))))

    (var changed true)
    (while changed
      (assign changed false)
      (each store in stores
        (let [s (get store 0)]
          (when (and (not (get holds s)) (fills? (get store 1)))
            (put holds s true)
            (assign changed true)))))

    {:name name
     :fills? fills?
     :slot-name slot-name
     :capture-name capture-name
     :written written})

  {:naming naming})
