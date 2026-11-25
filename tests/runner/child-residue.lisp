(elle/epoch 18)
## audited: 2026-11-23
## An isolated child records the regions its teardown left alive, read from the last stats line it printed, and NULL when it printed none.
## tests/runner/overview.md
## docs/test-cost.md
## docs/test-store.md
##
## The counter-factual: a runner that never asks a child for its stats records
## nothing, and one that reads the first stats line records whatever the
## program printed. A runner that answers 0 for a child that printed no line
## reads a killed child as clean.

(include-file "lib/harness.lisp")

(def residue-line "[stats] live regions after teardown: ")

(with-temp-dir dir
  (defn isolate [name src extra]
    "`elle test --isolate ''` over one fixture holding SRC, with EXTRA flags,
     into a store of its own."
    (elle-test (path/join dir (string name ".db"))
               ["--isolate" "" ;extra
                (write-fixture dir (string name ".lisp") src)]))

  (defn host [name script]
    "A host program that runs SCRIPT, a body of sh, in place of a child."
    (let [path (write-fixture dir (string name ".sh")
                              (string "#!/bin/sh\n" script "\n"))]
      (subprocess/system "chmod" ["755" path])
      path))

  (defn hosted [name script]
    "`elle test --host HOST --isolate ''` over one passing fixture, where HOST
     runs SCRIPT."
    (elle-test (path/join dir (string name ".db"))
               ["--host" (host name script) "--isolate" ""
                (write-fixture dir (string name ".lisp")
                               "(assert true \"ok\")\n")]))

  (defn residue [db]
    "The residue column of the latest run's one result."
    (let [[_ total] (latest db "1")]
      (assert (= total 1) (string "one path is one row, got " total)))
    (get (get (rows db
                    (string "SELECT residue_regions AS n FROM result WHERE run_id = "
                            latest-run)) 0) :n))

  (eprintln "scenario: a child records the residue its teardown left")
  (let [r (isolate "plain" "(assert true \"ok\")\n" [])]
    (assert (= r:exit 0) (string "the child must pass; stderr: " r:err))
    (assert (integer? (residue r:db))
            (string "the child's row records its residue, got " (residue r:db))))

  # The program prints a line that looks like the teardown's. The teardown runs
  # after the program, so its own line comes last.
  (eprintln "scenario: the last stats line is the child's own")
  (let [r (isolate "mimic"
                   (string "(eprintln \"" residue-line "999\")\n"
                           "(assert true \"ok\")\n") [])]
    (assert (= r:exit 0) (string "the child must pass; stderr: " r:err))
    (let [n (residue r:db)]
      (assert (and (integer? n) (not (= n 999)))
              (string "the row records the teardown's count, not the program's: "
                      n))))

  (eprintln "scenario: the count is read from the line")
  (let [r (hosted "seven"
                  (string "echo '" residue-line "999' >&2\n" "echo '"
                          residue-line
                          "7 (0 = clean; residue names open leaks)' >&2\nexit 0"))]
    (assert (= r:exit 0) (string "the host exits 0; stderr: " r:err))
    (assert (= (residue r:db) 7)
            (string "the row records the last line's count: " (residue r:db)))
    (eprintln "scenario: an import keeps the residue")
    (let [here (path/join dir "here.db")
          i (elle-test here ["--import" r:db])]
      (assert (= i:exit 0) (string "the import failed; stderr: " i:err))
      (assert (= (residue here) 7)
              (string "the imported row carries the residue: " (residue here)))))

  (eprintln "scenario: a child that prints no line records NULL")
  (let [r (hosted "silent" "exit 0")]
    (assert (= r:exit 0) (string "the host exits 0; stderr: " r:err))
    (assert (nil? (residue r:db))
            (string "a silent child records no residue: " (residue r:db))))

  # os/exit ends the process before the teardown runs.
  (eprintln "scenario: a child that ends in os/exit records NULL")
  (let [r (isolate "exits" "(os/exit 0)\n" [])]
    (assert (= r:exit 0) (string "the child exits 0; stderr: " r:err))
    (assert (nil? (residue r:db))
            (string "a child with no teardown records no residue: "
                    (residue r:db))))

  (eprintln "scenario: a killed child records NULL")
  (let [r (isolate "slow" "(ev/sleep 5)\n" ["--timeout" "500"])]
    (assert (not (= r:exit 0)) "a timeout gates non-zero")
    (assert (nil? (residue r:db))
            (string "a killed child records no residue: " (residue r:db))))

  (eprintln "scenario: a form in a worker records NULL")
  (let [db (path/join dir "worker.db")
        r (elle-test db
                     [(write-fixture dir "worker.lisp" "(assert true \"ok\")\n")])]
    (assert (= r:exit 0) (string "the form must pass; stderr: " r:err))
    (every-row db "residue_regions IS NULL"
               "no row of an in-process run records a residue"))

  # SQLite drops a column on request, which is how the older table is made.
  (eprintln "scenario: a store from before residue_regions gains the column")
  (let [db (path/join dir "migrate.db")]
    (query! db "ALTER TABLE result DROP COLUMN residue_regions")
    (let [r (elle-test db
                       ["--isolate" ""
                        (write-fixture dir "migrate.lisp"
                                       "(assert true \"ok\")\n")])]
      (assert (= r:exit 0) (string "the run must gate green; stderr: " r:err))
      (assert (integer? (residue db))
              (string "the migrated store records the residue: " (residue db)))))

  (eprintln "all child-residue scenarios passed"))
