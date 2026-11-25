(elle/epoch 18)
## audited: 2026-11-24
## Each result records what it cost: a form its wall and CPU time, a child its total and peak memory, and nothing where nothing ran.
## tests/runner/overview.md
## docs/test-cost.md
## docs/test-store.md
##
## The counter-factual: a runner that records no cost still passes every corpus
## test, and a store full of NULLs answers "which file is slow on this build"
## with nothing. A timeout that fails in one CI job and nowhere else is then a
## guess.
##
## The scenarios that require a NULL JIT count run under the rig, where a
## result that ran carries one (result-jit.lisp).

(include-file "lib/harness.lisp")

(with-temp-dir dir
  (defn run-on [runner name src extra]
    "Run one fixture holding SRC, with EXTRA flags, into a store of its own,
     under RUNNER: elle-test or rig-test."
    (runner (path/join dir (string name ".db"))
            [(write-fixture dir (string name ".lisp") src) ;extra]))

  (defn run-green [name src extra]
    (let [r (run-on elle-test name src extra)]
      (assert (= r:exit 0)
              (string name ": the fixture must gate green; stderr: " r:err))
      r:db))

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

  # From the scratch directory, which holds no tests/ledger: from the
  # repository's, the fixture's residue has no row and reads as unledgered.
  (eprintln "scenario: an isolated child records its total, and an import keeps it")
  (let [r (rig-test (path/join dir "isolate.db")
                    [(write-fixture dir "isolate.lisp"
                                    "(ev/sleep 0.3)\n(assert true \"slept\")\n")
                     "--isolate" ""] :cwd dir)
        db r:db
        here (path/join dir "here.db")
        [costed total] (latest db
                               (string "tier = 'process' AND wall_ms >= 300 "
                                       "AND cpu_us > 0 AND max_rss_kb > 0"))]
    (assert (= r:exit 0) (string "the fixture must gate green; stderr: " r:err))
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

  (eprintln "all result-cost scenarios passed"))
