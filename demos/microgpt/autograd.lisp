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
    (set *next-id* (+ *next-id* 1))
    @{:id id :data data :grad 0.0 :children @[] :local-grads @[]}))

# Accessors
(defn v-data [v] (get v :data))
(defn v-grad [v] (get v :grad))
(defn v-id   [v] (get v :id))

# Arithmetic operations — each returns a new Value node

(defn v+ [a b]
  "Add two Value nodes."
  (let* ([out (make-value (+ (v-data a) (v-data b)))])
    (put out :children @[a b])
    (put out :local-grads @[1.0 1.0])
    out))

(defn v* [a b]
  "Multiply two Value nodes."
  (let* ([out (make-value (* (v-data a) (v-data b)))])
    (put out :children @[a b])
    (put out :local-grads @[(v-data b) (v-data a)])
    out))

(defn v-neg [a]
  "Negate a Value node."
  (v* a (make-value -1.0)))

(defn v- [a b]
  "Subtract two Value nodes."
  (v+ a (v-neg b)))

(defn vpow [a n]
  "Raise Value node a to scalar power n (n is a plain number)."
  (let* ([out (make-value (pow (v-data a) n))])
    (put out :children @[a])
    (put out :local-grads @[(* n (pow (v-data a) (- n 1.0)))])
    out))

(defn vexp [a]
  "e^a."
  (let* ([ea (exp (v-data a))]
         [out (make-value ea)])
    (put out :children @[a])
    (put out :local-grads @[ea])
    out))

(defn vlog [a]
  "ln(a)."
  (let* ([out (make-value (log (v-data a)))])
    (put out :children @[a])
    (put out :local-grads @[(/ 1.0 (v-data a))])
    out))

(defn vrelu [a]
  "ReLU(a)."
  (let* ([d (v-data a)]
         [out (make-value (if (> d 0.0) d 0.0))])
    (put out :children @[a])
    (put out :local-grads @[(if (> d 0.0) 1.0 0.0)])
    out))

# Scalar-Value mixed ops

(defn v*s [v s]
  "Multiply Value v by scalar s."
  (v* v (make-value s)))

(defn v+s [v s]
  "Add scalar s to Value v."
  (v+ v (make-value s)))

(defn v/ [a b]
  "Divide Value a by Value b."
  (v* a (vpow b -1.0)))

# Backward pass

(defn backward [root]
  "Backpropagate gradients from root."
  (let* ([topo @[]]
         [visited @{}])

    # Topological sort (DFS, post-order)
    (letrec ([topo-sort (fn [node]
      (when (not (has-key? visited (v-id node)))
        (put visited (v-id node) true)
        (each child in (get node :children)
          (topo-sort child))
        (push topo node)))])
      (topo-sort root))

    # Zero all grads
    (each node in topo
      (put node :grad 0.0))

    # Seed root gradient
    (put root :grad 1.0)

    # Reverse accumulation
    (var i (- (length topo) 1))
    (while (>= i 0)
      (let* ([node (get topo i)]
             [children (get node :children)]
             [local-grads (get node :local-grads)]
             [node-grad (v-grad node)])
        (var j 0)
        (while (< j (length children))
          (let* ([child (get children j)]
                 [lg (get local-grads j)])
            (put child :grad (+ (v-grad child) (* node-grad lg))))
          (set j (+ j 1))))
      (set i (- i 1)))))
