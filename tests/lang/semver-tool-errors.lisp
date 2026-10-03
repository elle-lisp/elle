(elle/epoch 14)
# audited: 2026-09-30
# elle semver refuses a patch claim over a major break with exit 1, and reports a tool error with exit 2.
# docs/semver.md
#
# The counter-factual: the exit code is the whole CI contract — a tool
# that prints INSUFFICIENT but exits 0 gates nothing.

(def exe (elle/executable))

(defn run-tool [dir args]
  (subprocess/system exe args {:cwd dir}))

(def v3
  (string "(elle/epoch 12)\n" "(elle/version \"1.1.0\")\n" "(fn []\n"
          "  (letrec [f (fn [a] a)\n" "           g (fn [b] b)]\n"
          "    {:f f :g g}))\n"))

(def v4
  (string "(elle/epoch 12)\n" "(elle/version \"1.1.1\")\n" "(fn []\n"
          "  (letrec [g (fn [b] b)]\n" "    {:g g}))\n"))

(def v5
  (string "(elle/epoch 12)\n" "(fn []\n" "  (letrec [f (fn [a] a)]\n"
          "    {:f f}))\n"))

(with-temp-dir dir (file/mkdir-all (path/join dir "lib"))
               (let [mod-path (path/join dir "lib/x.lisp")]
                 (file/write mod-path v3)
                 (let [r (run-tool dir ["semver" "release" "lib/x.lisp"])]
                   (assert (= (r :exit) 0) "the 1.1.0 baseline is released"))

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
