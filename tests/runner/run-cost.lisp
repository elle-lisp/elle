(elle/epoch 18)
## audited: 2026-11-23
## A run records what its process cost after each file; a run killed in its first file records none, an old store gains the columns, and an import keeps them.
## tests/runner/overview.md
## docs/test-store.md
##
## The counter-factual: a runner that reads its cost from the calling thread
## alone, as `clock/cpu` would, misses every worker and the JIT's thread, and
## reads a few milliseconds for a run that burned seconds. A runner that
## stamped the cost at insert, or before each file, would give a run killed in
## its first file a peak that no file's work produced. A runner that wrote the
## cost at completion alone gives the OOM killer's run no peak at all.

(include-file "lib/harness.lisp")

(with-temp-dir dir
  (defn latest? [db cond]
    "Whether the latest run's row meets COND, which names the row `r`."
    (= 1
       (scalar db
               (string "SELECT count(*) AS c FROM run r WHERE r.id = "
                       latest-run " AND " cond))))

  (defn cost [db]
    "The latest run's cost columns, for a failure to show."
    (string (rows db
                  (string "SELECT wall_ms, max_rss_kb, cpu_user_ms, cpu_sys_ms "
                          "FROM run WHERE id = " latest-run))))

  (defn kill-after [db files]
    "Run FILES into DB, the last of which SIGKILLs the runner, and require the
     kill."
    (let [r (elle-test db
                       [;files
                        (write-fixture dir "kills.lisp"
                                       "(os/sig-raise :sigkill)\n")])]
      (assert (= r:exit -9)
              (string "the fixture must SIGKILL the runner; exit " r:exit
                      " stderr: " r:err))))

  (defn run-green [db src]
    "Run SRC as a fixture into DB, and require a green gate."
    (let [r (elle-test db [(write-fixture dir "fixture.lisp" src)])]
      (assert (= r:exit 0)
              (string "the fixture must gate green; stderr: " r:err))))

  # Booting a runtime alone costs user time, so a zero is a reading that was
  # never taken.
  (eprintln "scenario: a finished run records its peak and its CPU time")
  (let [db (path/join dir "done.db")]
    (run-green db "(assert true \"ok\")\n")
    (assert (latest? db
                     (string "r.max_rss_kb > 0 AND r.cpu_user_ms > 0 "
                             "AND r.cpu_sys_ms >= 0"))
            (string "the run row must carry a peak and both CPU times: "
                    (cost db))))

  # A multi-form file runs once per JIT policy, each run in a worker that spins
  # for 1 s of its own CPU time, so the results alone hold at least 1 s. Each
  # column is truncated to whole milliseconds, which 2 ms of slack absorbs.
  #
  # The trap: the runner's own thread burns about 0.6 s on a build without
  # fat LTO, so a spin shorter than that passes against a runner that read its
  # own thread's clock alone.
  (eprintln "scenario: the run counts the CPU time of its workers")
  (let [db (path/join dir "workers.db")
        workers "(SELECT sum(cpu_us) FROM result WHERE run_id = r.id)"]
    (run-green db
               (string "(def t0 (clock/cpu))\n"
                       "(while (< (- (clock/cpu) t0) 1.0) nil)\n"
                       "(assert true \"spun\")\n"))
    (assert (latest? db
                     (string workers " >= 1000000 AND "
                             "(r.cpu_user_ms + r.cpu_sys_ms + 2) * 1000 >= "
                             workers))
            (string "the run's CPU time must cover every worker's: " (cost db)
                    " workers "
                    (rows db
                          (string "SELECT sum(cpu_us) AS us FROM result "
                                  "WHERE run_id = " latest-run)))))

  # The fixture SIGKILLs the runner, which stands in for the OOM killer.
  (eprintln "scenario: a run killed in its first file records no cost")
  (let [db (path/join dir "killed.db")]
    (kill-after db [])
    (assert (latest? db
                     (string "r.finished_at IS NULL AND r.wall_ms IS NULL "
                             "AND r.max_rss_kb IS NULL "
                             "AND r.cpu_user_ms IS NULL "
                             "AND r.cpu_sys_ms IS NULL"))
            (string "a run killed before any file ended must leave all four "
                    "cost columns NULL: " (cost db))))

  (eprintln "scenario: a run killed after a file keeps that file's cost")
  (let [db (path/join dir "killed-later.db")]
    (kill-after db [(write-fixture dir "passes.lisp" "(assert true \"ok\")\n")])
    (assert (latest? db
                     (string "r.finished_at IS NULL AND r.wall_ms > 0 "
                             "AND r.max_rss_kb > 0 AND r.cpu_user_ms > 0 "
                             "AND r.cpu_sys_ms >= 0"))
            (string "a killed run must keep the cost its finished file "
                    "recorded: " (cost db))))

  # SQLite drops a column on request, which is how the older table is made.
  (eprintln "scenario: a store from before the cost columns gains them")
  (let [db (path/join dir "migrate.db")]
    (each col ["max_rss_kb" "cpu_user_ms" "cpu_sys_ms"]
      (query! db (string "ALTER TABLE run DROP COLUMN " col)))
    (run-green db "(assert true \"ok\")\n")
    (assert (latest? db "r.max_rss_kb > 0 AND r.cpu_user_ms > 0")
            (string "the migrated store must record the run's cost: " (cost db))))

  (eprintln "scenario: an import keeps the run's cost")
  (let [db (path/join dir "far.db")
        here (path/join dir "here.db")]
    (run-green db "(assert true \"ok\")\n")
    (let [r (elle-test here ["--import" db])]
      (assert (= r:exit 0) (string "the import failed; stderr: " r:err)))
    (assert (latest? here "r.max_rss_kb > 0 AND r.cpu_user_ms > 0")
            (string "the imported run must carry its cost: " (cost here))))

  (eprintln "all run-cost scenarios passed"))
