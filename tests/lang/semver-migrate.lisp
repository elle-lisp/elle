(elle/epoch 12)
# audited: 2026-09-21
# elle semver migrate — consumers repaired from a library's shipped
# rules. docs/semver.md fixes the surface this file pins.
#
# The counter-factual: rules must chain ascending by major, or a
# consumer two majors behind lands on names the middle major already
# renamed away.

(def exe (elle/executable))

(defn run-tool [dir args]
  (subprocess/system exe args {:cwd dir}))

(def lib-text
  (string "(elle/epoch 12)\n" "(elle/version \"3.0.0\")\n" "(elle/migration 2\n"
          "  \"f became g; bump swapped its arguments into increment\"\n"
          "  (rename f g)\n" "  (replace (bump $1 $2) (increment $2 $1))\n"
          "  (remove legacy \"use g\")\n"
          "  (warn parse \"parse now answers a struct\"))\n"
          "(elle/migration 3\n" "  (rename g h))\n" "(fn []\n"
          "  (letrec [h (fn [a] a)\n" "           increment (fn [a b] a)\n"
          "           parse (fn [s] s)]\n"
          "    {:h h :increment increment :parse parse}))\n"))

(def consumer-text
  (string "(elle/epoch 12)\n" "(def m ((import \"lib/x\")))\n" "(m:f 1)\n"
          "(m:bump a b)\n" "(m:legacy 2)\n" "(m:parse \"s\")\n"))

(def migrated-text
  (string "(elle/epoch 12)\n" "(def m ((import \"lib/x\")))\n" "(m:h 1)\n"
          "(m:increment b a)\n" "(m:legacy 2)\n" "(m:parse \"s\")\n"))

(with-temp-dir dir
               (let [con-path (path/join dir "consumer.lisp")]
                 (file/mkdir-all (path/join dir "lib"))
                 (file/write (path/join dir "lib/x.lisp") lib-text)
                 (file/write con-path consumer-text)

                 # ── --check sees the pending migration ───────────────
                 (let [r (run-tool dir
                                   ["semver" "migrate" "lib/x.lisp"
                                    "consumer.lisp" "--check"])]
                   (assert (= (r :exit) 1) "an unmigrated consumer fails")
                   (assert (= (file/read con-path) consumer-text)
                           "--check writes nothing"))

                 # ── --dry-run reports and writes nothing ─────────────
                 (let [r (run-tool dir
                                   ["semver" "migrate" "lib/x.lisp"
                                    "consumer.lisp" "--dry-run"])]
                   (assert (= (r :exit) 0) "a dry run is not a verdict")
                   (assert (= (file/read con-path) consumer-text)
                           "--dry-run writes nothing"))

                 # ── apply: renames chain across majors ───────────────
                 (let [r (run-tool dir
                                   ["semver" "migrate" "lib/x.lisp"
                                    "consumer.lisp"])]
                   (assert (= (r :exit) 0) "migrate applies and exits 0")
                   (assert (string/contains? (r :stdout) "use g")
                           "the removed export's message is shipped")
                   (assert (string/contains? (r :stdout)
                           "parse now answers a struct")
                           "the warn message is shipped"))
                 (assert (= (file/read con-path) migrated-text)
                         "f chains to h, bump swaps into increment")

                 # ── the removed export still gates --check ───────────
                 (let [r (run-tool dir
                                   ["semver" "migrate" "lib/x.lisp"
                                    "consumer.lisp" "--check"])]
                   (assert (= (r :exit) 1) "a removed export in use still fails"))

                 # ── --from skips crossed majors ──────────────────────
                 (file/write con-path consumer-text)
                 (let [r (run-tool dir
                                   ["semver" "migrate" "lib/x.lisp"
                                    "consumer.lisp" "--from" "3"])]
                   (assert (= (r :exit) 0) "nothing left to apply")
                   (assert (= (file/read con-path) consumer-text)
                           "--from 3 skips both majors"))

                 # ── an unseen import shape is reported, not guessed ──
                 (let [odd-path (path/join dir "odd.lisp")]
                   (file/write odd-path
                               (string "(elle/epoch 12)\n"
                                       "(def pick (fn [] (import \"lib/x\")))\n"
                                       "(pick)\n"))
                   (let [r (run-tool dir
                                     ["semver" "migrate" "lib/x.lisp" "odd.lisp"])]
                     (assert (= (r :exit) 0) "reporting is not failing")
                     (assert (string/contains? (r :stdout) "manual")
                             "the import the tool cannot see through")
                     (assert (= (file/read odd-path)
                                (string "(elle/epoch 12)\n"
                                        "(def pick (fn [] (import \"lib/x\")))\n"
                                        "(pick)\n")) "and nothing was guessed")))))

(println "semver-migrate: all tests passed")
