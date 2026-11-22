(elle/epoch 18)
## audited: 2026-11-22
## Which processes run Cranelift's verifier: `elle` as asked, `elle test` and its children always, every recipe, and every CI job.
## tests/runner/overview.md
## docs/impl/verifier.md
## docs/config.md
##
## src/config/tests/verifier.rs pins what the parser reads. This file pins what
## the binary does with it: the setting reaches the VM a program reads it from,
## `elle test` turns it on whatever its environment says, and a child it starts
## inherits it. Every run clears the variable first, because the Makefile
## exports it to the suite that runs this file.

(include-file "lib/harness.lisp")
(include-file "lib/make.lisp")

(def var "ELLE_JIT_VERIFY")
(def verifies
  "(assert (= (vm/config :jit-verify) true) \"this test process verifies\")")

(defn env-at [value]
  "The environment with the variable at VALUE, or unset for nil."
  (if (nil? value) (env-without var) (put (env-without var) var value)))

(defn reads [args value]
  "What `(vm/config :jit-verify)` prints in `elle ARGS`, the variable at VALUE."
  (let [r (elle-run [;args "-e" "(println (vm/config :jit-verify))"]
                    :env (env-at value))]
    (assert (= r:exit 0)
            (string "elle " args " with " var "=" value " exited " r:exit ": "
                    r:err))
    (string/trim r:out)))

(eprintln "scenario: elle does not verify unless asked")
(assert (= (reads [] nil) "false") "elle verifies with the variable unset")
(assert (= (reads [] "") "false") "an empty variable is unset")

(eprintln "scenario: the variable turns the verifier on and off")
(assert (= (reads [] "on") "true") "the variable at on verifies")
(assert (= (reads [] "off") "false") "the variable at off does not")

(eprintln "scenario: the flag wins over the variable")
(assert (= (reads ["--jit-verify=on"] nil) "true") "the flag turns it on")
(assert (= (reads ["--jit-verify=off"] "on") "false")
        "the flag turns it off over the variable")

# The counter-factual: a value read as off would let `ELLE_JIT_VERIFY=1` run a
# whole suite unverified and report a pass.
(eprintln "scenario: a bad value stops elle before the program runs")
(let [r (elle-run ["-e" "(println :ran)"] :env (env-at "1"))]
  (assert (not (= r:exit 0)) (string "elle ran with " var "=1"))
  (assert (not (string/contains? r:out "ran"))
          "the program ran before the refusal")
  (assert (and (string/contains? r:err var) (string/contains? r:err "1"))
          (string "the refusal must name the variable and the value: " r:err)))

(eprintln "scenario: --help names the flag and the variable")
(let [r (elle-run ["--help"] :env (env-at nil))]
  (assert (and (string/contains? r:out "--jit-verify")
               (string/contains? r:out var))
          (string "--help must list --jit-verify and " var ": " r:out)))

(with-temp-dir dir
  # The counter-factual for `off`: a runner that left the setting to its
  # environment would test the translator unverified whenever a shell
  # exported the user default.
  (eprintln "scenario: elle test verifies whatever its environment says")
  (each value [nil "off"]
    (let [r (elle-test (path/join dir (string "runner-" value ".db"))
                       ["-e" verifies] :env (env-at value))]
      (assert (= r:exit 0)
              (string "elle test with " var "=" value " exited " r:exit ": "
                      r:err))))

  # An `--isolate` child is a plain `elle FILE`, which reads only its
  # environment, so the runner has to hand the setting down.
  (eprintln "scenario: a child of elle test verifies")
  (let [r (elle-test (path/join dir "child.db")
                     ["--isolate" ""
                      (write-fixture dir "child.lisp" (string verifies "\n"))]
                     :env (env-at nil))]
    (assert (= r:exit 0)
            (string "elle test --isolate '' exited " r:exit ": " r:err)))

  # Every recipe sees the variable, so `make doctest` and `make smoke-plugins`,
  # which run `elle` on a file directly, verify as the suites do. The probe is
  # a second makefile read after the repository's through `-f`: the `make`
  # macOS ships is GNU make 3.81, which has no `--eval`.
  (eprintln "scenario: the Makefile exports the variable to every recipe")
  (let [probe (write-fixture dir "probe.mk"
                             (string "verifier-probe: ; @printenv " var "\n"))
        r (make ["-f" "Makefile" "-f" probe "verifier-probe"]
                :env (del make-env var))]
    (assert (= r:exit 0) (string "the probe recipe failed: " r:stderr))
    (assert (= (string/trim r:stdout) "on")
            (string "the recipe saw " var "=" (string/trim r:stdout)))))

# A CI step that calls `cargo test` runs no recipe, so the workflow has to set
# the variable itself, for every job, and no job may set it otherwise.
(eprintln "scenario: every workflow sets the variable for all its jobs")
(let [workflows (filter (fn [n]
                          (or (string/ends-with? n ".yml")
                              (string/ends-with? n ".yaml")))
                        (file/ls ".github/workflows"))
      wanted (string "  " var ": \"on\"")]
  (assert (not (empty? workflows)) "no workflow file read")
  (each name workflows
    (let [text (string/split (slurp (path/join ".github/workflows" name)) "\n")
          top (take-while (fn [l] (or (string/starts-with? l " ") (empty? l)))
                          (rest (drop-while (fn [l] (not (= l "env:"))) text)))
          settings (filter (fn [l]
                             (string/starts-with? (string/trim l)
                               (string var ":"))) text)]
      (assert (any? (fn [l] (= l wanted)) top)
              (string name ": the top-level `env:` must hold "
                      (string/trim wanted)))
      (assert (= (length settings) 1)
              (string name ": a job sets " var " over the workflow's")))))

(eprintln "all verifier scenarios passed")
