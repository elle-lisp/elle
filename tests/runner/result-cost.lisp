(elle/epoch 18)
## audited: 2026-11-23
## Each result records what it cost: a form its wall and CPU time and, under the rig, its VM's JIT compiles; a child its total and peak memory.
## tests/runner/overview.md
## docs/test-store.md
##
## The counter-factual: a runner that records no cost still passes every corpus
## test, and a store full of NULLs answers "which file is slow on this build"
## with nothing. A timeout that fails in one CI job and nowhere else is then a
## guess.
##
## The JIT columns come from the rig's `jit/stats` (rig/overview.md), so the
## scenarios that read them run `elle-rig test`, and a run under `elle test`
## must leave them NULL rather than read a counter a user build lacks.

(include-file "lib/harness.lisp")

(with-temp-dir dir
  (defn run-on [runner name src extra]
    "Run one fixture holding SRC, with EXTRA flags, into a store of its own,
     under RUNNER: elle-test or rig-test."
    (runner (path/join dir (string name ".db"))
            [(write-fixture dir (string name ".lisp") src) ;extra]))

  (defn run [name src extra]
    (run-on elle-test name src extra))

  (defn green-on [runner name src extra]
    (let [r (run-on runner name src extra)]
      (assert (= r:exit 0)
              (string name ": the fixture must gate green; stderr: " r:err))
      r:db))

  (defn run-green [name src extra]
    (green-on elle-test name src extra))

  (defn rig-green [name src extra]
    (green-on rig-test name src extra))

  (defn latest [db cond]
    "How many of the latest run's results meet COND, and how many there are."
    (let [this (string "run_id = " latest-run)]
      [(scalar db
               (string "SELECT count(*) AS c FROM result WHERE " this " AND "
                       cond))
       (scalar db (string "SELECT count(*) AS c FROM result WHERE " this))]))

  (defn every-row [db cond what]
    "Require every result of the latest run, and at least one, to meet COND."
    (let [[meet total] (latest db cond)]
      (assert (> total 0) "the run recorded no result")
      (assert (= meet total) (string what ": " meet " of " total " rows"))))

  # The JIT columns three ways: each a reading, with no more rejections than
  # compiles; each a reading of time spent; and each NULL. `>= 0` is false of
  # a NULL.
  (def jit-counted
    (string "jit_compiles >= 0 AND jit_rejected >= 0 "
            "AND jit_rejected <= jit_compiles AND jit_wall_us >= 0 "
            "AND jit_cpu_us >= 0"))
  (def jit-spent "jit_compiles >= 1 AND jit_wall_us > 0 AND jit_cpu_us > 0")
  (def jit-uncounted
    (string "jit_compiles IS NULL AND jit_rejected IS NULL "
            "AND jit_wall_us IS NULL AND jit_cpu_us IS NULL"))

  (defn one-row [db cond what]
    "Require exactly one result of the latest run to meet COND."
    (let [[meet _] (latest db cond)]
      (assert (= meet 1) (string what ": " meet " rows"))))

  # A multi-form file runs as one script per JIT policy.
  (eprintln "scenario: a script records its wall and CPU time per policy")
  (let [db (run-green "script" "(ev/sleep 0.3)\n(assert true \"slept\")\n" [])]
    (every-row db "wall_ms >= 300 AND cpu_us IS NOT NULL"
               "every policy's row records a wall time that covers the sleep, and a CPU time")
    (every-row db "max_rss_kb IS NULL"
               "a form in a worker records no peak resident set"))

  # A debug build interprets each turn of the loop in tens of microseconds, so
  # a loop of millions outlives the runner's default budget there.
  (eprintln "scenario: a busy script records CPU time")
  (every-row (run-green "busy"
                        (string "(var i 0)\n(while (< i 20000) (assign i (+ i 1)))\n"
                                "(assert (= i 20000) \"counted\")\n") [])
             "cpu_us > 0" "every row of a busy script counts its CPU time")

  (eprintln "scenario: a run under elle records no JIT count")
  (every-row (run-green "user-form" "(assert (> 2 1) \"ordered\")\n" [])
             (string "wall_ms IS NOT NULL AND cpu_us IS NOT NULL AND "
                     jit-uncounted)
             "every tier's row records wall and CPU time, and no JIT count a user build cannot read")

  (eprintln "scenario: a single form records its cost on every tier under the rig")
  # The jit tier runs the form through `compile/run-on :jit`, which compiles
  # it, so that row has a compile to count whatever else the JIT does.
  (let [db (rig-green "form" "(assert (> 2 1) \"ordered\")\n" [])
        here (path/join dir "jit-here.db")
        forced (string "tier = 'jit' AND " jit-spent)]
    (every-row db
               (string "wall_ms IS NOT NULL AND cpu_us IS NOT NULL AND "
                       jit-counted)
               "every tier's row records wall and CPU time, and its VM's JIT compiles")
    (when jit?
      (one-row db forced
               "the jit row counts the compile compile/run-on ran, on both clocks")
      (let [r (elle-test here ["--import" db])]
        (assert (= r:exit 0) (string "the import failed; stderr: " r:err)))
      (one-row here forced "the imported jit row keeps its JIT compiles")))

  # The JIT refuses a closure that calls eval, so the jit tier skips the form,
  # and the compile compile/run-on tried counts as rejected.
  (when jit?
    (eprintln "scenario: the jit row of a form the JIT refuses counts the rejection")
    (let [db (rig-green "refused" "(assert (= (eval '1) 1) \"evaluated\")\n" [])
          here (path/join dir "refused-here.db")
          refused (string "tier = 'jit' AND status = 'skip' AND jit_rejected >= 1 "
                          "AND " jit-counted)]
      (one-row db refused "the jit row counts the compile the JIT rejected")
      (let [r (elle-test here ["--import" db])]
        (assert (= r:exit 0) (string "the import failed; stderr: " r:err)))
      (one-row here refused "the imported jit row keeps its rejections")))

  # Which functions a policy compiles is the JIT's business (tests/impl); the
  # runner's is to record its VM's reading. With the JIT off there is none.
  (eprintln "scenario: a script records the JIT compiles of each policy")
  (let [db (rig-green "jit-script" "(ev/sleep 0.01)\n(assert true \"slept\")\n"
                      [])]
    (every-row db jit-counted "every policy's row records its VM's JIT compiles")
    (one-row db
             (string "tier = 'vm' AND jit_compiles = 0 AND jit_rejected = 0 "
                     "AND jit_wall_us = 0 AND jit_cpu_us = 0")
             "the row with the JIT off counts no compile"))

  # Its worker never handed a CPU time back.
  (eprintln "scenario: a timeout records its wall time and no CPU time")
  (let [r (run-on rig-test "timeout" "(ev/sleep 5)\n(assert true \"never\")\n"
                  ["--timeout" "500"])]
    (assert (not (= r:exit 0)) "a timeout must gate non-zero")
    (every-row r:db
               (string "status = 'timeout' AND wall_ms >= 500 AND cpu_us IS NULL "
                       "AND " jit-uncounted)
               "every row is a timeout that waited out its budget and holds no CPU time or JIT count"))

  # The row ran nothing. The counter-factual is a zero, which reads as a form
  # that ran in no time. A file that does not read never reaches this row.
  (eprintln "scenario: a file-level error records no cost")
  (let [r (run-on rig-test "file-error"
                  "(assert (no-such-binding) \"unbound\")\n" [])]
    (assert (not (= r:exit 0)) "a file that does not compile gates non-zero")
    (every-row r:db
               (string "wall_ms IS NULL AND cpu_us IS NULL AND max_rss_kb IS NULL "
                       "AND " jit-uncounted)
               "a file-level error leaves every cost column NULL"))

  (eprintln "scenario: an isolated child records its total, and an import keeps it")
  (let [db (rig-green "isolate" "(ev/sleep 0.3)\n(assert true \"slept\")\n"
                      ["--isolate" ""])
        here (path/join dir "here.db")
        [costed total] (latest db
                               (string "tier = 'process' AND wall_ms >= 300 "
                                       "AND cpu_us > 0 AND max_rss_kb > 0"))]
    (assert (= total 1) "one path under --isolate is one row")
    (assert (= costed 1)
            "the child's row records its wall time, CPU total and peak resident set")
    (one-row db jit-uncounted
             "the child's row records no JIT count, which no VM of the runner took")
    (let [r (elle-test here ["--import" db])]
      (assert (= r:exit 0) (string "the import failed; stderr: " r:err)))
    (let [[kept imported] (latest here
                                  (string "max_rss_kb > 0 AND cpu_us > 0 "
                                    "AND wall_ms >= 300"))]
      (assert (= imported 1) "the import must land the one result")
      (assert (= kept 1) "the imported row must carry the child's cost")))

  # SQLite drops a column on request, which is how the older table is made.
  (eprintln "scenario: a store from before max_rss_kb gains the column")
  (let [db (path/join dir "migrate.db")]
    (query! db "ALTER TABLE result DROP COLUMN max_rss_kb")
    (let [r (elle-test db
                       [(write-fixture dir "migrate.lisp"
                                       "(assert true \"ok\")\n") "--isolate" ""])
          [costed total] (latest db "max_rss_kb > 0")]
      (assert (= r:exit 0) (string "the run must gate green; stderr: " r:err))
      (assert (= total 1) "one path under --isolate is one row")
      (assert (= costed 1) "the migrated store must record the child's peak")))

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

  (eprintln "all result-cost scenarios passed"))
