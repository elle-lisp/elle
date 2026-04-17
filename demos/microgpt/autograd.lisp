(elle/epoch 7)
## autograd.lisp — Scalar autograd engine
##
## Each Value node is a mutable @struct with:
##   :id          unique integer (for visited-set keying)
##   :data        float (forward value)
##   :grad        float (accumulated gradient, mutated during backward)
##   :children    array of Value nodes this was computed from
##   :local-grads array of floats, d(this)/d(child_i)

(fn []

  # ── Value construction ─────────────────────────────────────────

  (var *next-id* 0)

  (defn make-value [data]
    "Create a leaf Value node (no children)."
    (let* [id *next-id*]
      (assign *next-id* (inc *next-id*))
      @{:id id :data data :grad 0.0 :children @[] :local-grads @[]}))

  (defn make-op [data children local-grads]
    "Create a Value node that is the result of an operation."
    (let* [id *next-id*]
      (assign *next-id* (inc *next-id*))
      @{:id id :data data :grad 0.0
        :children children :local-grads local-grads}))

  # ── Accessors ──────────────────────────────────────────────────

  (defn v-data [v] v:data)
  (defn v-grad [v] v:grad)

  # ── Arithmetic operations ──────────────────────────────────────

  (defn v+ [a b]
    (make-op (+ (v-data a) (v-data b)) @[a b] @[1.0 1.0]))

  (defn v* [a b]
    (make-op (* (v-data a) (v-data b)) @[a b] @[(v-data b) (v-data a)]))

  (defn vneg [a]
    (make-op (- (v-data a)) @[a] @[-1.0]))

  (defn vpow [a n]
    (make-op (pow (v-data a) n) @[a] @[(* n (pow (v-data a) (- n 1.0)))]))

  (defn vexp [a]
    (let* [ea (exp (v-data a))]
      (make-op ea @[a] @[ea])))

  (defn vlog [a]
    (make-op (log (v-data a)) @[a] @[(/ 1.0 (v-data a))]))

  (defn vrelu [a]
    (let* [d (v-data a)]
      (make-op (if (> d 0.0) d 0.0) @[a] @[(if (> d 0.0) 1.0 0.0)])))

  (defn v- [a b] (v+ a (vneg b)))
  (defn v/ [a b] (v* a (vpow b -1.0)))

  (defn v*s [v s]
    "Multiply Value v by scalar s."
    (v* v (make-value s)))

  (defn v+s [v s]
    "Add scalar s to Value v."
    (v+ v (make-value s)))

  # ── Fused operations (reduce node count) ───────────────────────

  (defn vdot [avec bvec n &named offset-a offset-b]
    "Fused dot product: sum(avec[i]*bvec[i]) for i in 0..n-1.
     Single Value node instead of 2n nodes.
     Optional offset-a/offset-b for sliced access."
    (default offset-a 0)
    (default offset-b 0)
    (var sum 0.0)
    (let* [children @[] grads @[]]
      (var i 0)
      (while (< i n)
        (let* [a (avec (+ offset-a i))
               b (bvec (+ offset-b i))]
          (assign sum (+ sum (* a:data b:data)))
          (push children a)
          (push children b)
          (push grads b:data)
          (push grads a:data))
        (assign i (inc i)))
      (make-op sum children grads)))

  (defn vsum [vec]
    "Fused sum: single Value node with all inputs as children."
    (var sum 0.0)
    (let [grads @[]]
      (each v in vec
        (assign sum (+ sum v:data))
        (push grads 1.0))
      (make-op sum (if (array? vec) vec (thaw (->array vec))) grads)))

  # ── Backward pass ──────────────────────────────────────────────

  (defn topo-sort [root]
    "Topological sort (DFS, post-order) from root."
    (let* [topo @[]
           visited @||]
      (letrec [walk (fn [node]
        (when (not (contains? visited node:id))
          (add visited node:id)
          (each child in node:children
            (walk child))
          (push topo node)))]
        (walk root))
      topo))

  (defn backward [root]
    "Run backpropagation from root."
    (let* [topo (topo-sort root)]
      (each node in topo
        (put node :grad 0.0))
      (put root :grad 1.0)
      (each node in (reverse topo)
        (let* [children   node:children
               local-grads node:local-grads
               node-grad  node:grad]
          (var j 0)
          (while (< j (length children))
            (let* [child (children j)
                   lg    (local-grads j)]
              (put child :grad (+ child:grad (* node-grad lg))))
            (assign j (inc j)))))))

  {:make-value make-value :make-op make-op
   :v-data v-data :v-grad v-grad
   :v+ v+ :v* v* :vneg vneg :vpow vpow :vexp vexp :vlog vlog :vrelu vrelu
   :v- v- :v/ v/ :v*s v*s :v+s v+s
   :vdot vdot :vsum vsum
   :backward backward})
