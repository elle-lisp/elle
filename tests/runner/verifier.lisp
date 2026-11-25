(elle/epoch 18)
## audited: 2026-11-23
## Which processes run Cranelift's verifier: `elle` as its flag and build say, `elle test` and its children always, and the test recipes.
## tests/runner/overview.md
## docs/impl/verifier.md
##
## src/config/tests/verifier.rs pins what the parser reads. This file pins what
## the binary does with it: the setting reaches the VM a program reads it from,
## `elle test` turns it on whatever its flags say, and a child it starts gets it
## on its command line. No environment variable carries the setting, so every
## run below sets the old one and shows that nothing reads it.

(include-file "lib/harness.lisp")
(include-file "lib/make.lisp")

(def var "ELLE_JIT_VERIFY")
(def verifies
  "(assert (= (vm/config :jit-verify) true) \"this test process verifies\")")

(defn env-at [value]
  "The environment with the old variable at VALUE, or unset for nil."
  (if (nil? value) (env-without var) (put (env-without var) var value)))

(defn reads [args value]
  "What `(vm/config :jit-verify)` prints in `elle ARGS`, the old variable at
   VALUE."
  (let [r (elle-run [;args "-e" "(println (vm/config :jit-verify))"]
                    :env (env-at value))]
    (assert (= r:exit 0)
            (string "elle " args " with " var "=" value " exited " r:exit ": "
                    r:err))
    (string/trim r:out)))

# The verifier serves the suites and their recipes, not a user program.
(eprintln "scenario: --help names neither the flag nor the variable")
(let [r (elle-run ["--help"] :env (env-at nil))]
  (assert (= r:exit 0) (string "elle --help exited " r:exit ": " r:err))
  (assert (not (string/contains? r:out "jit-verify"))
          (string "--help must not list --jit-verify: " r:out))
  (assert (not (string/contains? r:out var))
          (string "--help must not name " var ": " r:out)))

# The build decides the default, and src/config/tests/verifier.rs pins which.
# A run without the flag is the baseline, so this file runs against a debug
# binary or a release one.
(def default (reads [] nil))
(assert (contains? |"true" "false"| default)
        (string "a run without the flag reads a boolean, got " default))

# The counter-factual: a binary that still read the variable would turn the
# verifier on, or stop at `1`, because a shell exported a name it no longer
# documents.
(eprintln "scenario: the old variable does nothing")
(each value ["on" "off" "1"]
  (assert (= (reads [] value) default)
          (string var "=" value " moved the verifier off the default")))

(eprintln "scenario: the flag turns the verifier on and off")
(assert (= (reads ["--jit-verify=on"] "off") "true") "the flag turns it on")
(assert (= (reads ["--jit-verify=off"] "on") "false") "the flag turns it off")

(eprintln "scenario: a bad flag value stops elle before the program runs")
(let [r (elle-run ["--jit-verify=1" "-e" "(println :ran)"] :env (env-at nil))]
  (assert (not (= r:exit 0)) "elle ran with --jit-verify=1")
  (assert (not (string/contains? r:out "ran"))
          "the program ran before the refusal")
  (assert (and (string/contains? r:err "--jit-verify")
               (string/contains? r:err "1"))
          (string "the refusal must name the flag and the value: " r:err)))

(with-temp-dir dir
  # The counter-factual: a runner that took the build's default would test the
  # translator unverified on every release binary.
  (eprintln "scenario: elle test verifies")
  (each value [nil "off"]
    (let [r (elle-test (path/join dir (string "runner-" value ".db"))
                       ["-e" verifies] :env (env-at value))]
      (assert (= r:exit 0)
              (string "elle test with " var "=" value " exited " r:exit ": "
                      r:err))))

  # An `--isolate` child is a plain `elle FILE`, so the runner names the flag
  # on its command line. The trap: a debug child verifies by default, so only a
  # release binary shows a runner that forgot the flag.
  (eprintln "scenario: a child of elle test verifies")
  (let [r (elle-test (path/join dir "child.db")
                     ["--isolate" ""
                      (write-fixture dir "child.lisp" (string verifies "\n"))]
                     :env (env-at "off"))]
    (assert (= r:exit 0)
            (string "elle test --isolate '' exited " r:exit ": " r:err)))

  # The runner's flag comes first, so a flag the caller gives the child wins.
  (eprintln "scenario: the --isolate flags decide over the runner's")
  (let [r (elle-test (path/join dir "off.db")
                     ["--isolate" "--jit-verify=off"
                      (write-fixture dir "off.lisp"
                                     "(assert (= (vm/config :jit-verify) false) \"the child's flag decides\")\n")]
                     :env (env-at nil))]
    (assert (= r:exit 0)
            (string "elle test --isolate --jit-verify=off exited " r:exit ": "
                    r:err)))

  # A recipe that sees the variable would hand every program it runs a setting
  # nothing reads. The probe is a second makefile read after the repository's
  # through `-f`: the `make` macOS ships is GNU make 3.81, which has no
  # `--eval`.
  (eprintln "scenario: the Makefile exports no verifier variable")
  (let [probe (write-fixture dir "probe.mk"
                             (string "verifier-probe: ; @echo \"[$$" var "]\"\n"))
        r (make ["-f" "Makefile" "-f" probe "verifier-probe"]
                :env (del make-env var))]
    (assert (= r:exit 0) (string "the probe recipe failed: " r:stderr))
    (assert (= (string/trim r:stdout) "[]")
            (string "the recipe saw " var "=" (string/trim r:stdout)))))

# `make doctest` and `make smoke-plugins` run `elle` on each file directly, so
# each recipe names the flag on the command line it runs.
(eprintln "scenario: the doctest and plugin recipes pass the flag")
(each target ["doctest" "smoke-plugins"]
  (let [runs (filter (fn [l] (string/contains? l "{}"))
                     (lines (make-dry-run target)))]
    (assert (not (empty? runs)) (string "make " target " runs no file"))
    (each line runs
      (assert (string/contains? line " --jit-verify=on {}")
              (string "make " target " runs a file without the flag: " line)))))

# A CI step that sets the variable sets a name nothing reads.
(eprintln "scenario: no workflow names the variable")
(each name (filter (fn [n]
                     (or (string/ends-with? n ".yml")
                         (string/ends-with? n ".yaml")))
                   (file/ls ".github/workflows"))
  (assert (not (string/contains? (slurp (path/join ".github/workflows" name))
                                 var)) (string name " names " var)))

(eprintln "all verifier scenarios passed")
