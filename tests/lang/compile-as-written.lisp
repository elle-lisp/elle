(elle/epoch 18)
# audited: 2026-11-20
# The compile queries report the program as its source spells it, and nothing a macro or a literal adds.
# docs/analysis/portrait.md

(defn names [entries]
  (map (fn [e] (get e :name)) entries))
(defn count-of [name all]
  (length (filter (fn [n] (= n name)) all)))

# ── A macro's calls are not the function's ──────────────────────────────
#
# `each` expands to a dispatch over the kinds of collection, and places its
# body once in each arm. The counter-factual reported more than thirty callees
# for this one call, four copies of `validate` among them.

(def a
  (compile/analyze "(defn validate [d] d)
(defn process [rows]
  (each row in rows (validate row)))"))
(assert (= ["validate"] (names (compile/callees a :process)))
        "process makes the one call it spells")
(assert (= ["process"] (names (compile/callers a :validate)))
        "validate has one call site")

# ── Each call site is one entry, with its own line ──────────────────────
#
# The counter-factual gave every caller entry the line of the caller's first
# call to the function.

(def two (compile/analyze "(defn g [x] x)
(defn f [x]
  (g x)
  (g x))"))
(assert (= [3 4] (map (fn [s] (get s :line)) (compile/callers two :g)))
        "each call site carries its own line")

# ── A literal and a qualified name make no call of their own ────────────

(def lit (compile/analyze "(defn f [a] [a {:k a} |a|])"))
(assert (empty? (compile/callees lit :f)) "a literal is not a call")

(def q (compile/analyze "(defn send [conn msg] (conn:out:write msg))"))
(assert (= ["conn:out:write"] (names (compile/callees q :send)))
        "a call through a qualified name keeps its spelling")
(assert (nil? (get (first (compile/callees q :send)) :signal))
        "nothing says what a field of conn holds")

# ── A callee carries its signal when the analysis knows it ──────────────
#
# The counter-factual named callees and nothing else, so a reader asked
# compile/signal by name, which knows no primitive.

(def s
  (compile/analyze "(defn inner [x] (+ x 1))
(defn outer [p x] (inner x) (port/read p 1) (p x))"))
(def outer-calls (compile/callees s :outer))
(assert (= ["inner" "port/read" "p"] (names outer-calls)) "outer's three calls")
(assert (= (get (get outer-calls 0) :signal) (compile/signal s :inner))
        "a callee the source defines carries its signal")
(assert (get (get (get outer-calls 1) :signal) :io)
        "a primitive callee carries its signal")
(assert (nil? (get (get outer-calls 2) :signal))
        "a parameter's signal is its caller's to say")

# ── A macro's bindings are not the source's ─────────────────────────────

(def sym-names (names (compile/symbols a)))
(assert (empty? (filter (fn [n]
                          (contains? |"seq" "cur" "idx" "len" "broke" "items"| n))
                        sym-names))
        "the bindings each makes are no symbols of the source")
(assert (= 1 (count-of "row" sym-names))
        "row is one symbol, though each places it four times")

(def bind-names (names (compile/bindings a)))
(assert (= 1 (count-of "row" bind-names)) "row is one binding")
(assert (= 1 (count-of "validate" bind-names)) "validate is listed once")
(assert (= 0 (count-of "+" bind-names))
        "a primitive is no binding of the source")
(assert (not (first (protect (compile/binding a :seq))))
        "each's seq is no binding of the source")

# ── A user macro's binding is the macro's ───────────────────────────────

(def um
  (compile/analyze "(defmacro twice (e) `(let [tmp ,e] (+ tmp tmp)))
(defn f [x] (twice x))"))
(assert (= 0 (count-of "tmp" (names (compile/symbols um))))
        "a template's binding is no symbol")

# ── Captures ────────────────────────────────────────────────────────────

(def c
  (compile/analyze "(defn helper [x] x)
(var hits 0)
(defn uses-helper [x] (helper x))
(defn bump [] (assign hits (+ hits 1)) hits)"))
(assert (empty? (compile/captures c :uses-helper))
        "an immutable binding of the file is no capture")
(assert (= ["hits"] (names (compile/captures c :bump)))
        "a var of the file is a capture: a closure can write it")

(def m
  (compile/analyze "(fn [plugin]
  (defn run [x] (plugin:go x))
  {:run run})"))
(assert (= ["plugin"] (names (compile/captures m :run)))
        "a module closure's parameter is a capture")
(assert (= ["run"] (get (compile/call-graph m) :roots))
        "the file's own expression is no function of the source")

# ── A rewrite wraps one call site once ──────────────────────────────────
#
# The counter-factual held one edge for each copy of the body, so the handler
# wrapped the same call four times.

(def h
  (compile/analyze "(defn fetch [x] (println x))
(defn caller [xs] (each x in xs (fetch x)))"))
(assert (= 1 (get (compile/add-handler h :fetch :io) :wraps))
        "the one call site is wrapped once")

(println "compile-as-written: all tests passed")
