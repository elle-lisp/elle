(elle/epoch 18)
# audited: 2026-11-20
# compile/captures answers the slots fn/captures reads off each compiled closure, in slot order, and tags where each came from.
# docs/analysis/portrait.md
#
# The functions below run here, and the file analyzes its own text, so each
# claim holds the query to the closure this compiler built. The
# counter-factual read the analysis, before any compile pass, and hid every
# binding of the file and of a template. It answered no slot for helper, for
# cache or for the hoisted constant, and none for the slot the graft adds to
# wrap.

(defmacro defmemo (n)
  `(begin
     (def cache @{})
     (defn ,n []
       cache)))
(defmemo memo)
(defn helper [x]
  (+ x 1))
(defn uses-helper [x]
  (helper x))
(var hits 0)
(defn bump []
  (assign hits (+ hits 1))
  hits)
(defn konst []
  [1 2 3])
(defn grafted [y]
  (helper y))
(defn wrap [z]
  (grafted z))
(defn count-down [n]
  (if (= n 0)
    0
    (+ 1 (count-down (- n 1)))))

(def self (get (meta/origin helper) :file))
(def analysis (compile/analyze (file/read self) {:file self}))

(defn field [entries key]
  (map (fn [e] (get e key)) entries))
(defn origins [name]
  (field (compile/captures analysis name) :origin))

# ── The query reads the closure ─────────────────────────────────────────

(each pair in [[:memo memo] [:uses-helper uses-helper] [:bump bump]
               [:konst konst] [:grafted grafted] [:wrap wrap]
               [:count-down count-down]]
  (let* [[name f] pair
         compiled (compile/captures analysis name)
         held (fn/captures f)]
    (assert (= (field held :name) (field compiled :name))
            (string name " answers the names of its closure's slots, in order"))
    (assert (= (field held :mutable?) (field compiled :mutated))
            (string name " answers whether code assigns each slot's binding"))))

# ── Where each slot came from ───────────────────────────────────────────

(assert (= [:source] (origins :uses-helper))
        "a binding of the file is the source's")
(assert (= [:source] (origins :bump)) "a var of the file is the source's")
(assert (= [:source] (origins :count-down))
        "a self-call holds the function's own name")
(assert (= [:macro] (origins :memo)) "the template introduces cache")
(assert (= [:compiler] (origins :konst)) "the hoist moves [1 2 3] out of konst")
(assert (nil? (get (first (compile/captures analysis :konst)) :name))
        "the hoisted constant has no name")
(assert (= ["grafted" "helper"] (field (compile/captures analysis :wrap) :name))
        "the graft puts grafted's body in wrap, and helper with it")
(assert (= [:source :compiler] (origins :wrap)) "the graft adds helper's slot")

# ── Who holds a binding ─────────────────────────────────────────────────

(assert (= ["uses-helper" "grafted" "wrap"]
           (field (compile/captured-by analysis :helper) :name))
        "every closure that holds helper, the graft's slot included")
(assert (= ["memo"] (field (compile/captured-by analysis :cache) :name))
        "a template's binding is held as any other")

(println "compile-captures: all tests passed")
