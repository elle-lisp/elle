(elle/epoch 18)
## audited: 2026-11-22
## Each result records what it cost: wall and CPU time for a form in a worker, and a child's total, peak memory included.
## tests/runner/overview.md
## docs/test-store.md
##
## The counter-factual: a runner that records no cost still passes every corpus
## test, and a store full of NULLs answers "which file is slow on this build"
## with nothing. A timeout that fails in one CI job and nowhere else is then a
## guess.

(include-file "lib/harness.lisp")

(with-temp-dir dir
  (defn run [name src extra]
    "Run one fixture holding SRC, with EXTRA flags, into a store of its own."
    (elle-test (path/join dir (string name ".db"))
               [(write-fixture dir (string name ".lisp") src) ;extra]))

  (defn run-green [name src extra]
    (let [r (run name src extra)]
      (assert (= r:exit 0)
              (string name ": the fixture must gate green; stderr: " r:err))
      r:db))

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

  (eprintln "scenario: a single form records its cost on every tier")
  (every-row (run-green "form" "(assert (> 2 1) \"ordered\")\n" [])
             "wall_ms IS NOT NULL AND cpu_us IS NOT NULL"
             "every tier's row records wall and CPU time")

  # Its worker never handed a CPU time back.
  (eprintln "scenario: a timeout records its wall time and no CPU time")
  (let [r (run "timeout" "(ev/sleep 5)\n(assert true \"never\")\n"
               ["--timeout" "500"])]
    (assert (not (= r:exit 0)) "a timeout must gate non-zero")
    (every-row r:db "status = 'timeout' AND wall_ms >= 500 AND cpu_us IS NULL"
               "every row is a timeout that waited out its budget and holds no CPU time"))

  # The row ran nothing. The counter-factual is a zero, which reads as a form
  # that ran in no time. A file that does not read never reaches this row.
  (eprintln "scenario: a file-level error records no cost")
  (let [r (run "file-error" "(assert (no-such-binding) \"unbound\")\n" [])]
    (assert (not (= r:exit 0)) "a file that does not compile gates non-zero")
    (every-row r:db "wall_ms IS NULL AND cpu_us IS NULL AND max_rss_kb IS NULL"
               "a file-level error leaves every cost column NULL"))

  (eprintln "scenario: an isolated child records its total, and an import keeps it")
  (let [db (run-green "isolate" "(ev/sleep 0.3)\n(assert true \"slept\")\n"
                      ["--isolate" ""])
        here (path/join dir "here.db")
        [costed total] (latest db
                               (string "tier = 'process' AND wall_ms >= 300 "
                                       "AND cpu_us > 0 AND max_rss_kb > 0"))]
    (assert (= total 1) "one path under --isolate is one row")
    (assert (= costed 1)
            "the child's row records its wall time, CPU total and peak resident set")
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
