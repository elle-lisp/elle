(elle/epoch 12)
# audited: 2026-09-21
# std/semver/surface — hybrid extraction: the module loads and its
# closures answer, with static analysis supplying names and the fallback.
#
# The counter-factual: an extractor that only analyzed would go blind on
# re-exports and trait tables; one that only executed would lose param
# names and could not survive a constructor that signals.

(def surf ((import "std/semver/surface")))
(def sfile ((import "std/semver/file")))

(defn write-module [dir text]
  (let [path (path/join dir "mod.lisp")]
    (file/write path text)
    path))

# ── hybrid extraction ──────────────────────────────────────────────
(with-temp-dir dir
               (let [path (write-module dir
                                        (string "(elle/epoch 12)\n"
                                        "(elle/version \"1.2.3\")\n"
                                        "(fn [dep &opt log]\n"
                                        "  (letrec [run (fn [job] \"Run one job.\" job)\n"
                                        "           open (fn [path &named mode create?] path)\n"
                                        "           boom (fn [] (error {:error :x}))]\n"
                                        "    {:run run :open open :boom boom :limit 3}))\n"))
                     s (surf:extract {:path path :module "std/demo"})]
                 (assert (= (s :module) "std/demo") "module spec rides in")
                 (assert (= (s :version) "1.2.3")
                         "version read from the declaration")
                 (assert (= (s :mode) :hybrid) "construction succeeded")
                 (let [ctor (s :constructor)]
                   (assert (= (ctor :required) 1) "constructor requires dep")
                   (assert (= (ctor :optional) 1) "log is optional")
                   (assert (= (ctor :params) ["dep" "log"])
                           "constructor param names"))
                 (let [run ((s :exports) :run)]
                   (assert (= (run :kind) :fn) "run is a fn")
                   (assert (= (run :required) 1) "run requires one")
                   (assert (= (run :params) ["job"])
                           "static names survive the merge")
                   (assert (= (run :doc) (surf:short-hash "Run one job."))
                           "the doc rides as its short hash")
                   (assert (= ((run :signals) :bits) []) "run raises nothing"))
                 (let [open ((s :exports) :open)]
                   (assert (= (open :rest) :named) "&named collector")
                   (assert (= (open :named-keys) [:create? :mode]) "keys sorted"))
                 (let [boom ((s :exports) :boom)]
                   (assert (= ((boom :signals) :bits) [:error])
                           "boom's error bit"))
                 (let [limit ((s :exports) :limit)]
                   (assert (= (limit :kind) :value) "limit is a value")
                   (assert (= (limit :type) :integer) "limit's type")
                   (assert (= (length (limit :hash)) 8) "an 8-hex value hash"))
                 # Extraction is deterministic: two runs render byte-identically.
                 (assert (= (sfile:render s)
                            (sfile:render (surf:extract {:path path
                            :module "std/demo"})))
                         "byte-identical across extractions")))

# ── a trait-carrying export ────────────────────────────────────────
(with-temp-dir dir
               (let [path (write-module dir
                                        (string "(elle/epoch 12)\n"
                                        "(elle/version \"1.0.0\")\n" "(fn []\n"
                                        "  (letrec [deco (with-traits [1]\n"
                                        "                  @{:Collection {:length length}})]\n"
                                        "    {:deco deco :plain [1 2]}))\n"))
                     s (surf:extract {:path path :module "std/deco"})]
                 (assert (= (((s :exports) :deco) :traits)
                            {:Collection [:length]})
                         "an attached table records its methods")
                 (assert (nil? (get ((s :exports) :plain) :traits))
                         "a builtin table records nothing")))

# ── static fallback when construction signals ──────────────────────
(with-temp-dir dir
               (let [path (write-module dir
                                        (string "(elle/epoch 12)\n"
                                        "(elle/version \"0.3.0\")\n"
                                        "(fn [plugin]\n" "  (plugin:connect)\n"
                                        "  (letrec [f (fn [x y] x)]\n"
                                        "    {:f f}))\n"))
                     s (surf:extract {:path path :module "std/needy"})]
                 (assert (= (s :mode) :static)
                         "construction signaled, analysis ran")
                 (assert (= (((s :exports) :f) :required) 2)
                         "static record survives")
                 (assert (= (((s :exports) :f) :params) ["x" "y"])
                         "static names")))

# ── not a module ───────────────────────────────────────────────────
(with-temp-dir dir
               (let [path (write-module dir "(elle/epoch 12)\n(+ 1 2)\n")]
                 (let [[ok? _] (protect (surf:extract {:path path
                                        :module "std/no"}))]
                   (assert (not ok?) "a file with no surface refuses"))))

(println "semver-surface: all tests passed")
