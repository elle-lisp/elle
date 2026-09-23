(elle/epoch 12)
# audited: 2026-09-21
# elle semver — the command end to end over a fixture module: initial,
# release, unchanged, floor and verdict, JSON, refusals, exit codes.
# docs/semver.md fixes the surface this file pins.
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

(def v4
  (string "(elle/epoch 12)\n" "(elle/version \"1.1.1\")\n" "(fn []\n"
          "  (letrec [g (fn [b] b)]\n" "    {:g g}))\n"))

(def v5
  (string "(elle/epoch 12)\n" "(fn []\n" "  (letrec [f (fn [a] a)]\n"
          "    {:f f}))\n"))

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
                         "(version \"1.1.0\")") "the baseline moved")

                 # ── zero arguments walks from the cwd ────────────────
                 (let [r (run-tool dir ["semver"])]
                   (assert (= (r :exit) 0) "one unchanged module, exit 0")
                   (assert (string/contains? (r :stdout) "std/x")
                           "the walk names what it found"))

                 # ── a major break must outclaim the floor ────────────
                 (file/write mod-path v4)
                 (let [r (run-tool dir ["semver" "release" "lib/x.lisp"])]
                   (assert (= (r :exit) 1)
                           "a patch claim cannot cover a major break"))

                 # ── tool errors are exit 2 ───────────────────────────
                 (file/write mod-path v5)
                 (let [r (run-tool dir ["semver" "lib/x.lisp"])]
                   (assert (= (r :exit) 2) "a missing version form exits 2"))
                 (let [r (run-tool dir ["semver" "lib/absent.lisp"])]
                   (assert (= (r :exit) 2) "an unreadable module exits 2"))))

(println "semver-tool: all tests passed")
