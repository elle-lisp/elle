(elle/epoch 18)
# audited: 2026-11-20
# compile/call-graph: each node names a caller or callee once, and roots and leaves read the functions the source defines.
# docs/analysis/portrait.md

(def g
  (compile/analyze "(defn leaf [x] (+ x 1))
(defn mid [x] (leaf (leaf x)))
(defn top [x] (mid x))
(defn spin [n] (if (= n 0) 0 (spin (- n 1))))"))
(def graph (compile/call-graph g))
(defn node [name]
  (find (fn [n] (= (get n :name) name)) (get graph :nodes)))

# The counter-factual counted a self-call as a caller, so a self-recursive
# function was never a root.
(assert (= ["spin" "top"] (get graph :roots))
        "a root is a function no other function calls")

# The counter-factual counted a call to a primitive, so no function that
# added two numbers was a leaf.
(assert (= ["leaf" "spin"] (get graph :leaves))
        "a leaf calls no other function the source defines")

# The counter-factual listed a name once for each call.
(assert (= ["leaf"] (get (node "mid") :callees)) "a node names each callee once")
(assert (= ["mid"] (get (node "leaf") :callers)) "a node names each caller once")

# A function that makes no call is no node.
(def quiet (compile/call-graph (compile/analyze "(defn k [x] x)")))
(assert (empty? (get quiet :nodes)) "k makes no call")
(assert (= ["k"] (get quiet :leaves)) "k is a leaf")

(println "compile-call-graph: all tests passed")
