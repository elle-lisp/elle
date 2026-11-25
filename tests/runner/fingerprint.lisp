(elle/epoch 18)
## audited: 2026-11-22
## A run row names the binary that produced it, so a verdict belongs to a build and not merely to a commit.
## tests/runner/overview.md
## docs/test-store.md
##
## The counter-factual: a result recorded the form and the commit and nothing
## about the executable. Two runs of two compilers read alike, no cached green
## could prove which binary earned it, and a search across a compiler change
## had nothing to group by.
##
## The trap: the fingerprint has to be the binary's own answer. A column filled
## from the version string would agree across every build of one release, which
## is the case a cache must not reuse.

(include-file "lib/harness.lisp")

(with-temp-dir dir
  (def passes (write-fixture dir "pass.lisp" "(assert true \"ok\")\n"))

  (defn run-once [db]
    "Run the passing fixture into DB, and require a green gate."
    (let [r (elle-test db [passes])]
      (assert (= r:exit 0) (string "a passing form gates green; stderr: " r:err))))

  (eprintln "scenario: a run row carries the binary's fingerprint as a number")
  (let [db (path/join dir "row.db")]
    (run-once db)
    (let [row (get (rows db
                         (string "SELECT typeof(boot_fingerprint) AS kind "
                                 "FROM run WHERE id = " latest-run)) 0)]
      (assert (= row:kind "integer")
              (string "the fingerprint is a 64-bit number this box names, got "
                      row:kind))))

  # A fingerprint that moved between two runs of one executable keys nothing.
  (eprintln "scenario: two runs of one binary record one fingerprint")
  (let [db (path/join dir "stable.db")]
    (run-once db)
    (run-once db)
    (let [row (get (rows db
                         (string "SELECT count(*) AS runs, "
                                 "count(DISTINCT boot_fingerprint) AS prints "
                                 "FROM run")) 0)]
      (assert (and (= row:runs 2) (= row:prints 1))
              (string "two runs of one binary share one fingerprint, got " row))))

  (eprintln "scenario: the binary reports the fingerprint the run recorded")
  (let [db (path/join dir "primitive.db")
        r (elle-run ["-e" "(print (elle/boot-fingerprint))"])]
    (run-once db)
    (assert (= r:exit 0) (string "(elle/boot-fingerprint) failed: " r:err))
    (assert (= (parse-int (string/trim r:out))
               (scalar db
                       (string "SELECT boot_fingerprint AS c FROM run "
                               "WHERE id = " latest-run)))
            (string "the run recorded what the binary reports, "
                    (string/trim r:out))))

  (eprintln "all fingerprint scenarios passed"))
