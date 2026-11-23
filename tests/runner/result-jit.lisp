(elle/epoch 18)
## audited: 2026-11-23
## Under the rig, each form's result records the JIT compiles its VM took and how many were rejected; under elle, none.
## tests/runner/overview.md
## docs/test-store.md
##
## The JIT columns come from the rig's `jit/stats` (rig/overview.md), so the
## scenarios that read them run `elle-rig test`, and a run under `elle test`
## must leave them NULL rather than read a counter a user build lacks. The
## counter-factual: a count of compiles alone cannot say how much of a `jit`
## row ran native.

(include-file "lib/harness.lisp")

(with-temp-dir dir
  (defn green-on [runner name src]
    "Run one fixture holding SRC into a store of its own under RUNNER,
     elle-test or rig-test, and require a green gate."
    (let [r (runner (path/join dir (string name ".db"))
                    [(write-fixture dir (string name ".lisp") src)])]
      (assert (= r:exit 0)
              (string name ": the fixture must gate green; stderr: " r:err))
      r:db))

  (defn imported [db name]
    "A store of its own that DB's runs were imported into."
    (let [here (path/join dir (string name "-here.db"))
          r (elle-test here ["--import" db])]
      (assert (= r:exit 0) (string "the import failed; stderr: " r:err))
      here))

  # The JIT columns as a reading, with no more rejections than compiles, and
  # as a reading of time spent. `>= 0` is false of a NULL.
  (def jit-counted
    (string "jit_compiles >= 0 AND jit_rejected >= 0 "
            "AND jit_rejected <= jit_compiles AND jit_wall_us >= 0 "
            "AND jit_cpu_us >= 0"))
  (def jit-spent "jit_compiles >= 1 AND jit_wall_us > 0 AND jit_cpu_us > 0")

  (eprintln "scenario: a run under elle records no JIT count")
  (every-row (green-on elle-test "user-form" "(assert (> 2 1) \"ordered\")\n")
             (string "wall_ms IS NOT NULL AND cpu_us IS NOT NULL AND "
                     jit-uncounted)
             "every tier's row records wall and CPU time, and no JIT count a user build cannot read")

  (eprintln "scenario: a single form records its JIT compiles on every tier under the rig")
  # The jit tier runs the form through `compile/run-on :jit`, which compiles
  # it, so that row has a compile to count whatever else the JIT does.
  (let [db (green-on rig-test "form" "(assert (> 2 1) \"ordered\")\n")
        forced (string "tier = 'jit' AND " jit-spent)]
    (every-row db
               (string "wall_ms IS NOT NULL AND cpu_us IS NOT NULL AND "
                       jit-counted)
               "every tier's row records wall and CPU time, and its VM's JIT compiles")
    (when jit?
      (one-row db forced
               "the jit row counts the compile compile/run-on ran, on both clocks")
      (one-row (imported db "form") forced
               "the imported jit row keeps its JIT compiles")))

  # The JIT refuses a closure that calls eval, so the jit tier skips the form,
  # and the compile compile/run-on tried counts as rejected.
  (when jit?
    (eprintln "scenario: the jit row of a form the JIT refuses counts the rejection")
    (let [db (green-on rig-test "refused"
                       "(assert (= (eval '1) 1) \"evaluated\")\n")
          refused (string "tier = 'jit' AND status = 'skip' AND jit_rejected >= 1 "
                          "AND " jit-counted)]
      (one-row db refused "the jit row counts the compile the JIT rejected")
      (one-row (imported db "refused") refused
               "the imported jit row keeps its rejections")))

  # Which functions a policy compiles is the JIT's business (tests/impl); the
  # runner's is to record its VM's reading. With the JIT off there is none.
  (eprintln "scenario: a script records the JIT compiles of each policy")
  (let [db (green-on rig-test "jit-script"
                     "(ev/sleep 0.01)\n(assert true \"slept\")\n")]
    (every-row db jit-counted "every policy's row records its VM's JIT compiles")
    (one-row db
             (string "tier = 'vm' AND jit_compiles = 0 AND jit_rejected = 0 "
                     "AND jit_wall_us = 0 AND jit_cpu_us = 0")
             "the row with the JIT off counts no compile"))

  # SQLite drops a column on request, which is how the older table is made.
  (eprintln "scenario: a store from before the JIT columns gains them")
  (let [db (path/join dir "migrate-jit.db")]
    (each col ["jit_compiles" "jit_rejected" "jit_wall_us" "jit_cpu_us"]
      (query! db (string "ALTER TABLE result DROP COLUMN " col)))
    (let [r (rig-test db
                      [(write-fixture dir "migrate-jit.lisp"
                                      "(assert true \"ok\")\n")])]
      (assert (= r:exit 0) (string "the run must gate green; stderr: " r:err))
      (every-row db jit-counted
                 "the migrated store must record each row's JIT compiles")))

  (eprintln "all result-jit scenarios passed"))
