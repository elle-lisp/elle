(elle/epoch 8)
# ── compile/add-handler tests ─────────────────────────────────────────

# ── Yield signal wrapping ────────────────────────────────────────────

(def src "
(defn fetch [x] (println x))
(defn caller [x] (fetch x))
")
(def a (compile/analyze src))

# Verify fetch yields
(def sig (compile/signal a :fetch))
(assert (get sig :yields) "fetch yields")

(def result (compile/add-handler a :fetch :yield))
(assert (not (nil? result)) "add-handler returned a result")
(assert (>= (get result :wraps) 0) "wraps count is non-negative")
(def new-src (get result :source))
(assert (not (nil? new-src)) "source is returned")

# The new source should compile
(def a2 (compile/analyze new-src))
(assert (= (type a2) :analysis) "wrapped source compiles")

# ── Function that doesn't emit the signal ────────────────────────────

(def src2 "
(defn pure [x] (+ x 1))
(defn caller [x] (pure x))
")
(def a3 (compile/analyze src2))

(let [[ok? _] (protect (compile/add-handler a3 :pure :yield))]
  (assert (not ok?) "add-handler errors when function doesn't emit the signal"))

# ── Non-existent function ────────────────────────────────────────────

(let [[ok? _] (protect (compile/add-handler a :nonexistent :yield))]
  (assert (not ok?) "add-handler errors for nonexistent function"))

(println "all compile/add-handler tests passed")
