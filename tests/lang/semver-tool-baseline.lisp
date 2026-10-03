(elle/epoch 14)
# audited: 2026-09-30
# elle semver before and at the first release: initial, release, unchanged, an import spec, and the walk from the cwd.
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

(with-temp-dir dir (file/mkdir-all (path/join dir "lib"))
               (let [mod-path (path/join dir "lib/x.lisp")
                     surf-path (path/join dir "lib/x.surface")]
                 (file/write mod-path v1)

                 # ── no baseline yet ──────────────────────────────────
                 (let [r (run-tool dir ["semver" "lib/x.lisp"])]
                   (assert (= (r :exit) 0) "initial exits 0")
                   (assert (string/contains? (r :stdout) "initial")
                           "the initial state is named"))
                 (assert (not (file/exists? surf-path))
                         "the dev loop writes nothing")

                 # ── release creates the baseline ─────────────────────
                 (let [r (run-tool dir ["semver" "release" "lib/x.lisp"])]
                   (assert (= (r :exit) 0) "release exits 0"))
                 (let [text (file/read surf-path)]
                   (assert (string/contains? text "(elle-surface 1)")
                           "the format header")
                   (assert (string/contains? text "(module \"std/x\")")
                           "the module spec derives from the lib path")
                   (assert (string/contains? text "(version \"1.0.0\")")
                           "the claimed version is recorded")
                   (assert (string/contains? text
                           "(export f :fn [a] :signals [])") "the export record"))

                 # ── unchanged ────────────────────────────────────────
                 (let [r (run-tool dir ["semver" "lib/x.lisp"])]
                   (assert (= (r :exit) 0) "unchanged exits 0")
                   (assert (string/contains? (r :stdout) "surface unchanged")
                           "and says so"))
                 (let [r (run-tool dir ["semver" "std/x"])]
                   (assert (= (r :exit) 0)
                           "an import spec resolves under the cwd"))

                 # ── zero arguments walks from the cwd ────────────────
                 (let [r (run-tool dir ["semver"])]
                   (assert (= (r :exit) 0) "one unchanged module, exit 0")
                   (assert (string/contains? (r :stdout) "std/x")
                           "the walk names what it found"))))
