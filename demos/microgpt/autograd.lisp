# Scalar autograd engine
#
# Each Value node is a mutable table with:
#   :id          unique integer (for visited-set keying)
#   :data        float (forward value)
#   :grad        float (accumulated gradient, mutated during backward)
#   :children    array of Value nodes this was computed from
#   :local-grads array of floats, d(this)/d(child_i)

(var *next-id* 0)

(defn make-value [data]
  "Create a leaf Value node (no children)."
  (let* ([id *next-id*])
     (assign *next-id* (+ *next-id* 1))
    @{:id id :data data :grad 0.0 :children @[] :local-grads @[]}))

(defn make-op [data children local-grads]
  "Create a Value node that is the result of an operation."
  (let* ([id *next-id*])
     (assign *next-id* (+ *next-id* 1))
    @{:id id :data data :grad 0.0
      :children children :local-grads local-grads}))

# Accessors
(defn v-data [v] (get v :data))
(defn v-grad [v] (get v :grad))
(defn v-id   [v] (get v :id))

# Arithmetic operations — each returns a new Value node

(defn v+ [a b]
  (make-op (+ (v-data a) (v-data b)) @[a b] @[1.0 1.0]))

(defn v* [a b]
  (make-op (* (v-data a) (v-data b)) @[a b] @[(v-data b) (v-data a)]))

(defn vneg [a]
  (make-op (- (v-data a)) @[a] @[-1.0]))

(defn vpow [a n]
  (make-op (pow (v-data a) n) @[a] @[(* n (pow (v-data a) (- n 1.0)))]))

(defn vexp [a]
  (let* ([ea (exp (v-data a))])
    (make-op ea @[a] @[ea])))

(defn vlog [a]
  (make-op (log (v-data a)) @[a] @[(/ 1.0 (v-data a))]))

(defn vrelu [a]
  (let* ([d (v-data a)])
    (make-op (if (> d 0.0) d 0.0) @[a] @[(if (> d 0.0) 1.0 0.0)])))

(defn v- [a b] (v+ a (vneg b)))
(defn v/ [a b] (v* a (vpow b -1.0)))

# Scalar-Value mixed ops

(defn v*s [v s]
  "Multiply Value v by scalar s."
  (v* v (make-value s)))

(defn v+s [v s]
  "Add scalar s to Value v."
  (v+ v (make-value s)))

# Backward pass

(defn topo-sort [root]
  "Topological sort (DFS, post-order) from root."
  (let* ([topo @[]]
         [visited @{}])
    (letrec ([walk (fn [node]
      (when (not (has? visited (v-id node)))
        (put visited (v-id node) true)
        (each child in (get node :children)
          (walk child))
        (push topo node)))])
      (walk root))
    topo))

(defn backward! [root]
  "Run backpropagation from root. Assumes all grads already zeroed."
  (let* ([topo (topo-sort root)])
    # Zero all grads
    (each node in topo
      (put node :grad 0.0))
    # Seed root gradient
    (put root :grad 1.0)
    # Reverse accumulation
    (each node in (reverse topo)
      (let* ([children   (get node :children)]
             [local-grads (get node :local-grads)]
             [node-grad  (v-grad node)])
        (var j 0)
        (while (< j (length children))
          (let* ([child (get children j)]
                 [lg    (get local-grads j)])
            (put child :grad (+ (v-grad child) (* node-grad lg))))
           (assign j (+ j 1)))))))
