(elle/epoch 12)
# audited: 2026-09-21
# elle semver check — old-test arbitration over a fixture repository.
# docs/semver.md fixes the mechanics this file pins.
#
# The counter-factual: a behavior change under an unchanged shape moves
# no floor, so only the previous release's tests can reject the patch
# claim. A check that never ran them would pass everything here.

# Gate on libgit2: arbitration resolves the baseline through the
# repository, so without the library there is nothing to test.
(def _libgit2
  (let [r (protect (ffi/native "libgit2.so"))]
    (if (get r 0)
      true
      (error (struct :error :gated :reason "libgit2.so not installed")))))

(def git ((import "std/git")))
(def exe (elle/executable))

(defn run-tool [dir args]
  (subprocess/system exe args {:cwd dir}))

(defn module-text [version body]
  (string "(elle/epoch 12)\n(elle/version \"" version "\")\n(fn []\n"
          "  (letrec [f " body "]\n    {:f f}))\n"))

# The old test imports by the spec the worktree's search path resolves:
# the child runs with the worktree as its working directory, and "lib/x"
# reaches lib/x.lisp there. The "std/" prefix names the interpreter's
# own stdlib and would not reach a fixture tree.
(def old-test
  (string "(elle/epoch 12)\n" "(def m ((import \"lib/x\")))\n"
          "(assert (= (m:f 1) 1) \"f answers its argument\")\n"
          "(println \"x: ok\")\n"))

(with-temp-dir dir
               (let [repo (git:init dir)
                     mod-path (path/join dir "lib/x.lisp")]
                 (git:config-set repo "user.name" "Test")
                 (git:config-set repo "user.email" "test@test.com")
                 (file/mkdir-all (path/join dir "lib"))
                 (file/mkdir-all (path/join dir "tests/elle"))
                 (file/write mod-path (module-text "1.0.0" "(fn [a] a)"))
                 (file/write (path/join dir "tests/elle/x.lisp") old-test)
                 (git:add repo ["lib/x.lisp" "tests/elle/x.lisp"])
                 (git:commit repo "v1")
                 (let [r (run-tool dir ["semver" "release" "lib/x.lisp"])]
                   (assert (= (r :exit) 0) "release records the baseline"))
                 (git:add repo ["lib/x.surface"])
                 (git:commit repo "release 1.0.0")

                 # ── an honest claim checks clean ─────────────────────
                 (let [r (run-tool dir ["semver" "check" "lib/x.lisp"])]
                   (assert (= (r :exit) 0) "unchanged module, old tests pass"))

                 # ── behavior mutates under an unchanged shape ────────
                 # The body must stay silent: (+ a 1) would add the
                 # :error signal bit, and that IS a surface change.
                 (file/write mod-path (module-text "1.0.1" "(fn [a] 2)"))
                 (let [r (run-tool dir ["semver" "lib/x.lisp"])]
                   (assert (= (r :exit) 0) "the floor sees nothing"))
                 (let [r (run-tool dir ["semver" "check" "lib/x.lisp"])]
                   (assert (= (r :exit) 1) "the old tests reject the claim")
                   (assert (string/contains? (r :stdout) "compat claim rejected")
                           "and the rejection is named"))
                 (let [r (run-tool dir
                                   ["semver" "check" "lib/x.lisp" "--no-tests"])]
                   (assert (= (r :exit) 0) "--no-tests skips arbitration"))

                 # ── a compatible patch passes arbitration ────────────
                 (file/write mod-path (module-text "1.0.1" "(fn [a] a)"))
                 (let [r (run-tool dir ["semver" "check" "lib/x.lisp"])]
                   (assert (= (r :exit) 0) "old tests pass against the patch"))

                 # ── a major claim is not arbitrated ──────────────────
                 (file/write mod-path
                             (string "(elle/epoch 12)\n"
                                     "(elle/version \"2.0.0\")\n" "(fn []\n"
                                     "  (letrec [g (fn [b] b)]\n"
                                     "    {:g g}))\n"))
                 (let [r (run-tool dir ["semver" "check" "lib/x.lisp"])]
                   (assert (= (r :exit) 0)
                           "a major claim promises no compatibility"))

                 # ── unavailable arbitration: note, or --strict ───────
                 (git:close repo)
                 (file/delete-dir-all (path/join dir ".git"))
                 (file/write mod-path (module-text "1.0.1" "(fn [a] a)"))
                 (let [r (run-tool dir ["semver" "check" "lib/x.lisp"])]
                   (assert (= (r :exit) 0) "no repository: a note and a pass"))
                 (let [r (run-tool dir
                                   ["semver" "check" "lib/x.lisp" "--strict"])]
                   (assert (= (r :exit) 1)
                           "--strict fails what it cannot arbitrate"))))

(println "semver-check: all tests passed")
