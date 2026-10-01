(elle/epoch 14)
# audited: 2026-09-30
# elle semver judges a claimed version against the baseline: insufficient, JSON, refusal, then a sufficient claim.
# docs/semver.md
#
# The counter-factual: the exit code is the whole CI contract — a tool
# that prints INSUFFICIENT but exits 0 gates nothing.

(def exe (elle/executable))

(defn run-tool [dir args]
  (subprocess/system exe args {:cwd dir}))

(def v1
  (string "(elle/epoch 12)\n" "(elle/version \"1.0.0\")\n" "(fn []\n"
          "  (letrec [f (fn [a] a)]\n" "    {:f f}))\n"))

(def v2
  (string "(elle/epoch 12)\n" "(elle/version \"1.0.0\")\n" "(fn []\n"
          "  (letrec [f (fn [a] a)\n" "           g (fn [b] b)]\n"
          "    {:f f :g g}))\n"))

(def v3 (string/replace v2 "1.0.0" "1.1.0"))

(with-temp-dir dir (file/mkdir-all (path/join dir "lib"))
               (let [mod-path (path/join dir "lib/x.lisp")
                     surf-path (path/join dir "lib/x.surface")]
                 (file/write mod-path v1)
                 (let [r (run-tool dir ["semver" "release" "lib/x.lisp"])]
                   (assert (= (r :exit) 0) "the 1.0.0 baseline is released"))

                 # ── a minor change under an unmoved claim ────────────
                 (file/write mod-path v2)
                 (let [r (run-tool dir ["semver" "lib/x.lisp"])]
                   (assert (= (r :exit) 1) "an insufficient claim exits 1")
                   (assert (string/contains? (r :stdout) "INSUFFICIENT")
                           "the verdict is loud")
                   (assert (string/contains? (r :stdout) "1.1.0")
                           "and names the requirement"))
                 (let [r (run-tool dir ["semver" "lib/x.lisp" "--json"])
                       j (json/parse (r :stdout) :keys :keyword)]
                   (assert (= (r :exit) 1) "--json changes no exit code")
                   (assert (= (j :module) "std/x") "module")
                   (assert (= (j :baseline) "1.0.0") "baseline")
                   (assert (= (j :claimed) "1.0.0") "claimed")
                   (assert (= (j :floor) "minor") "floor")
                   (assert (= (j :required) "1.1.0") "required")
                   (assert (= (j :verdict) "insufficient") "verdict")
                   (let [c (first (->list (j :changes)))]
                     (assert (= (c :export) "g") "the change names its export")
                     (assert (= (c :change) "export-added") "and its kind")
                     (assert (= (c :floor) "minor") "and its floor")))

                 # ── release refuses the insufficient claim ───────────
                 (let [r (run-tool dir ["semver" "release" "lib/x.lisp"])]
                   (assert (= (r :exit) 1) "release refuses"))
                 (assert (string/contains? (file/read surf-path)
                         "(version \"1.0.0\")")
                         "a refusal leaves the baseline untouched")

                 # ── a sufficient claim moves the baseline ────────────
                 (file/write mod-path v3)
                 (let [r (run-tool dir ["semver" "lib/x.lisp"])]
                   (assert (= (r :exit) 0) "a sufficient claim exits 0"))
                 (let [r (run-tool dir ["semver" "release" "lib/x.lisp"])]
                   (assert (= (r :exit) 0) "release accepts"))
                 (assert (string/contains? (file/read surf-path)
                         "(version \"1.1.0\")") "the baseline moved")))
