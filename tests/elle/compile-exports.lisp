(elle/epoch 12)
# audited: 2026-09-21
# compile/exports — the module surface read statically off an analysis.
#
# The counter-factual: a version-bump verifier diffs these records, so a
# field the walk drops or reshapes turns a breaking change invisible.
# Each assertion pins one field of the {:constructor :exports} contract.

# A parametric module: constructor shape, fn records, value records.
(def modsrc
  (string "(fn [dep &opt log]\n" "  (letrec [helper (fn [x] x)\n"
          "           run (fn [job] \"Run one job.\" (helper job))\n"
          "           spread (fn [a & rest] a)\n"
          "           open (fn [path &named mode create?] path)]\n"
          "    {:run run :spread spread :open open :limit 3}))"))
(def a (compile/analyze modsrc {:file "mod.lisp"}))
(def ex (compile/exports a))

(let [ctor (get ex :constructor)]
  (assert (= (get ctor :kind) :fn) "constructor is a fn record")
  (assert (= (get ctor :required) 1) "constructor requires dep")
  (assert (= (get ctor :optional) 1) "constructor: log is optional")
  (assert (= (get ctor :rest) :none) "constructor has no collector"))

(let [run (get (get ex :exports) :run)]
  (assert (= (get run :kind) :fn) "run is a fn")
  (assert (= (get run :required) 1) "run requires one")
  (assert (= (get run :optional) 0) "run has no optionals")
  (assert (= (get run :rest) :none) "run has no collector")
  (assert (= (get run :named-keys) []) "run has no named keys")
  (assert (= (get run :params) ["job"]) "run's param names")
  (assert (= (get run :doc) "Run one job.") "run's docstring")
  (assert (get run :line) "run has a line"))

(let [spread (get (get ex :exports) :spread)]
  (assert (= (get spread :rest) :list) "& collects a list")
  (assert (= (get spread :required) 1) "spread requires one"))

(let [open (get (get ex :exports) :open)]
  (assert (= (get open :rest) :named) "&named collector")
  (assert (= (get open :named-keys) [:create? :mode]) "keys sorted"))

(assert (= (get (get ex :exports) :limit) {:kind :value})
        "a non-function export is a value record")

# The exports map resolves through bindings: an export key may name a
# binding spelled differently, and a nested helper sharing an export's
# spelling never shadows the exported binding.
(def alias-src
  (string "(fn []\n" "  (letrec [go (fn [a b] a)\n"
          "           wrap (fn [x] (letrec [go (fn [] nil)] (go)))]\n"
          "    {:run go :wrap wrap}))"))
(def alias-ex (compile/exports (compile/analyze alias-src)))
(assert (= (get (get (get alias-ex :exports) :run) :required) 2)
        "export key :run reaches binding go")
(assert (= (get (get (get alias-ex :exports) :wrap) :required) 1)
        "outer wrap wins over its nested go")

# Signals ride each record in the shape compile/signal returns.
(def sig-src
  (string "(fn []\n" "  (letrec [boom (fn [] (error {:error :boom}))]\n"
          "    {:boom boom}))"))
(def sig-ex (compile/exports (compile/analyze sig-src)))
(let [sigs (get (get (get sig-ex :exports) :boom) :signals)]
  (assert (has? (get sigs :bits) :error) "boom's :error bit")
  (assert (not (get sigs :silent)) "boom is not silent"))

# A bare-struct module has exports and no constructor.
(def bare-ex (compile/exports (compile/analyze "(def f (fn [x] x)) {:f f}")))
(assert (nil? (get bare-ex :constructor)) "no module closure")
(assert (= (get (get (get bare-ex :exports) :f) :required) 1) "f exported")

# A file that returns no export struct has no surface.
(assert (nil? (compile/exports (compile/analyze "(+ 1 2)")))
        "no struct, no surface")

# A non-handle argument is an error.
(let [[ok? _] (protect (compile/exports 42))]
  (assert (not ok?) "non-handle is an error"))

(println "compile-exports: all tests passed")
