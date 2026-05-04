(elle/epoch 10)
# ── compile/analyze: compiler-as-library tests ──────────────────────────

# ── Basic analysis ──────────────────────────────────────────────────────

(def src "(defn add [a b] (+ a b))")
(def a (compile/analyze src))
(assert (= (type a) :analysis) "analysis handle type is analysis")

# ── Diagnostics ─────────────────────────────────────────────────────────

(def diags (compile/diagnostics a))
(assert (array? diags) "diagnostics returns array")

# ── Symbols ─────────────────────────────────────────────────────────────

(def syms (compile/symbols a))
(assert (array? syms) "symbols returns array")
(assert (not (empty? syms)) "symbols not empty")

# Find the 'add' symbol
(def add-sym (first (filter (fn [s] (= (get s :name) "add")) syms)))
(assert (not (nil? add-sym)) "found add symbol")
(assert (= (get add-sym :kind) :function) "add is a function")

# ── Signal queries ──────────────────────────────────────────────────────

(def sig (compile/signal a :add))
# add calls stdlib + (a closure), so signal inference conservatively
# includes :error and other flags from the generic call path.
# Any signal (including :error) is a potential suspension, so add is
# not jit-eligible — stdlib + can error on non-numeric arguments.
(assert (not (get sig :silent)) "add is not silent (calls stdlib +)")
(assert (not (get sig :jit-eligible)) "add is not jit-eligible (may error)")
(assert (get sig :yields) "add may suspend (stdlib + can error)")

# ── Bindings ────────────────────────────────────────────────────────────

(def bindings (compile/bindings a))
(assert (array? bindings) "bindings returns array")

# ── Binding detail ──────────────────────────────────────────────────────

(def b-a (compile/binding a :a))
(assert (= (get b-a :scope) :parameter) "a is a parameter")
(assert (not (get b-a :mutated)) "a is not mutated")

(def b-b (compile/binding a :b))
(assert (= (get b-b :scope) :parameter) "b is a parameter")

# ── Call graph ──────────────────────────────────────────────────────────

(def graph (compile/call-graph a))
(assert (array? (get graph :nodes)) "call-graph has nodes")
(assert (array? (get graph :roots)) "call-graph has roots")
(assert (array? (get graph :leaves)) "call-graph has leaves")

# ── Callees ─────────────────────────────────────────────────────────────

(def callees (compile/callees a :add))
(assert (array? callees) "callees returns array")
# add calls +
(assert (not (empty? callees)) "add has callees")
(assert (= (get (first callees) :name) "+") "add calls +")

# ── Callers ─────────────────────────────────────────────────────────────

(def callers (compile/callers a :add))
(assert (array? callers) "callers returns array")
# add has no callers in this single-function analysis
(assert (empty? callers) "add has no callers")

# ── Captures ────────────────────────────────────────────────────────────

(def caps (compile/captures a :add))
(assert (array? caps) "captures returns array")
(assert (empty? caps) "add captures nothing")

# ── Query signal ────────────────────────────────────────────────────────

(def silent-fns (compile/query-signal a :silent))
(assert (array? silent-fns) "query-signal returns array")
# add has SIG_ERROR so it's not silent — no silent functions here
(assert (empty? silent-fns) "no silent functions (add has SIG_ERROR)")

# ── Multi-function analysis ─────────────────────────────────────────────

(def src2 "
(defn pure-fn [x] (* x x))
(defn caller [x] (pure-fn (+ x 1)))
")
(def a2 (compile/analyze src2))

# Both have SIG_ERROR (from * and +). Any signal is a potential
# suspension (fiber transfer), so neither is jit-eligible.
(assert (not (get (compile/signal a2 :pure-fn) :silent))
        "pure-fn has SIG_ERROR from *")
(assert (not (get (compile/signal a2 :caller) :silent))
        "caller has SIG_ERROR from + and pure-fn")
(assert (not (get (compile/signal a2 :pure-fn) :jit-eligible))
        "pure-fn may error → not jit-eligible")
(assert (not (get (compile/signal a2 :caller) :jit-eligible))
        "caller may error → not jit-eligible")

# caller calls pure-fn
(def callees2 (compile/callees a2 :caller))
(defn has-callee? [callees name]
  (not (empty? (filter (fn [c] (= (get c :name) name)) callees))))
(assert (has-callee? callees2 "pure-fn") "caller calls pure-fn")
(assert (has-callee? callees2 "+") "caller calls +")

# pure-fn is called by caller
(def callers2 (compile/callers a2 :pure-fn))
(assert (not (empty? callers2)) "pure-fn has callers")
(assert (= (get (first callers2) :name) "caller") "pure-fn called by caller")

# ── Closure with captures ──────────────────────────────────────────────

(def src3
  "
(defn make-counter [start]
  (var n start)
  (defn next [] (assign n (+ n 1)) n)
  next)
")
(def a3 (compile/analyze src3))
(def caps3 (compile/captures a3 :next))
(assert (not (empty? caps3)) "next captures something")
(assert (= (get (first caps3) :name) "n") "next captures n")
(assert (= (get (first caps3) :kind) :lbox) "n captured as lbox (mutable)")

# ── Analysis with :file option ──────────────────────────────────────────

(def a4 (compile/analyze "(def x 1)" {:file "test.lisp"}))
(assert (= (type a4) :analysis) "analysis with file option works")

# ── Error on bad source ─────────────────────────────────────────────────

(let [[ok? _] (protect (compile/analyze "(defn"))]
  (assert (not ok?) "bad source produces error"))

(println "all compile/analyze tests passed")
