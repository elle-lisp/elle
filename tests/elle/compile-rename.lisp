(elle/epoch 7)
# ── compile/rename tests ──────────────────────────────────────────────

# ── Basic rename ─────────────────────────────────────────────────────

(def src "(defn add [a b] (+ a b))
(defn use-add [x] (add x 1))")
(def a (compile/analyze src))

(def result (compile/rename a :add :sum))
(assert (> (get result :edits) 0) "rename produced edits")

# Verify the new source has 'sum' instead of 'add'
(def new-src (get result :source))
(assert (not (nil? new-src)) "rename produced source")

# The new source should compile cleanly
(def a2 (compile/analyze new-src))
(def syms (compile/symbols a2))
(def sum-sym (first (filter (fn [s] (= (get s :name) "sum")) syms)))
(assert (not (nil? sum-sym)) "renamed symbol 'sum' exists")

# Old name should not appear as a function
(def add-matches (filter (fn [s] (= (get s :name) "add")) syms))
(assert (empty? add-matches) "old name 'add' is gone")

# ── Shadowed names are NOT renamed ───────────────────────────────────

(def shadow-src "
(defn outer [x]
  (let [x (+ x 1)]
    x))
")
(def a3 (compile/analyze shadow-src))
# Rename outer's parameter 'x' — the let-bound 'x' is a different binding
(def r3 (compile/rename a3 :x :y))
(assert (> (get r3 :edits) 0) "shadow rename produced edits")

# The new source should compile
(def a4 (compile/analyze (get r3 :source)))
(assert (= (type a4) :analysis) "shadowed rename compiles")

# ── Rename with multiple references ──────────────────────────────────

(def multi-src "
(defn double [n] (* n 2))
(defn triple [n] (* n 3))
(defn use-both [x] (+ (double x) (triple x)))
")
(def a5 (compile/analyze multi-src))
(def r5 (compile/rename a5 :double :twice))
(def new5 (get r5 :source))

# Verify the rename happened
(def a6 (compile/analyze new5))
(def twice-sym (first (filter (fn [s] (= (get s :name) "twice")) (compile/symbols a6))))
(assert (not (nil? twice-sym)) "twice exists after rename")

# triple should still be there
(def triple-sym (first (filter (fn [s] (= (get s :name) "triple")) (compile/symbols a6))))
(assert (not (nil? triple-sym)) "triple is untouched")

(println "all compile/rename tests passed")
