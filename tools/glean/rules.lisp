(elle/epoch 18)
# audited: 2026-11-23
# Glean's rule for each kind: the facts one function gives, point by point, and the kinds it marks.
# tools/glean/overview.md

(fn []
  (def kinds ((import "./kinds")))
  (def {:view view :edges edges} ((import "./points")))
  (def {:naming naming} ((import "./names")))
  (def {:dead-stores dead-stores} ((import "./live")))
  (def {:add add :mark mark :call-facts call-facts} ((import "./calls")))

  (defn each-named [names regs f]
    "Call F with the value each register of REGS names, skipping those that
  name none."
    (each r in regs
      (let [v (names:name r)]
        (unless (nil? v) (f v)))))

  (defn kind-facts [ctx facts marks node p]
    "The facts the rule for NODE's kind gives at P, beside its next and use."
    (def names (get ctx :names))
    (def kind (get node :kind))
    (defn named [field]
      (names:name (get node field)))
    (def d (when (has? node :dst) (named :dst)))
    (cond
      (has? kinds:marked kind) (mark marks kind)
      (or (= kind :const) (has? kinds:computed kind)) (add facts :immediate d)
      (= kind :value-const)
        (let [v (get node :value)]
          (cond
            (kinds:immediate? v) (add facts :immediate d)
            (not (fn? v)) (add facts :free [:const p])))
      (= kind :store-local)
        (let [v (named :src)
              s (names:slot-name (get node :slot))]
          (when (and (not (contains? (get ctx :dead) p))
                     (names:fills? (get node :src)) (not= v s))
            (add facts :pass [v s p])))
      (= kind :materialize-const)
        (begin
          (add facts :alloc [d p])
          (add facts :immutable d))
      (= kind :list)
        (begin
          (add facts :alloc [d p])
          (each-named names [(get node :head) (get node :tail)]
                      (fn [x] (add facts :operand [x d p])))
          (add facts :immutable d))
      (= kind :make-array-mut)
        (begin
          (add facts :alloc [d p])
          (each-named names (get node :elements)
                      (fn [x] (add facts :operand [x d p]))))
      (= kind :make-closure)
        (begin
          (add facts :alloc [d p])
          (each-named names (get node :captures)
                      (fn [x] (add facts :capture [x d p]))))
      (has? kinds:reads kind)
        (let [c (named (get kinds:reads kind))]
          (unless (nil? c) (add facts :read [d c p])))
      (or (= kind :call) (= kind :suspending-call))
        (call-facts ctx facts marks node p)
      (= kind :tail-call)
        (when (call-facts ctx facts marks node p) (add facts :return [d p]))
      (= kind :load-resume-value) (add facts :alloc [d p])
      (= kind :match-fail)
        (begin
          (each-named names [(get node :src)] (fn [s] (add facts :cross [s p])))
          (add facts :alloc [d p]))
      (= kind :eval)
        (let [env (named :env)]
          (if (or (nil? env) (kinds:immediate-name? env))
            (add facts :alloc [d p])
            (mark marks kind)))
      (= kind :return)
        (each-named names [(get node :value)] (fn [v] (add facts :return [v p])))
      (= kind :emit)
        (each-named names [(get node :value)] (fn [v] (add facts :cross [v p])))))

  (defn frozen [facts]
    "FACTS with each key's tuples frozen."
    (from-pairs (map (fn [kv] [(get kv 0) (freeze (get kv 1))]) (pairs facts))))

  (defn function-facts [unit-ctx fun]
    "The struct glean answers for FUN: its id, its name, its facts and its
  marks. UNIT-CTX holds the natives and the unit's closure table."
    (def seen (view fun))
    (def names (naming fun seen))
    (def ctx (merge unit-ctx {:names names :dead (dead-stores fun seen)}))
    (def facts @{})
    (def marks @{})
    (each i in (range (get fun :params))
      (add facts :param [(names:slot-name i) i]))
    (each i in (range (length (get fun :captures)))
      (let [n (names:capture-name i)]
        (add facts :free n)
        (add facts :captured n)))
    (each p in (range (length (get seen :nodes)))
      (let [node (get (get (get seen :nodes) p) :node)]
        (each-named names (kinds:used node) (fn [v] (add facts :use [v p])))
        (each e in (edges seen p)
          (add facts :next e))
        (kind-facts ctx facts marks node p)))
    {:id (get fun :id)
     :name (get fun :name)
     :facts (frozen facts)
     :marks (freeze marks)})

  {:function-facts function-facts})
