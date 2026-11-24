(elle/epoch 18)
# audited: 2026-11-24
# What each step of a function does to the regions present walks, and the points its paths go on to.
# tools/glean/present.md

(fn []
  (def kinds ((import "./kinds")))
  (def {:edges edges} ((import "./points")))
  (def {:record record
        :with-record with-record
        :content content
        :lower lower
        :raise raise
        :adopt adopt
        :free-group free-group
        :exit exit} ((import "./regions")))
  (def {:make make} ((import "./made")))
  (def {:reg reg
        :set-reg set-reg
        :slot slot
        :set-slot set-slot
        :env env
        :named named
        :give give
        :set-content set-content
        :by-static by-static} ((import "./slots")))
  (def {:calls calls :call-step call-step} ((import "./callees")))

  (def allocations
    |:list :make-array-mut :make-closure :materialize-const :make-capture-cell
     :freeze :thaw|)

  (defn count-step [ctx state node p]
    "STATE after the count instruction NODE at P."
    (defn at [field]
      (reg ctx state (get node field)))
    (defn through [field]
      (content state (at field)))
    (case (get node :kind)
      :incref-region (by-static ctx state (get node :region-id) p :raise)
      :decref-region (by-static ctx state (get node :region-id) p :release)
      :incref-value-region (raise ctx state (through :src) p)
      :decref-value-region (lower ctx state (through :src) p :release)
      :decref-cell-region (lower ctx state (at :src) p :release)
      :decref-postponed
        (let [s (lower ctx state (slot state (get node :slot)) p :release)]
          (set-slot (set-slot s (get node :slot) :none) (get node :route) :none))
      :adopt-region (adopt state (through :child) (through :parent))
      :adopt-cell-region (adopt state (at :child) (at :parent))
      :adopt-into-activation (adopt state (through :child) :activation)
      :free-region-group
        (fold (fn [s r] (free-group ctx s (content s (reg ctx s r)) p)) state
              (get node :members))
      :assert-region-matches state))

  (defn value-step [ctx state node p]
    "STATE after the step NODE at P, which neither counts nor calls."
    (def kind (get node :kind))
    (def dst (get node :dst))
    (cond
      (or (= kind :const) (has? kinds:computed kind)) (set-reg state dst :none)
      (= kind :value-const)
        (if (kinds:immediate? (get node :value))
          (set-reg state dst :none)
          (give ctx state node p 0))
      (= kind :load-local)
        (set-reg state dst (slot state (get node :slot)))
      (= kind :store-local)
        (set-slot state (get node :slot) (reg ctx state (get node :src)))
      (= kind :load-capture)
        (set-reg state dst (content state (env ctx state (get node :index))))
      (= kind :load-capture-raw)
        (set-reg state dst (env ctx state (get node :index)))
      (= kind :store-capture)
        (let [i (get node :index)
              tok (reg ctx state (get node :src))
              target (env ctx state i)
              rec (if (= target :none) nil (record state target))]
          (cond
            (and rec (has? rec :content)) (set-content state target tok)
            (>= i (get ctx :ncaps))
              (set-slot state (- i (get ctx :ncaps)) tok)
            state))
      (= kind :load-capture-cell)
        (set-reg state dst (content state (reg ctx state (get node :cell))))
      (= kind :store-capture-cell)
        (set-content state (reg ctx state (get node :cell))
                     (reg ctx state (get node :value)))
      (= kind :make-capture-cell)
        (let [s (give ctx state node p 1)
              cell (get (get s :regs) dst)]
          (with-record s cell
                       (put (record s cell)
                            :content (reg ctx state (get node :value)))))
      (has? allocations kind) (give ctx state node p 1)
      (= kind :eval) (make ctx state p dst nil 1 (named ctx dst))
      (has? node :dst) (make ctx state p dst nil 0 (named ctx dst))
      state))

  (defn term-step [ctx state node p]
    "The points and states after the terminator NODE at P."
    (case (get node :kind)
      :return
        (let [s (lower ctx state
                       (content state (reg ctx state (get node :value))) p :hand)]
          (exit ctx s p)
          [])
      :unreachable []
      (map (fn [e] [(get e 1) state]) (edges (get ctx :view) p))))

  (defn step [ctx state p]
    "The points and states the step at P goes on to from STATE."
    (def {:node node :term? term?} (get (get (get ctx :view) :nodes) p))
    (def kind (get node :kind))
    (each r in (kinds:used node)
      (reg ctx state r))
    (cond
      term? (term-step ctx state node p)
      (has? kinds:counts kind)
        [[(+ p 1) (count-step ctx state node p)]]
      (has? calls kind) (call-step ctx state node p)
      [[(+ p 1) (value-step ctx state node p)]]))

  {:step step})
